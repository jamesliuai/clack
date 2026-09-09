//! Deterministic, terminal-independent reducer. Times are monotonic receipt microseconds.
mod model;
pub use model::*;

/// Shared Ready-state eligibility for the pure reducer and timestamping reader.
/// Commands/modifier bindings are resolved before calling this helper.
pub fn eligible_text_start(mode: Mode, policy: Policy, text: &str) -> bool {
    text.chars().any(|character| {
        let literal_whitespace =
            (mode == Mode::Zen || policy == Policy::Exact) && matches!(character, '\t' | '\n');
        (!character.is_control() || literal_whitespace)
            && (mode == Mode::Zen || policy == Policy::Exact || !character.is_whitespace())
    })
}
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use unicode_normalization::{UnicodeNormalization, char::canonical_combining_class};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Match {
    Correct,
    Prefix,
    Wrong,
}

/// Inline, bounded grapheme storage keeps the warmed ASCII reducer allocation-free.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    bytes: [u8; 128],
    len: u16,
    width: u16,
}
impl std::fmt::Debug for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}
impl Unit {
    pub fn new(s: &str) -> Result<Self, String> {
        if s.len() > 128 || s.chars().count() > 32 {
            return Err("input grapheme exceeds 32 Unicode scalars".into());
        }
        let mut u = Self {
            bytes: [0; 128],
            len: s.len() as u16,
            width: UnicodeWidthStr::width(s) as u16,
        };
        u.bytes[..s.len()].copy_from_slice(s.as_bytes());
        Ok(u)
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).expect("constructed from UTF-8")
    }
    pub fn width(&self) -> usize {
        usize::from(self.width)
    }
    fn extended_by(&self, c: char) -> bool {
        let mut buffer = [0u8; 132];
        let len = self.len as usize;
        buffer[..len].copy_from_slice(&self.bytes[..len]);
        let mut scalar = [0; 4];
        let encoded = c.encode_utf8(&mut scalar);
        buffer[len..len + encoded.len()].copy_from_slice(encoded.as_bytes());
        std::str::from_utf8(&buffer[..len + encoded.len()])
            .expect("constructed from UTF-8")
            .graphemes(true)
            .count()
            == 1
    }
    fn append(self, c: char) -> Option<Self> {
        let mut out = self;
        let mut buf = [0; 4];
        let bytes = c.encode_utf8(&mut buf).as_bytes();
        let len = self.len as usize;
        if len + bytes.len() > 128 || self.as_str().chars().count() >= 32 {
            return None;
        }
        out.bytes[len..len + bytes.len()].copy_from_slice(bytes);
        out.len += bytes.len() as u16;
        out.width = UnicodeWidthStr::width(out.as_str()) as u16;
        Some(out)
    }
}
#[derive(Debug, Clone)]
pub struct Entered {
    pub unit: Unit,
    pub correct: bool,
    pub assisted: bool,
}
#[derive(Debug, Clone)]
pub struct Token {
    pub target_start: usize,
    pub layout_revision: u64,
    pub target: Arc<[Unit]>,
    pub entered: Vec<Entered>,
    pub separator: bool,
    pub committed: bool,
    pub attempts: u64,
    pub errors: u64,
    pub corrected: bool,
    pub elapsed_us: u64,
    pub label: String,
    wrong: u64,
    manual: u64,
    contribution: u64,
    missed: u64,
}
impl Token {
    fn new<'a>(s: &'a str, prepared: &mut HashMap<&'a str, Arc<[Unit]>>) -> Self {
        // Practice may repeat a maximum-size custom token 25 times. Its immutable
        // Unicode metadata is shared; correction storage grows only as it is used.
        let target = prepared
            .entry(s)
            .or_insert_with(|| {
                s.graphemes(true)
                    .map(|g| Unit::new(g).expect("validated target"))
                    .collect()
            })
            .clone();
        Self {
            target_start: 0,
            layout_revision: 0,
            entered: Vec::with_capacity(target.len().min(256) + 32),
            target,
            separator: false,
            committed: false,
            attempts: 0,
            errors: 0,
            corrected: false,
            elapsed_us: 0,
            label: s.to_owned(),
            wrong: 0,
            manual: 0,
            contribution: 0,
            missed: 0,
        }
    }
    pub fn correct(&self) -> bool {
        self.wrong == 0 && self.entered.len() == self.target.len()
    }
    fn prefix_credit(&self) -> u64 {
        if self.wrong == 0 && self.entered.len() <= self.target.len() {
            self.manual
        } else {
            0
        }
    }
}
#[derive(Debug, Clone, Copy)]
struct Pending {
    unit: Unit,
    token: usize,
    index: usize,
    matched: Match,
    counted: bool,
    retained: bool,
}
#[derive(Debug, Clone, Copy)]
pub enum Action<'a> {
    Text(&'a str),
    Backspace,
    DeleteWord,
    Finish,
    Tick,
    Paste,
    FocusLost,
    Abort,
    Interrupt(&'a str),
    Overload,
}
#[derive(Debug, Clone)]
pub struct Engine {
    spec: TestSpec,
    state: State,
    tokens: Vec<Token>,
    current: usize,
    edit_stack: Vec<usize>,
    counts: Counts,
    committed_credit: u64,
    start_us: Option<u64>,
    now_us: u64,
    end_us: Option<u64>,
    outcome: Outcome,
    reason: Option<String>,
    integrity: Integrity,
    pending: Option<Pending>,
    samples: VecDeque<Sample>,
    next_bucket: u64,
    sample_attempts: u64,
    sample_errors: u64,
    rate_count: u64,
    rate_sum: f64,
    rate_squares: f64,
    token_start_us: u64,
    zen: VecDeque<Entered>,
    zen_discarded: u64,
}
impl Engine {
    pub fn new(spec: TestSpec, text: &str) -> Result<Self, String> {
        spec.validate()?;
        if spec.mode != Mode::Zen && text.is_empty() {
            return Err("empty target".into());
        }
        for g in text.graphemes(true) {
            Unit::new(g)?;
        }
        let mut tokens = Vec::new();
        let mut prepared = HashMap::new();
        if spec.mode != Mode::Zen {
            if spec.policy == Policy::Prose {
                tokens.extend(
                    text.split_whitespace()
                        .map(|word| Token::new(word, &mut prepared)),
                );
            } else {
                let mut start = 0;
                // A word ends at a literal whitespace unit. Leading indentation is its own segment.
                let mut in_word = false;
                for (i, g) in text.grapheme_indices(true) {
                    let whitespace = g.chars().all(char::is_whitespace);
                    if !whitespace && !in_word && i > start {
                        tokens.push(Token::new(&text[start..i], &mut prepared));
                        start = i;
                    }
                    if whitespace && in_word {
                        let end = i + g.len();
                        tokens.push(Token::new(&text[start..end], &mut prepared));
                        start = end;
                    }
                    in_word = !whitespace;
                }
                if start < text.len() {
                    tokens.push(Token::new(&text[start..], &mut prepared));
                }
            }
            if tokens.is_empty() {
                return Err("empty normalized target".into());
            }
        }
        let mut target_start = 0;
        for token in &mut tokens {
            token.target_start = target_start;
            target_start += token.target.len() + usize::from(spec.policy == Policy::Prose);
        }
        let edit_stack = Vec::with_capacity(tokens.len());
        Ok(Self {
            spec,
            state: State::Ready,
            tokens,
            current: 0,
            edit_stack,
            counts: Counts::default(),
            committed_credit: 0,
            start_us: None,
            now_us: 0,
            end_us: None,
            outcome: Outcome::Complete,
            reason: None,
            integrity: Integrity::default(),
            pending: None,
            samples: VecDeque::with_capacity(SAMPLE_CAPACITY),
            next_bucket: 1,
            sample_attempts: 0,
            sample_errors: 0,
            rate_count: 0,
            rate_sum: 0.0,
            rate_squares: 0.0,
            token_start_us: 0,
            zen: VecDeque::with_capacity(ZEN_WINDOW + 1),
            zen_discarded: 0,
        })
    }
    pub fn logical_target_position(&self) -> usize {
        self.tokens.get(self.current).map_or_else(
            || self.prepared_target_units(),
            |token| token.target_start + token.entered.len().min(token.target.len()),
        )
    }
    pub fn prepared_target_units(&self) -> usize {
        self.tokens
            .last()
            .map_or(0, |token| token.target_start + token.target.len())
    }
    pub fn spec(&self) -> &TestSpec {
        &self.spec
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }
    pub fn current_token(&self) -> usize {
        self.current
    }
    pub fn zen_window(&self) -> &VecDeque<Entered> {
        &self.zen
    }
    pub fn zen_discarded(&self) -> u64 {
        self.zen_discarded
    }
    pub fn started_at(&self) -> Option<u64> {
        self.start_us
    }
    pub fn deadline(&self) -> Option<u64> {
        if self.spec.mode == Mode::Time {
            self.start_us
                .map(|s| s + u64::from(self.spec.seconds) * 1_000_000)
        } else {
            None
        }
    }
    pub fn elapsed_us(&self) -> u64 {
        self.start_us
            .map_or(0, |s| self.end_us.unwrap_or(self.now_us).saturating_sub(s))
    }
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
    pub fn integrity(&self) -> &Integrity {
        &self.integrity
    }
    pub fn counts(&self) -> Counts {
        let mut c = self.counts;
        c.credited_units = if self.spec.mode == Mode::Zen {
            0
        } else {
            self.committed_credit
                + self
                    .tokens
                    .get(self.current)
                    .filter(|t| !t.committed)
                    .map_or(0, Token::prefix_credit)
        };
        c
    }
    pub fn metrics(&self) -> Metrics {
        let mut m = Metrics::calculate(
            self.counts(),
            self.elapsed_us(),
            self.spec.mode == Mode::Zen,
            self.state != State::Results,
        );
        if self.rate_count >= 5 && self.rate_sum > 0.0 {
            let mean = self.rate_sum / self.rate_count as f64;
            let variance = (self.rate_squares / self.rate_count as f64 - mean * mean).max(0.0);
            m.consistency = Some(100.0 / (1.0 + variance.sqrt() / mean));
        }
        m
    }
    pub fn samples(&self) -> &VecDeque<Sample> {
        &self.samples
    }
    /// Content preparation/extension occurs outside apply; callers maintain deterministic lookahead.
    pub fn append_prepared(&mut self, text: &str) -> Result<(), String> {
        if self.spec.mode != Mode::Time || self.spec.policy != Policy::Prose {
            return Err("only timed prose accepts lookahead chunks".into());
        }
        for g in text.graphemes(true) {
            Unit::new(g)?;
        }
        let mut target_start = self
            .tokens
            .last()
            .map_or(0, |token| token.target_start + token.target.len() + 1);
        let mut prepared = HashMap::new();
        for word in text.split_whitespace() {
            let mut token = Token::new(word, &mut prepared);
            token.target_start = target_start;
            target_start += token.target.len() + 1;
            self.tokens.push(token);
        }
        self.edit_stack
            .reserve(self.tokens.len().saturating_sub(self.edit_stack.capacity()));
        Ok(())
    }
    pub fn apply(&mut self, action: Action<'_>, receipt_us: u64) {
        if self.state == State::Results {
            return;
        }
        // Receipt order is part of the adapter contract; never permit a clock to go backwards.
        self.now_us = self.now_us.max(receipt_us);
        if self.state == State::Running {
            if let Some(deadline) = self.deadline()
                && receipt_us >= deadline
            {
                self.advance_samples(deadline);
                self.finish(Outcome::Complete, None, deadline);
                return;
            }
            self.advance_samples(self.now_us);
            if self.state == State::Results {
                return;
            }
        }
        match action {
            Action::Text(text) => {
                let mut characters = text.chars().peekable();
                while let Some(c) = characters.next() {
                    if self.state == State::Results {
                        break;
                    }
                    self.insert(c, characters.peek().copied());
                }
            }
            Action::Backspace if self.state == State::Running => {
                self.finalize_pending();
                if self.state == State::Running {
                    self.delete_one();
                }
            }
            Action::DeleteWord if self.state == State::Running => {
                self.finalize_pending();
                if self.state == State::Running {
                    self.delete_word();
                }
            }
            Action::Finish if self.state == State::Running => {
                self.finalize_pending();
                if self.state != State::Running {
                    return;
                }
                if self.spec.mode == Mode::Zen {
                    self.finish(Outcome::Complete, None, self.now_us);
                } else if self.spec.policy == Policy::Exact
                    && self.current + 1 == self.tokens.len()
                    && self.tokens[self.current].entered.len()
                        >= self.tokens[self.current].target.len()
                {
                    if self.spec.rules.stop_on_error == StopOnError::Word
                        && !self.tokens[self.current].correct()
                    {
                        return;
                    }
                    self.commit();
                    if self.state == State::Running {
                        self.finish(Outcome::Complete, None, self.now_us);
                    }
                } else {
                    self.finish(
                        Outcome::Incomplete,
                        Some("finished before the target was complete"),
                        self.now_us,
                    );
                }
            }
            Action::Paste if self.state == State::Running => self.integrity.paste_attempted = true,
            Action::FocusLost if self.state == State::Running => self.integrity.focus_lost = true,
            Action::Abort if self.state == State::Running => self.finish(
                Outcome::Aborted,
                Some("opened commands or restarted"),
                self.now_us,
            ),
            Action::Interrupt(reason) if self.state == State::Running => {
                self.integrity.clock_interrupted =
                    reason.contains("clock") || reason.contains("suspend");
                self.finish(Outcome::Interrupted, Some(reason), self.now_us);
            }
            Action::Overload if self.state == State::Running => {
                self.integrity.input_overload = true;
                self.finish(
                    Outcome::Interrupted,
                    Some("input overload: the ordered event queue filled"),
                    self.now_us,
                );
            }
            _ => {}
        }
    }
    fn insert(&mut self, c: char, following: Option<char>) {
        if c.is_control() && !(self.spec.policy == Policy::Exact || self.spec.mode == Mode::Zen) {
            return;
        }
        if c.is_control() && !matches!(c, '\t' | '\n') {
            return;
        }
        if self.state == State::Ready {
            let mut bytes = [0; 4];
            if !eligible_text_start(self.spec.mode, self.spec.policy, c.encode_utf8(&mut bytes)) {
                return;
            }
            self.state = State::Running;
            self.start_us = Some(self.now_us);
            self.token_start_us = self.now_us;
        }
        // A scalar can extend the last received grapheme. Only this trailing unit is reconsidered.
        if let Some(old) = self.pending {
            if let Some(combined) = old.unit.append(c) {
                if combined.as_str().graphemes(true).count() == 1 {
                    self.revise_pending(old, combined, following);
                    return;
                }
            } else if old.unit.extended_by(c) {
                self.finish(
                    Outcome::Interrupted,
                    Some("input grapheme exceeds 32 Unicode scalars"),
                    self.now_us,
                );
                return;
            }
        }
        self.finalize_pending();
        if self.state != State::Running {
            return;
        }
        if self.spec.mode == Mode::Zen {
            self.insert_zen(c);
            return;
        }
        if self.current >= self.tokens.len() {
            self.finish(
                Outcome::Interrupted,
                Some("prepared content exhausted"),
                self.now_us,
            );
            return;
        }
        if self.spec.policy == Policy::Prose
            && c == ' '
            && following.is_none_or(|next| !Unit::new(" ").expect("space").extended_by(next))
        {
            self.submit_prose();
            return;
        }
        if self.spec.policy == Policy::Prose && c.is_whitespace() && c != ' ' {
            return;
        }
        let mut buf = [0; 4];
        let unit = Unit::new(c.encode_utf8(&mut buf)).expect("one scalar");
        let index = self.tokens[self.current].entered.len();
        if index >= self.tokens[self.current].target.len() + TOKEN_EXTRA_LIMIT {
            self.finish(
                Outcome::Interrupted,
                Some("current token exceeds the bounded extra-input window"),
                self.now_us,
            );
            return;
        }
        let matched = self.match_unit(unit, self.current, index);
        let counted = matched != Match::Prefix;
        if counted {
            self.count_attempt(self.current, matched == Match::Correct, 1);
        }
        let retained =
            matched != Match::Wrong || self.spec.rules.stop_on_error != StopOnError::Letter;
        if retained {
            self.push_unit(self.current, unit, matched == Match::Correct, false);
        }
        self.pending = Some(Pending {
            unit,
            token: self.current,
            index,
            matched,
            counted,
            retained,
        });
        self.after_text_scalar(following);
    }
    fn after_text_scalar(&mut self, following: Option<char>) {
        // Associated text is already known in full. Delay completion and
        // challenge checks while the next scalar extends this same grapheme.
        // Examine the actual trailing cluster, including previous events, so
        // regional-indicator parity and ZWJ context are not guessed per payload.
        if following.is_some_and(|next| {
            self.pending
                .is_some_and(|pending| pending.unit.extended_by(next))
        }) {
            return;
        }
        if self
            .pending
            .is_some_and(|pending| pending.matched == Match::Wrong)
            && self.spec.rules.difficulty == Difficulty::Master
        {
            self.finish(
                Outcome::Failed,
                Some("master: incorrect text attempt"),
                self.now_us,
            );
            return;
        }
        self.check_accuracy();
        if self.state == State::Running {
            self.after_insertion();
        }
    }
    fn match_unit(&self, unit: Unit, token: usize, index: usize) -> Match {
        let Some(expected) = self.tokens[token].target.get(index) else {
            return Match::Wrong;
        };
        if unit == *expected {
            return Match::Correct;
        }
        if unit.as_str().is_ascii() && expected.as_str().is_ascii() {
            return Match::Wrong;
        }
        let normalize = self.spec.policy == Policy::Prose || self.spec.normalize;
        if normalize {
            if unit.as_str().nfc().eq(expected.as_str().nfc()) {
                return Match::Correct;
            }
            let input: Vec<_> = unit.as_str().nfd().collect();
            let target: Vec<_> = expected.as_str().nfd().collect();
            if input.len() < target.len() && input.first() == target.first() {
                // Later combining marks may reorder under canonical decomposition.
                // A matching subsequence can still extend to the target cluster.
                let mut position = 0;
                let possible = input.iter().all(|scalar| {
                    while position < target.len() && target[position] != *scalar {
                        let missing_class = canonical_combining_class(target[position]);
                        if missing_class == 0 || missing_class >= canonical_combining_class(*scalar)
                        {
                            return false;
                        }
                        position += 1;
                    }
                    if position == target.len() {
                        return false;
                    }
                    position += 1;
                    true
                });
                if possible {
                    return Match::Prefix;
                }
            }
        } else if expected.as_str().starts_with(unit.as_str()) {
            return Match::Prefix;
        }
        Match::Wrong
    }
    fn count_attempt(&mut self, token: usize, correct: bool, direction: i8) {
        let t = &mut self.tokens[token];
        if direction > 0 {
            self.counts.attempts_total += 1;
            t.attempts += 1;
            if correct {
                self.counts.attempts_correct += 1;
            } else {
                t.errors += 1;
            }
        } else {
            self.counts.attempts_total -= 1;
            t.attempts -= 1;
            if correct {
                self.counts.attempts_correct -= 1;
            } else {
                t.errors -= 1;
            }
        }
    }
    fn push_unit(&mut self, token: usize, unit: Unit, correct: bool, assisted: bool) {
        let t = &mut self.tokens[token];
        let extra = t.entered.len() >= t.target.len();
        if extra
            || t.target
                .get(t.entered.len())
                .is_some_and(|target| target.width() != unit.width())
        {
            t.layout_revision = t.layout_revision.wrapping_add(1);
        }
        if !assisted {
            t.manual += 1;
            self.counts.retained_units += 1;
        }
        if !correct {
            t.wrong += 1;
        }
        if extra {
            self.counts.final_extra += 1;
        } else if correct {
            self.counts.final_correct += 1;
        } else {
            self.counts.final_incorrect += 1;
        }
        t.entered.push(Entered {
            unit,
            correct,
            assisted,
        });
    }
    fn pop_unit(&mut self, token: usize) {
        let t = &mut self.tokens[token];
        if let Some(entry) = t.entered.pop() {
            if t.entered.len() >= t.target.len()
                || t.target
                    .get(t.entered.len())
                    .is_some_and(|target| target.width() != entry.unit.width())
            {
                t.layout_revision = t.layout_revision.wrapping_add(1);
            }
            if !entry.assisted {
                t.manual -= 1;
                self.counts.retained_units -= 1;
            }
            if !entry.correct {
                t.wrong -= 1;
            }
            if t.entered.len() >= t.target.len() {
                self.counts.final_extra -= 1;
            } else if entry.correct {
                self.counts.final_correct -= 1;
            } else {
                self.counts.final_incorrect -= 1;
            }
        }
    }
    fn revise_pending(&mut self, old: Pending, unit: Unit, following: Option<char>) {
        if self.spec.mode == Mode::Zen {
            if let Some(last) = self.zen.back_mut() {
                last.unit = unit;
            }
            self.pending = Some(Pending { unit, ..old });
            return;
        }
        if old.token != self.current || self.tokens[old.token].committed {
            return;
        }
        if old.counted {
            self.count_attempt(old.token, old.matched == Match::Correct, -1);
        }
        if old.retained {
            self.pop_unit(old.token);
        }
        let matched = self.match_unit(unit, old.token, old.index);
        let counted = matched != Match::Prefix;
        if counted {
            self.count_attempt(old.token, matched == Match::Correct, 1);
        }
        let retained =
            matched != Match::Wrong || self.spec.rules.stop_on_error != StopOnError::Letter;
        if retained {
            self.push_unit(old.token, unit, matched == Match::Correct, false);
        }
        self.pending = Some(Pending {
            unit,
            matched,
            counted,
            retained,
            ..old
        });
        self.after_text_scalar(following);
    }
    fn settle_pending(&mut self) -> bool {
        if let Some(p) = self.pending.take()
            && !p.counted
            && self.spec.mode != Mode::Zen
        {
            self.count_attempt(p.token, false, 1);
            if self.spec.rules.stop_on_error == StopOnError::Letter && p.retained {
                self.pop_unit(p.token);
            }
            return true;
        }
        false
    }
    fn finalize_pending(&mut self) {
        if self.settle_pending() && self.spec.rules.difficulty == Difficulty::Master {
            self.finish(
                Outcome::Failed,
                Some("master: incomplete grapheme attempt"),
                self.now_us,
            );
        }
        self.check_accuracy();
    }
    fn after_insertion(&mut self) {
        if self.current >= self.tokens.len() {
            return;
        }
        let t = &self.tokens[self.current];
        if self.spec.policy == Policy::Exact
            && t.entered.len() >= t.target.len()
            && self.pending.is_none_or(|p| p.matched != Match::Prefix)
        {
            if self.current + 1 < self.tokens.len() {
                if self.spec.rules.stop_on_error == StopOnError::Word && !t.correct() {
                    // The boundary unit is not retained until the segment is correct.
                    self.pop_unit(self.current);
                    if let Some(p) = &mut self.pending {
                        p.retained = false;
                    }
                    return;
                }
                let newline = t.entered.last().is_some_and(|e| e.unit.as_str() == "\n");
                self.finalize_pending();
                if self.state != State::Running {
                    return;
                }
                self.commit();
                if self.state == State::Running && self.spec.auto_indent && newline {
                    self.insert_indentation();
                }
            } else if self.spec.completion == Completion::Auto {
                if self.spec.rules.stop_on_error == StopOnError::Word && !t.correct() {
                    return;
                }
                self.finalize_pending();
                if self.state == State::Running {
                    self.commit();
                    if self.state == State::Running {
                        self.finish(Outcome::Complete, None, self.now_us);
                    }
                }
            }
        } else if self.spec.policy == Policy::Prose
            && self.spec.mode != Mode::Time
            && self.current + 1 == self.tokens.len()
            && t.correct()
        {
            self.finalize_pending();
            self.commit();
            if self.state == State::Running {
                self.finish(Outcome::Complete, None, self.now_us);
            }
        }
    }
    fn submit_prose(&mut self) {
        let t = &self.tokens[self.current];
        if t.entered.is_empty() {
            return;
        }
        let correct = t.entered.len() >= t.target.len();
        self.count_attempt(self.current, correct, 1);
        if self.spec.rules.difficulty == Difficulty::Master && !correct {
            self.finish(
                Outcome::Failed,
                Some("master: separator skipped target units"),
                self.now_us,
            );
            return;
        }
        if (self.spec.rules.stop_on_error == StopOnError::Word
            && !self.tokens[self.current].correct())
            || (self.spec.rules.stop_on_error == StopOnError::Letter && !correct)
        {
            self.check_accuracy();
            return;
        }
        self.tokens[self.current].separator = true;
        self.counts.retained_units += 1;
        self.counts.final_correct += 1;
        self.check_accuracy();
        if self.state != State::Running {
            return;
        }
        self.commit();
        if self.state == State::Running && self.current == self.tokens.len() {
            if self.spec.mode == Mode::Time {
                self.finish(
                    Outcome::Interrupted,
                    Some("prepared content exhausted"),
                    self.now_us,
                );
            } else {
                self.finish(Outcome::Complete, None, self.now_us);
            }
        }
        self.check_accuracy();
    }
    fn commit(&mut self) {
        let t = &mut self.tokens[self.current];
        if t.committed {
            return;
        }
        let correct = t.correct();
        t.contribution = if correct {
            t.manual + u64::from(t.separator)
        } else {
            0
        };
        t.missed = t.target.len().saturating_sub(t.entered.len()) as u64;
        t.elapsed_us = self.now_us.saturating_sub(self.token_start_us);
        self.committed_credit += t.contribution;
        self.counts.final_missed += t.missed;
        t.committed = true;
        self.edit_stack.push(self.current);
        self.current += 1;
        self.token_start_us = self.now_us;
        if !correct && self.spec.rules.difficulty == Difficulty::Expert {
            self.finish(
                Outcome::Failed,
                Some("expert: incorrect token submitted"),
                self.now_us,
            );
        }
    }
    fn insert_indentation(&mut self) {
        while self.current < self.tokens.len() {
            let i = self.tokens[self.current].entered.len();
            let Some(unit) = self.tokens[self.current].target.get(i).copied() else {
                break;
            };
            if !matches!(unit.as_str(), " " | "\t") {
                break;
            }
            self.push_unit(self.current, unit, true, true);
            if self.tokens[self.current].entered.len() == self.tokens[self.current].target.len()
                && self.current + 1 < self.tokens.len()
            {
                self.commit();
            }
        }
    }
    fn delete_one(&mut self) -> bool {
        if self.spec.rules.backspace == Backspace::None {
            return false;
        }
        if self.spec.mode == Mode::Zen {
            if self.spec.rules.backspace != Backspace::Full
                && self
                    .zen
                    .back()
                    .is_some_and(|entry| entry.unit.as_str().chars().all(char::is_whitespace))
            {
                return false;
            }
            if self.zen.pop_back().is_some() {
                self.counts.retained_units -= 1;
                self.counts.deletion_count += 1;
                return true;
            }
            return false;
        }
        if self.current < self.tokens.len() && !self.tokens[self.current].entered.is_empty() {
            self.pop_unit(self.current);
            self.tokens[self.current].corrected = true;
            self.counts.deletion_count += 1;
            return true;
        }
        let Some(&previous) = self.edit_stack.last() else {
            return false;
        };
        let allowed = match self.spec.rules.backspace {
            Backspace::Full => true,
            Backspace::Mistakes => !self.tokens[previous].correct(),
            _ => false,
        };
        if !allowed {
            return false;
        }
        self.edit_stack.pop();
        self.current = previous;
        let t = &mut self.tokens[previous];
        self.committed_credit -= t.contribution;
        self.counts.final_missed -= t.missed;
        t.contribution = 0;
        t.missed = 0;
        t.committed = false;
        t.corrected = true;
        if t.separator {
            t.separator = false;
            self.counts.retained_units -= 1;
            self.counts.final_correct -= 1;
            self.counts.deletion_count += 1;
            return true;
        }
        self.pop_unit(previous);
        self.counts.deletion_count += 1;
        true
    }
    fn delete_word(&mut self) {
        if self.spec.mode == Mode::Zen {
            while self
                .zen
                .back()
                .is_some_and(|e| e.unit.as_str().chars().all(char::is_whitespace))
            {
                if !self.delete_one() {
                    return;
                }
            }
            while self
                .zen
                .back()
                .is_some_and(|e| !e.unit.as_str().chars().all(char::is_whitespace))
            {
                if !self.delete_one() {
                    return;
                }
            }
            return;
        }
        let before = self.current;
        if !self.delete_one() {
            return;
        }
        let token = self.current;
        while self.current == token
            && self.current < self.tokens.len()
            && !self.tokens[self.current].entered.is_empty()
        {
            if !self.delete_one() {
                break;
            }
        }
        debug_assert!(self.current <= before);
    }
    fn insert_zen(&mut self, c: char) {
        let mut b = [0; 4];
        let unit = Unit::new(c.encode_utf8(&mut b)).expect("one scalar");
        self.zen.push_back(Entered {
            unit,
            correct: true,
            assisted: false,
        });
        self.counts.attempts_total += 1;
        self.counts.retained_units += 1;
        if self.zen.len() > ZEN_WINDOW {
            self.zen.pop_front();
            self.zen_discarded += 1;
        }
        self.pending = Some(Pending {
            unit,
            token: 0,
            index: 0,
            matched: Match::Correct,
            counted: true,
            retained: true,
        });
    }
    fn check_accuracy(&mut self) {
        if self.state != State::Running || self.spec.mode == Mode::Zen {
            return;
        }
        if let Some(min) = self.spec.rules.minimum_accuracy
            && self.counts.attempts_total >= 20
            && 100.0 * self.counts.attempts_correct as f64 / (self.counts.attempts_total as f64)
                < min
        {
            self.finish(
                Outcome::Failed,
                Some("minimum accuracy rule: cumulative accuracy below configured threshold"),
                self.now_us,
            );
        }
    }
    fn advance_samples(&mut self, until: u64) {
        let Some(start) = self.start_us else {
            return;
        };
        while start + self.next_bucket * 1_000_000 <= until && self.state == State::Running {
            let at = start + self.next_bucket * 1_000_000;
            self.record_sample(self.next_bucket - 1, 1_000_000, at - start);
            if self.next_bucket > 5
                && self.spec.mode != Mode::Zen
                && let Some(min) = self.spec.rules.minimum_wpm
                && 12_000_000.0 * self.counts().credited_units as f64 / ((at - start) as f64) < min
            {
                self.next_bucket += 1;
                self.finish(Outcome::Failed,Some("minimum WPM rule: cumulative WPM below configured threshold after grace period"),at);
                return;
            }
            self.next_bucket += 1;
        }
    }
    fn record_sample(&mut self, bucket_index: u64, duration_us: u64, elapsed: u64) {
        let counts = self.counts();
        let attempts = counts.attempts_total.saturating_sub(self.sample_attempts);
        let total_errors = if self.spec.mode == Mode::Zen {
            0
        } else {
            counts.attempts_total - counts.attempts_correct
        };
        let errors = total_errors.saturating_sub(self.sample_errors);
        if duration_us == 1_000_000 {
            self.rate_count += 1;
            self.rate_sum += attempts as f64;
            self.rate_squares += (attempts as f64).powi(2);
        }
        let metrics = Metrics::calculate(counts, elapsed, self.spec.mode == Mode::Zen, false);
        if self.samples.len() == SAMPLE_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            bucket_index,
            duration_us,
            counts,
            attempts,
            errors,
            metrics,
        });
        self.sample_attempts = counts.attempts_total;
        self.sample_errors = total_errors;
    }
    fn finish(&mut self, mut outcome: Outcome, mut reason: Option<&str>, at: u64) {
        if self.state == State::Results {
            return;
        }
        self.now_us = at;
        let incomplete_cluster = self.settle_pending();
        if !matches!(
            outcome,
            Outcome::Interrupted | Outcome::Aborted | Outcome::Failed
        ) {
            if incomplete_cluster && self.spec.rules.difficulty == Difficulty::Master {
                outcome = Outcome::Failed;
                reason = Some("master: incomplete grapheme attempt");
            } else if self.spec.mode != Mode::Zen
                && self.counts.attempts_total >= 20
                && self.spec.rules.minimum_accuracy.is_some_and(|minimum| {
                    100.0 * self.counts.attempts_correct as f64
                        / (self.counts.attempts_total as f64)
                        < minimum
                })
            {
                outcome = Outcome::Failed;
                reason =
                    Some("minimum accuracy rule: cumulative accuracy below configured threshold");
            }
        }
        self.state = State::Results;
        self.outcome = outcome;
        self.reason = reason.map(str::to_owned);
        self.end_us = Some(at);
        let elapsed = self.elapsed_us();
        let tail = elapsed % 1_000_000;
        if tail > 0 {
            self.record_sample(elapsed / 1_000_000, tail, elapsed);
        } else if elapsed > 0 {
            // A provisional unit finalized at a full-second endpoint belongs to
            // that last bucket, even when the deadline watermark arrives late.
            let counts = self.counts();
            if let Some(last) = self.samples.back_mut()
                && (last.bucket_index + 1) * 1_000_000 == elapsed
            {
                let old_attempts = last.attempts as f64;
                last.attempts += counts.attempts_total.saturating_sub(self.sample_attempts);
                let errors = if self.spec.mode == Mode::Zen {
                    0
                } else {
                    counts.attempts_total - counts.attempts_correct
                };
                last.errors += errors.saturating_sub(self.sample_errors);
                last.counts = counts;
                last.metrics =
                    Metrics::calculate(counts, elapsed, self.spec.mode == Mode::Zen, false);
                self.rate_sum += last.attempts as f64 - old_attempts;
                self.rate_squares += (last.attempts as f64).powi(2) - old_attempts.powi(2);
                self.sample_attempts = counts.attempts_total;
                self.sample_errors = errors;
            }
        }
    }
    pub fn snapshot(&self) -> ResultSnapshot {
        let counts = self.counts();
        let mut result = ResultSnapshot {
            export_version: 1,
            app_version: env!("CARGO_PKG_VERSION").into(),
            spec: self.spec.clone(),
            profile_key: self.spec.profile_key(),
            outcome: self.outcome,
            reason: self.reason.clone(),
            elapsed_us: self.elapsed_us(),
            counts,
            metrics: self.metrics(),
            integrity: self.integrity.clone(),
            samples: self.samples.iter().cloned().collect(),
            words: self
                .tokens
                .iter()
                .filter(|t| t.attempts > 0 || t.committed)
                .map(|t| WordSummary {
                    token: t.label.clone(),
                    entered: t.entered.iter().map(|e| e.unit.as_str()).collect(),
                    attempts: t.attempts,
                    errors: t.errors,
                    elapsed_us: t.elapsed_us,
                    expected_units: t.target.len() as u64,
                    corrected: t.corrected,
                    completed_correct: t.committed && t.correct(),
                })
                .collect(),
            personal_best_eligible: false,
        };
        result.personal_best_eligible =
            self.state == State::Results && result.eligible_for_standard_best();
        result
    }
    fn practice_words(&self, kind: crate::content::PracticeKind) -> Vec<String> {
        let tokens: Vec<_> = self
            .tokens
            .iter()
            .map(|token| crate::content::PracticeToken {
                target: token.label.clone(),
                expected_units: token.target.len(),
                elapsed_us: token.elapsed_us,
                correctly_completed: token.committed && token.correct(),
                had_incorrect_attempt: token.errors > 0,
                has_final_mistake_or_omission: token.committed && !token.correct(),
                had_correction: token.corrected,
            })
            .collect();
        crate::content::practice_candidates(&tokens, kind).unwrap_or_default()
    }
    pub fn missed_words(&self) -> Vec<String> {
        self.practice_words(crate::content::PracticeKind::Missed)
    }
    pub fn slow_words(&self) -> Vec<String> {
        self.practice_words(crate::content::PracticeKind::Slow)
    }
}
