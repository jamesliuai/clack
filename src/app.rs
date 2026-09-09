//! Single-owner engine/render scheduling. The reader supplies all scored time watermarks.
mod presentation;
mod settings_flow;
mod storage_flow;
use crate::{
    cli::Cli,
    config::{Config, Paths},
    engine::{Action, Outcome, Policy, ResultSnapshot, State},
    sample::{Sample, Samples},
    terminal::{CaretStyle, Control, InputKind, Reader, ReaderOptions, Session, SessionOptions},
    ui::{self, Caret, Focus, Viewport},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use presentation::Presentation;
use ratatui::{Terminal, backend::CrosstermBackend};
use serde::Serialize;
use settings_flow::Deferred;
#[cfg(feature = "test-hooks")]
use std::{fs::File, io::Write};
use std::{
    io::BufWriter,
    sync::{Arc, mpsc::TryRecvError},
    thread,
    time::{Duration, Instant},
};
use storage_flow::StorageFlow;
const FRAME_INTERVAL: Duration = Duration::from_micros(8334);
const BENCH_CAPACITY: usize = 65_536;

#[derive(Debug, Serialize)]
pub struct Bench {
    schema_version: u32,
    debug_assertions: bool,
    test_hooks_enabled: bool,
    first_usable_frame_us: Option<u64>,
    frame_flush_count: u64,
    input_received_count: u64,
    input_applied_count: u64,
    text_events_applied_count: u64,
    input_overload_count: u64,
    sequence_violation_count: u64,
    receive_to_flush_us: Vec<u64>,
    samples_total: u64,
    samples_dropped: u64,
    max_event_queue_depth: usize,
    max_retained_units: usize,
    max_editable_units: usize,
    max_chart_samples: usize,
    completed_runs: u64,
    interrupted_runs: u64,
    #[serde(skip)]
    pending: Vec<u64>,
}
impl Bench {
    fn new() -> Self {
        Self {
            schema_version: 1,
            debug_assertions: cfg!(debug_assertions),
            test_hooks_enabled: cfg!(feature = "test-hooks"),
            first_usable_frame_us: None,
            frame_flush_count: 0,
            input_received_count: 0,
            input_applied_count: 0,
            text_events_applied_count: 0,
            input_overload_count: 0,
            sequence_violation_count: 0,
            receive_to_flush_us: Vec::with_capacity(BENCH_CAPACITY),
            samples_total: 0,
            samples_dropped: 0,
            max_event_queue_depth: 0,
            max_retained_units: 0,
            max_editable_units: 0,
            max_chart_samples: 0,
            completed_runs: 0,
            interrupted_runs: 0,
            pending: Vec::with_capacity(4096),
        }
    }
    fn flushed(&mut self, now: u64, usable: bool) {
        self.frame_flush_count += 1;
        if usable {
            self.first_usable_frame_us.get_or_insert(now);
        }
        for received in self.pending.drain(..) {
            self.samples_total += 1;
            if self.receive_to_flush_us.len() < BENCH_CAPACITY {
                self.receive_to_flush_us.push(now.saturating_sub(received));
            } else {
                self.samples_dropped += 1;
            }
        }
    }
}
pub struct RunOutput {
    pub result: Option<ResultSnapshot>,
    pub exit_code: i32,
    pub benchmark: Option<Bench>,
    pub unsaved_count: usize,
}
use crate::ui::palette::{EditPurpose, Editor, EntryKind, Palette};

struct App {
    sample: Sample,
    samples: Samples,
    config: Config,
    _paths: Paths,
    epoch: u64,
    accepted: bool,
    armed: bool,
    safe_size: bool,
    finalized: bool,
    completed_at: u64,
    latest: Option<ResultSnapshot>,
    palette: Option<Palette>,
    details: bool,
    dirty: bool,
    notice: Option<String>,
    exit: Option<i32>,
    benchmark: Option<Bench>,
    last_sequence: Option<u64>,
    practice_return: Option<Config>,
    pending_result: Option<PendingResult>,
    review: Option<ui::review::Review>,
    bindings: Arc<crate::settings::Bindings>,
    panel: Option<ui::panel::Panel>,
    private_locked: bool,
    deferred: Option<Deferred>,
    history: Option<ui::history::History>,
    storage: StorageFlow,
}
struct PendingResult {
    epoch: u64,
    result: ResultSnapshot,
    config: Config,
    full_text: Option<String>,
    trace: Option<crate::storage::EventTrace>,
}
impl App {
    fn options(&self) -> ReaderOptions {
        ReaderOptions {
            mode: self.sample.engine.spec().mode,
            policy: self.sample.engine.spec().policy,
            seconds: self.sample.engine.spec().seconds,
            bindings: Arc::clone(&self.bindings),
        }
    }
    fn barrier(&mut self, reader: &Reader, armed: bool) -> Result<(), String> {
        self.epoch = self.epoch.checked_add(1).ok_or("test epoch exhausted")?;
        self.accepted = false;
        self.armed = armed;
        reader
            .command(if armed {
                Control::Restart {
                    epoch: self.epoch,
                    options: self.options(),
                }
            } else {
                Control::Disarm { epoch: self.epoch }
            })
            .map_err(|error| error.to_string())
    }
    fn capture(&mut self, now: u64) {
        if self.finalized || self.sample.engine.state() != State::Results {
            return;
        }
        self.pending_result = Some(PendingResult {
            epoch: self.epoch,
            result: self.sample.engine.snapshot(),
            config: self.config.clone(),
            full_text: if self.config.privacy.store_custom_text
                && !self.private_locked
                && !matches!(
                    self.sample.engine.spec().mode,
                    crate::engine::Mode::Time | crate::engine::Mode::Words
                ) {
                Some(
                    if self.sample.engine.spec().mode == crate::engine::Mode::Zen {
                        self.sample
                            .engine
                            .zen_window()
                            .iter()
                            .map(|entered| entered.unit.as_str())
                            .collect()
                    } else {
                        self.sample.text.clone()
                    },
                )
            } else {
                None
            },
            trace: self.storage.trace.take(),
        });
        self.storage.visible_epoch = Some(self.epoch);
        self.finalized = true;
        self.completed_at = now;
        self.dirty = true;
        self.notice = None;
    }
    fn closed_epoch(&mut self, epoch: u64, overloaded: bool) {
        if self
            .pending_result
            .as_ref()
            .is_none_or(|pending| pending.epoch != epoch)
        {
            return;
        }
        let mut pending = self.pending_result.take().expect("matching pending result");
        if overloaded {
            pending.result.outcome = Outcome::Interrupted;
            pending.result.reason = Some("input queue overload".into());
            pending.result.integrity.input_overload = true;
            pending.result.personal_best_eligible = false;
        }
        if let Some(bench) = &mut self.benchmark {
            if matches!(
                pending.result.outcome,
                Outcome::Complete | Outcome::Failed | Outcome::Incomplete
            ) {
                bench.completed_runs += 1;
            } else {
                bench.interrupted_runs += 1;
            }
        }
        if self.storage.visible_epoch == Some(epoch) {
            self.notice = if overloaded {
                Some("interrupted · input queue overload".into())
            } else if pending.config.privacy.save_results {
                Some("unsaved".into())
            } else {
                None
            };
        }
        self.persist_finished(&pending);
        self.latest = Some(public_snapshot(pending.result, &pending.config));
        if self.storage.visible_epoch == Some(epoch)
            && self.config.workflow.result_details
            && self.palette.is_none()
            && self.history.is_none()
            && self.panel.is_none()
        {
            self.open_review();
        }
        self.dirty = true;
    }
    fn restart(&mut self, reader: &Reader, repeat: bool, now: u64) -> Result<(), String> {
        self.restore_preview();
        if self.sample.engine.state() == State::Running {
            self.sample.engine.apply(Action::Abort, now);
            self.capture(now);
        }
        if !repeat && let Some(original) = self.practice_return.take() {
            // Practice temporarily changes only these fields. Later explicit
            // display, workflow, privacy and storage edits remain in effect.
            let mut restored = self.config.clone();
            restored.test = original.test;
            restored.rules = original.rules;
            restored.practice.pace = original.practice.pace;
            restored.practice.auto_indent = original.practice.auto_indent;
            self.samples.reconfigure(&restored, &self._paths)?;
            self.config = restored;
        }
        let next = if repeat {
            self.samples.repeat(&self.sample)?
        } else {
            self.samples.next(&self.config)?
        };
        self.sample = next;
        self.finalized = false;
        self.palette = None;
        self.details = false;
        self.panel = None;
        self.history = None;
        self.reset_run_storage();
        self.notice = None;
        self.dirty = true;
        self.barrier(reader, self.safe_size)
    }
    fn open_palette(&mut self, reader: &Reader, now: u64) -> Result<(), String> {
        let return_ready = self.sample.engine.state() != State::Results;
        if self.sample.engine.state() == State::Running {
            self.sample.engine.apply(Action::Abort, now);
            self.capture(now);
        }
        self.palette = Some(Palette::new(&self.config, return_ready));
        self.details = false;
        self.panel = None;
        self.dirty = true;
        self.barrier(reader, false)
    }
    fn close_palette(&mut self, reader: &Reader, now: u64) -> Result<(), String> {
        self.restore_preview();
        let ready = self
            .palette
            .take()
            .is_some_and(|palette| palette.return_ready);
        if self.history.is_some() {
            self.dirty = true;
            return Ok(());
        }
        if ready && self.sample.engine.state() == State::Results {
            self.restart(reader, false, now)?;
        } else {
            self.barrier(
                reader,
                self.sample.engine.state() == State::Ready && self.safe_size,
            )?;
        }
        self.dirty = true;
        Ok(())
    }
    fn quit(&mut self, now: u64) {
        if self.sample.engine.state() == State::Running {
            self.sample
                .engine
                .apply(Action::Interrupt("quit requested"), now);
            self.capture(now);
        }
        self.exit = Some(130);
        self.dirty = true;
    }
    fn open_review(&mut self) {
        if self.sample.engine.state() != State::Results {
            self.report("Finish a test to open its review");
            return;
        }
        let mut result = self.sample.engine.snapshot();
        if self.pending_result.is_none()
            && let Some(latest) = &self.latest
        {
            result.outcome = latest.outcome;
            result.reason = latest.reason.clone();
            result.integrity = latest.integrity.clone();
            result.personal_best_eligible = latest.personal_best_eligible;
        }
        self.review = Some(ui::review::Review::new(result));
        self.storage.stored_review = None;
        self.restore_preview();
        self.palette = None;
        self.panel = None;
        self.details = true;
        self.dirty = true;
    }
    fn dispatch(
        &mut self,
        action: crate::settings::CommandAction,
        reader: &Reader,
        now: u64,
    ) -> Result<(), String> {
        use crate::settings::CommandAction::*;
        match action {
            Palette => {
                if self.palette.is_some() {
                    self.palette_cancel(reader, now)?;
                } else if self.panel.is_some() {
                    self.close_panel(reader, now)?;
                } else if self.details {
                    self.details = false;
                    self.dirty = true;
                } else if self.history.is_some() {
                    self.close_history(reader, now)?;
                } else {
                    self.open_palette(reader, now)?;
                }
            }
            NewSample | NextSample => self.restart(reader, false, now)?,
            RepeatSample => self.restart(reader, true, now)?,
            Finish => {
                self.apply_visible(Action::Finish, now);
            }
            DeleteWord => {
                self.apply_visible(Action::DeleteWord, now);
            }
            Quit => self.quit(now),
            Details => self.open_review(),
            Practice => {
                self.open_palette(reader, now)?;
                if let Some(palette) = &mut self.palette {
                    palette.query = "practice".into();
                    let desired = if self.config.practice.selection == "slow" {
                        PracticeSlow
                    } else {
                        PracticeMissed
                    };
                    palette.selected=palette.matches().iter().position(|index|matches!(palette.entries[*index].kind,EntryKind::Action(action) if action==desired)).unwrap_or(0);
                }
            }
            PracticeMissed => self.start_practice(reader, crate::content::PracticeKind::Missed)?,
            PracticeSlow => self.start_practice(reader, crate::content::PracticeKind::Slow)?,
            SaveDefault => {
                if self.palette.is_none() {
                    self.open_palette(reader, now)?;
                }
                self.deferred = Some(Deferred::SaveDefaults);
                self.dirty = true;
            }
            Help => self.open_panel(reader, now, false)?,
            Config => self.open_panel(reader, now, true)?,
            Export => {
                if self.palette.is_none() {
                    self.open_palette(reader, now)?;
                }
                self.palette.as_mut().expect("palette").editor = Some(Editor {
                    purpose: EditPurpose::Export,
                    value: self
                        ._paths
                        .data
                        .join("recovery.jsonl")
                        .to_string_lossy()
                        .into_owned(),
                    choices: Vec::new(),
                    replace_on_type: true,
                });
                self.dirty = true;
            }
            History => self.open_history(reader, now)?,
            RetrySave => {
                self.deferred = Some(Deferred::RetrySave);
                self.dirty = true;
            }
        }
        Ok(())
    }
    fn start_practice(
        &mut self,
        reader: &Reader,
        kind: crate::content::PracticeKind,
    ) -> Result<(), String> {
        let mut config = self.config.clone();
        config.test.mode = crate::engine::Mode::Words;
        config.test.words = 25;
        config.test.policy = Policy::Prose;
        config.test.normalize_exact = false;
        config.test.completion = crate::engine::Completion::Confirm;
        config.test.punctuation = false;
        config.test.numbers = false;
        config.rules = crate::engine::Rules::default();
        config.practice.pace = "off".into();
        config.practice.auto_indent = false;
        match self.samples.practice(&config, &self.sample, kind) {
            Ok(sample) => {
                if self.practice_return.is_none() {
                    self.practice_return = Some(self.config.clone());
                }
                self.config = config;
                self.sample = sample;
                self.finalized = false;
                self.reset_run_storage();
                self.palette = None;
                self.details = false;
                self.panel = None;
                self.history = None;
                self.notice =
                    Some("25-word practice · original settings return on the next sample".into());
                self.dirty = true;
                self.barrier(reader, self.safe_size)?;
            }
            Err(reason) => {
                self.report(reason);
            }
        }
        Ok(())
    }
    fn key(
        &mut self,
        key: KeyEvent,
        text: Option<String>,
        reader: &Reader,
        received: u64,
    ) -> Result<(), String> {
        if !self.accepted {
            return Ok(());
        }
        let context = if self.palette.is_some()
            || self.details
            || self.panel.is_some()
            || self.history.is_some()
        {
            crate::settings::BindingContext::Overlay
        } else {
            match self.sample.engine.state() {
                State::Ready => crate::settings::BindingContext::Ready,
                State::Running => crate::settings::BindingContext::Running,
                State::Results => crate::settings::BindingContext::Results,
            }
        };
        if let Some(action) = self.bindings.resolve(&key, context) {
            if action == crate::settings::CommandAction::NextSample
                && key.code == KeyCode::Enter
                && key.modifiers.is_empty()
                && received.saturating_sub(self.completed_at) < 150_000
            {
                return Ok(());
            }
            return self.dispatch(action, reader, received);
        }
        if let Some(text) = text {
            if let Some(palette) = &mut self.palette {
                self.dirty |= palette.insert(&text);
            } else if self.history.is_some() && !self.details && text == "f" {
                if let Err(error) = self.history_key(KeyCode::Char('f')) {
                    self.report(error);
                }
            } else if self.details && text == "o" && self.storage.stored_review.is_some() {
                self.open_original_editor();
            } else if !self.details {
                self.type_text(&text, received);
            }
            self.preview_theme();
            return Ok(());
        }
        if key.modifiers.intersects(
            KeyModifiers::CONTROL
                | KeyModifiers::ALT
                | KeyModifiers::SUPER
                | KeyModifiers::META
                | KeyModifiers::HYPER,
        ) {
            return Ok(());
        }
        if let Some(palette) = &mut self.palette {
            match key.code {
                KeyCode::Esc => self.close_palette(reader, received)?,
                KeyCode::Char(character) => {
                    let mut bytes = [0; 4];
                    self.dirty |= palette.insert(character.encode_utf8(&mut bytes));
                }
                KeyCode::Backspace => {
                    self.dirty |= palette.backspace();
                }
                KeyCode::Up => {
                    self.dirty |= palette.move_by(-1);
                }
                KeyCode::Down => {
                    self.dirty |= palette.move_by(1);
                }
                KeyCode::Enter => {
                    self.palette_select(reader, received)?;
                }
                _ => {}
            }
            self.preview_theme();
            return Ok(());
        }
        if let Some(panel) = &mut self.panel {
            let previous = panel.scroll;
            match key.code {
                KeyCode::Down => panel.move_by(1),
                KeyCode::Up => panel.move_by(-1),
                KeyCode::PageDown => panel.move_by(10),
                KeyCode::PageUp => panel.move_by(-10),
                KeyCode::Home => panel.scroll = 0,
                _ => {}
            }
            self.dirty |= previous != panel.scroll;
            return Ok(());
        }
        if self.details {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::F(4)) {
                self.details = false;
                self.dirty = true;
            } else if let Some(review) = &mut self.review {
                let previous = (review.tab, review.scroll, review.horizontal);
                match key.code {
                    KeyCode::Tab => review.toggle(),
                    KeyCode::Down => review.move_by(1),
                    KeyCode::Up => review.move_by(-1),
                    KeyCode::PageDown => review.move_by(6),
                    KeyCode::PageUp => review.move_by(-6),
                    KeyCode::Home => {
                        review.scroll = 0;
                        review.horizontal = 0;
                    }
                    KeyCode::Left => review.pan(-1),
                    KeyCode::Right => review.pan(1),
                    KeyCode::End => review.show_tail(),
                    KeyCode::Char('o') if self.storage.stored_review.is_some() => {
                        self.open_original_editor();
                        return Ok(());
                    }
                    _ => {}
                }
                self.dirty |= previous != (review.tab, review.scroll, review.horizontal);
            }
            return Ok(());
        }
        if self.history.is_some() {
            if let Err(error) = self.history_key(key.code) {
                self.report(error);
            }
            return Ok(());
        }
        if key.code == KeyCode::Esc {
            return self.open_palette(reader, received);
        }
        if key.code == KeyCode::F(2) {
            return self.restart(reader, true, received);
        }
        if self.sample.engine.state() == State::Results {
            match key.code {
                KeyCode::Enter if received.saturating_sub(self.completed_at) >= 150_000 => {
                    self.restart(reader, false, received)?
                }
                KeyCode::F(4) => {
                    self.open_review();
                }
                KeyCode::F(3) => {
                    self.open_palette(reader, received)?;
                    if let Some(palette) = &mut self.palette {
                        palette.query = "practice".into();
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Char(character) => {
                let mut buffer = [0; 4];
                self.type_text(character.encode_utf8(&mut buffer), received);
            }
            KeyCode::Backspace => {
                self.apply_visible(Action::Backspace, received);
            }
            KeyCode::Tab if self.sample.engine.spec().policy == Policy::Exact => {
                self.type_text("\t", received)
            }
            KeyCode::Enter
                if self.sample.engine.spec().policy == Policy::Exact
                    || self.sample.engine.spec().mode == crate::engine::Mode::Zen =>
            {
                self.type_text("\n", received)
            }
            KeyCode::F(5) => {
                self.apply_visible(Action::Finish, received);
            }
            _ => {}
        }
        Ok(())
    }
    fn type_text(&mut self, text: &str, received: u64) {
        if !self.safe_size
            || !self.armed
            || self.palette.is_some()
            || self.panel.is_some()
            || self.history.is_some()
            || self.details
            || !matches!(self.sample.engine.state(), State::Ready | State::Running)
        {
            return;
        }
        let changed = self.apply_visible(Action::Text(text), received);
        if let Some(bench) = &mut self.benchmark {
            bench.text_events_applied_count += 1;
            bench.input_applied_count += 1;
            if changed && bench.pending.len() < 4096 {
                bench.pending.push(received);
            }
        }
    }
    fn apply_visible(&mut self, action: Action<'_>, received: u64) -> bool {
        let before = self.presentation_at(received, ui::live::Values::default());
        self.sample.engine.apply(action, received);
        let changed =
            !before.same_text(&self.presentation_at(received, ui::live::Values::default()));
        self.dirty |= changed;
        changed
    }
    fn presentation_at(&self, now: u64, live: ui::live::Values) -> Presentation {
        let elapsed = self
            .sample
            .engine
            .started_at()
            .map_or(0, |start| now.saturating_sub(start));
        Presentation::capture_live(&self.sample.engine, &self.config, elapsed, live)
    }
    fn rendered_presentation(&self, now: u64, elapsed: u64, viewport: &Viewport) -> Presentation {
        let mut live = viewport.live.preview(&self.sample.engine, elapsed);
        live.pace = live
            .pace
            .filter(|units| viewport.visible_pace(&self.sample.engine, *units));
        self.presentation_at(now, live)
    }
}
fn public_snapshot(mut result: ResultSnapshot, config: &Config) -> ResultSnapshot {
    if !result.spec.approved_content && !config.privacy.store_custom_text {
        result.words.clear();
    } else if !config.privacy.store_event_trace {
        for word in &mut result.words {
            word.entered.clear();
        }
    }
    result
}

pub fn run(
    cli: &Cli,
    mut config: Config,
    paths: Paths,
    mut samples: Samples,
    notice: Option<String>,
) -> Result<RunOutput, String> {
    if cli.private || config.privacy.private_session {
        config.privacy.private_session = true;
        config.privacy.save_results = false;
        config.privacy.store_custom_text = false;
        config.privacy.store_event_trace = false;
    }
    let sample = samples.next(&config)?;
    let private_locked = cli.private || config.privacy.private_session;
    let bindings = Arc::new(crate::settings::Bindings::from_config(&config)?);
    let origin = Instant::now();
    let caret = match config.appearance.caret {
        Caret::Bar => CaretStyle::Bar,
        Caret::Block => CaretStyle::Block,
        Caret::Underline => CaretStyle::Underline,
    };
    let mut session = Session::enter(SessionOptions {
        enhanced_keyboard: config.enhanced_keyboard,
        focus_reporting: true,
        caret,
    })
    .map_err(|error| error.to_string())?;
    // Crossterm has its own NO_COLOR cache. Our theme policy already resolves
    // NO_COLOR and the explicit override; avoid a second conflicting policy.
    crossterm::style::force_color_output(true);
    let writer = BufWriter::with_capacity(
        64 * 1024,
        session.writer().map_err(|error| error.to_string())?,
    );
    let mut terminal =
        Terminal::new(CrosstermBackend::new(writer)).map_err(|error| error.to_string())?;
    let size = crate::terminal::terminal_size().map_err(|error| error.to_string())?;
    let mut reader = Reader::start(
        ReaderOptions {
            mode: sample.engine.spec().mode,
            policy: sample.engine.spec().policy,
            seconds: sample.engine.spec().seconds,
            bindings: Arc::clone(&bindings),
        },
        1,
        origin,
        thread::current(),
    )
    .map_err(|error| error.to_string())?;
    let mut app = App {
        sample,
        samples,
        config,
        _paths: paths,
        epoch: 1,
        accepted: true,
        armed: true,
        safe_size: size.0 >= 40 && size.1 >= 10,
        finalized: false,
        completed_at: 0,
        latest: None,
        palette: None,
        details: false,
        dirty: true,
        notice,
        exit: None,
        benchmark: cli.benchmark_output.as_ref().map(|_| Bench::new()),
        last_sequence: None,
        practice_return: None,
        pending_result: None,
        review: None,
        bindings,
        panel: None,
        private_locked,
        deferred: None,
        history: None,
        storage: StorageFlow::default(),
    };
    if !app.safe_size {
        app.barrier(&reader, false)?;
    }
    let mut viewport = Viewport::default();
    let mut viewport_epoch = app.epoch;
    let mut last_draw: Option<Instant> = None;
    let mut last_visual_check: Option<Instant> = None;
    let mut last_presentation = None;
    let mut observer = Observer::new();
    let runtime = (|| -> Result<(), String> {
        loop {
            let now = origin.elapsed().as_micros() as u64;
            if let Some(bench) = &mut app.benchmark {
                bench.max_event_queue_depth = bench.max_event_queue_depth.max(reader.events.len());
            }
            if reader.overload_epoch() == Some(app.epoch) {
                if let Some(bench) = &mut app.benchmark {
                    bench.input_overload_count += 1;
                }
                app.sample.engine.apply(Action::Overload, now);
                app.capture(now);
                app.dirty = true;
                app.barrier(&reader, false)?;
            }
            let slice = Instant::now();
            let mut drained = 0;
            while last_draw.is_some() && drained < 64 && slice.elapsed() < Duration::from_millis(1)
            {
                let event = match reader.events.try_recv() {
                    Ok(event) => event,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        return Err("terminal input reader disconnected".into());
                    }
                };
                drained += 1;
                if let Some(bench) = &mut app.benchmark {
                    bench.input_received_count += 1;
                }
                if let Some(previous) = app.last_sequence
                    && event.sequence <= previous
                {
                    if let Some(bench) = &mut app.benchmark {
                        bench.sequence_violation_count += 1;
                    }
                    app.sample.engine.apply(
                        Action::Interrupt("input event sequence violation"),
                        event.received_us,
                    );
                    app.capture(now);
                    app.barrier(&reader, false)?;
                    continue;
                }
                app.last_sequence = Some(event.sequence);
                if let InputKind::Signal(signal) = event.kind {
                    #[cfg(not(unix))]
                    let _ = signal;
                    #[cfg(unix)]
                    if signal == signal_hook::consts::SIGTSTP {
                        app.sample
                            .engine
                            .apply(Action::Interrupt("suspended session"), event.received_us);
                        app.capture(now);
                        reader.shutdown().map_err(|error| error.to_string())?;
                        if let Some(closure) = reader.last_closed() {
                            app.closed_epoch(closure.closed_epoch, closure.overloaded);
                        }
                        session.suspend().map_err(|error| error.to_string())?;
                        reader =
                            Reader::start(app.options(), app.epoch + 1, origin, thread::current())
                                .map_err(|error| error.to_string())?;
                        app.epoch += 1;
                        app.last_sequence = None;
                        app.barrier(&reader, false)?;
                        app.dirty = true;
                        // Ratatui clear() queries the cursor through global stdin/stdout.
                        // Resizing the fullscreen viewport resets its buffers without a query.
                        let (width, height) =
                            crate::terminal::terminal_size().map_err(|error| error.to_string())?;
                        terminal
                            .resize(ratatui::layout::Rect::new(0, 0, width, height))
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    #[cfg(unix)]
                    if signal == signal_hook::consts::SIGCONT {
                        continue;
                    }
                    app.sample
                        .engine
                        .apply(Action::Interrupt("termination signal"), event.received_us);
                    app.capture(now);
                    if app.pending_result.is_some() {
                        app.barrier(&reader, false)?;
                    }
                    app.exit = Some(130);
                    app.dirty = true;
                    break;
                }
                if let InputKind::EpochClosed {
                    closed_epoch,
                    overloaded,
                } = event.kind
                {
                    app.closed_epoch(closed_epoch, overloaded);
                    continue;
                }
                if event.epoch != app.epoch {
                    continue;
                }
                app.trace_event(&event);
                match event.kind {
                    InputKind::EpochReady => app.accepted = true,
                    InputKind::EpochClosed { .. } => unreachable!(),
                    InputKind::Key {
                        key,
                        associated_text,
                    } => app.key(key, associated_text, &reader, event.received_us)?,
                    InputKind::Paste => {
                        app.sample.engine.apply(Action::Paste, event.received_us);
                    }
                    InputKind::FocusLost => app
                        .sample
                        .engine
                        .apply(Action::FocusLost, event.received_us),
                    InputKind::Tick => {
                        app.apply_visible(Action::Tick, event.received_us);
                    }
                    InputKind::Resize(width, height) => {
                        app.safe_size = width >= 40 && height >= 10;
                        if !app.safe_size {
                            app.sample.engine.apply(
                                Action::Interrupt("terminal resized below 40 × 10"),
                                event.received_us,
                            );
                            app.capture(now);
                            app.barrier(&reader, false)?;
                        } else if app.sample.engine.state() == State::Ready
                            && app.palette.is_none()
                            && app.panel.is_none()
                            && app.history.is_none()
                            && !app.details
                            && !app.armed
                        {
                            app.barrier(&reader, true)?;
                        }
                        app.dirty = true;
                    }
                    InputKind::Error(reason) => {
                        app.sample
                            .engine
                            .apply(Action::Interrupt(&reason), event.received_us);
                        app.capture(now);
                        app.report(reason);
                        app.barrier(&reader, false)?;
                        app.dirty = true;
                        if app.sample.engine.state() == State::Ready {
                            app.exit = Some(1);
                        }
                    }
                    InputKind::Signal(_) => unreachable!(),
                }
                if app.sample.engine.state() == State::Results && !app.finalized {
                    app.capture(origin.elapsed().as_micros() as u64);
                }
                if app.sample.engine.state() == State::Results && app.armed {
                    #[cfg(feature = "test-hooks")]
                    observer.completion_pending(&app, &reader)?;
                    app.barrier(&reader, false)?;
                }
                if app.exit.is_some() {
                    break;
                }
            }
            app.apply_deferred(&reader, &mut session, origin.elapsed().as_micros() as u64)?;
            app.service_storage();
            if app.sample.engine.state() == State::Running {
                // Preparation is scheduled after the bounded reducer batch.
                app.sample.lookahead(1200)?;
            }
            if app.finalized
                && app.pending_result.is_none()
                && cli.once
                && app.palette.is_none()
                && app.sample.engine.outcome() != Outcome::Aborted
            {
                app.exit = Some(
                    if app
                        .latest
                        .as_ref()
                        .is_some_and(|result| result.outcome == Outcome::Interrupted)
                    {
                        130
                    } else {
                        0
                    },
                );
            }
            let now = origin.elapsed().as_micros() as u64;
            let elapsed = app
                .sample
                .engine
                .started_at()
                .map_or(0, |start| now.saturating_sub(start));
            let interval = visible_interval(&app);
            if let (Some(interval), Some(last)) = (interval, last_visual_check)
                && last.elapsed() >= interval
            {
                app.dirty |= last_presentation.as_ref()
                    != Some(&app.rendered_presentation(now, elapsed, &viewport));
                last_visual_check = Some(Instant::now());
            }
            if app.dirty && last_draw.is_none_or(|last| last.elapsed() >= FRAME_INTERVAL) {
                if viewport_epoch != app.epoch {
                    viewport = Viewport::default();
                    viewport_epoch = app.epoch;
                }
                let mut cursor = None;
                let notice = app.visible_notice();
                terminal
                    .draw(|frame| {
                        if app.palette.is_some() {
                            draw_palette(frame, &app);
                        } else if app.details {
                            draw_details(frame, &app);
                        } else if let Some(panel) = &app.panel {
                            ui::panel::render(frame, panel, &app.config.appearance);
                        } else if let Some(history) = &app.history {
                            ui::history::render(frame, history, &app.config.appearance);
                        } else if app.sample.engine.state() == State::Results
                            && app.pending_result.is_none()
                            && app.storage.visible_epoch.is_some()
                            && let Some(result) = &app.latest
                        {
                            ui::render_snapshot(
                                frame,
                                result,
                                &app.config.appearance,
                                notice.as_deref(),
                            );
                        } else {
                            cursor = ui::render(
                                frame,
                                &app.sample.engine,
                                &app.config.appearance,
                                &app.config.status,
                                &mut viewport,
                                elapsed,
                                notice.as_deref(),
                            );
                        }
                    })
                    .map_err(|error| format!("terminal output interruption: {error}"))?;
                let flushed = origin.elapsed().as_micros() as u64;
                if let Some(bench) = &mut app.benchmark {
                    bench.flushed(
                        flushed,
                        app.safe_size
                            && app.palette.is_none()
                            && !app.details
                            && matches!(app.sample.engine.state(), State::Ready | State::Running),
                    );
                    let retained = if app.sample.engine.spec().mode == crate::engine::Mode::Zen {
                        app.sample.engine.zen_window().len()
                    } else {
                        app.sample.engine.counts().retained_units as usize
                    };
                    bench.max_retained_units = bench.max_retained_units.max(retained);
                    bench.max_editable_units = bench.max_editable_units.max(retained);
                    bench.max_chart_samples = bench
                        .max_chart_samples
                        .max(app.sample.engine.samples().len());
                }
                observer.frame(&app, cursor, flushed)?;
                last_presentation = Some(app.rendered_presentation(now, elapsed, &viewport));
                last_draw = Some(Instant::now());
                last_visual_check = last_draw;
                app.dirty = false;
                if app.safe_size && !app.storage.first_frame {
                    app.storage.first_frame = true;
                    app.service_storage();
                }
                #[cfg(feature = "test-hooks")]
                if observer.frames == 1 {
                    match std::env::var("CLACK_TEST_FAULT").as_deref() {
                        Ok("panic_after_first_frame") => {
                            panic!("injected terminal lifecycle panic")
                        }
                        Ok("error_after_first_frame") => {
                            return Err("injected terminal lifecycle error".into());
                        }
                        _ => {}
                    }
                }
                if app.exit.is_some() && app.pending_result.is_none() {
                    break;
                }
            }
            let wake = if app.dirty {
                last_draw.map_or(Duration::ZERO, |last| {
                    FRAME_INTERVAL.saturating_sub(last.elapsed())
                })
            } else if let Some(interval) = interval {
                last_visual_check.map_or(Duration::ZERO, |last| {
                    interval.saturating_sub(last.elapsed())
                })
            } else {
                Duration::from_secs(86_400)
            };
            // Unpark tokens are retained: checking both queues then parking cannot lose a wake.
            if drained >= 64 || slice.elapsed() >= Duration::from_millis(1) {
                thread::yield_now();
            } else {
                thread::park_timeout(wake);
            }
        }
        Ok(())
    })();
    if runtime.is_err() && app.sample.engine.state() == State::Running {
        let now = origin.elapsed().as_micros() as u64;
        app.sample
            .engine
            .apply(Action::Interrupt("runtime input/output failure"), now);
        app.capture(now);
    }
    let reader_cleanup = reader.shutdown().map_err(|error| error.to_string());
    if let Some(closure) = reader.last_closed() {
        app.closed_epoch(closure.closed_epoch, closure.overloaded);
    }
    drop(reader);
    let unsaved_count = app.flush_storage(app.exit == Some(130) || runtime.is_err());
    drop(terminal);
    let terminal_cleanup = session.restore().map_err(|error| error.to_string());
    if let Err(error) = runtime.and(reader_cleanup).and(terminal_cleanup) {
        return Err(if unsaved_count > 0 {
            format!("{error}; {unsaved_count} result(s) remain unsaved")
        } else {
            error
        });
    }
    Ok(RunOutput {
        result: app.latest,
        exit_code: app.exit.unwrap_or(0),
        benchmark: app.benchmark,
        unsaved_count,
    })
}
fn visible_interval(app: &App) -> Option<Duration> {
    if app.sample.engine.state() != State::Running
        || app.palette.is_some()
        || app.details
        || app.panel.is_some()
        || app.history.is_some()
    {
        return None;
    }
    if app.sample.engine.spec().pace_wpm.is_some() {
        Some(Duration::from_millis(100))
    } else if app.config.appearance.focus == Focus::Always {
        None
    } else if app.config.status.wpm
        || app.config.status.accuracy && app.sample.engine.spec().mode != crate::engine::Mode::Zen
    {
        Some(Duration::from_millis(250))
    } else if app.config.status.progress
        && app.sample.engine.spec().mode != crate::engine::Mode::Words
    {
        Some(Duration::from_secs(1))
    } else {
        None
    }
}
fn draw_palette(frame: &mut ratatui::Frame, app: &App) {
    if let Some(palette) = &app.palette {
        ui::palette::render(frame, palette, &app.config.appearance);
    }
}

fn draw_details(frame: &mut ratatui::Frame, app: &App) {
    if let Some(review) = &app.review {
        ui::review::render(frame, review, &app.config.appearance);
    }
}

struct Observer {
    #[cfg(feature = "test-hooks")]
    file: Option<File>,
    #[cfg(feature = "test-hooks")]
    frames: u64,
}
impl Observer {
    #[cfg(feature = "test-hooks")]
    fn completion_pending(&mut self, app: &App, reader: &Reader) -> Result<(), String> {
        if std::env::var("CLACK_TEST_FAULT").as_deref() != Ok("wait_overload_after_completion")
            || app.sample.engine.outcome() != Outcome::Complete
        {
            return Ok(());
        }
        if let Some(file) = &mut self.file {
            serde_json::to_writer(
                &mut *file,
                &serde_json::json!({"event":"completion_pending","epoch":app.epoch}),
            )
            .map_err(|error| error.to_string())?;
            writeln!(file).map_err(|error| error.to_string())?;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while reader.overload_epoch() != Some(app.epoch) {
            if Instant::now() >= deadline {
                return Err("overload race test did not supply a queue-filling burst".into());
            }
            thread::park_timeout(Duration::from_millis(10));
        }
        Ok(())
    }
    fn new() -> Self {
        Self {
            #[cfg(feature = "test-hooks")]
            file: std::env::var("CLACK_TEST_OBSERVER_FD")
                .ok()
                .and_then(|fd| fd.parse::<u32>().ok())
                .and_then(|fd| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(format!("/dev/fd/{fd}"))
                        .ok()
                }),
            #[cfg(feature = "test-hooks")]
            frames: 0,
        }
    }
    fn frame(&mut self, _app: &App, _cursor: Option<(u16, u16)>, _at: u64) -> Result<(), String> {
        #[cfg(feature = "test-hooks")]
        {
            self.frames += 1;
            if let Some(file) = &mut self.file {
                let value = serde_json::json!({"event":"frame","state":format!("{:?}",_app.sample.engine.state()).to_lowercase(),"epoch":_app.epoch,"cursor":_cursor,"counts":_app.sample.engine.counts(),"elapsed_us":_app.sample.engine.elapsed_us(),"at_us":_at});
                serde_json::to_writer(&mut *file, &value).map_err(|error| error.to_string())?;
                writeln!(file).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
}
