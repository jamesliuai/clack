//! A staged, keyboard-first test setup. Nothing is persisted until Apply.
use super::{Appearance, ColorDepth, Theme, line};
use crate::{config::Config, engine::Mode, settings::Edit};
use crossterm::event::KeyCode;
use ratatui::{Frame, style::Modifier};

const MODES: [Mode; 6] = [
    Mode::Time,
    Mode::Words,
    Mode::Quote,
    Mode::Custom,
    Mode::Code,
    Mode::Zen,
];
const MODE_NAMES: [&str; 6] = ["time", "words", "quote", "custom", "code", "zen"];
const TIMES: [&str; 4] = ["15", "30", "60", "120"];
const WORDS: [&str; 4] = ["10", "25", "50", "100"];
const QUOTES: [&str; 4] = ["short", "medium", "long", "extended"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Mode,
    Length,
    File,
    Punctuation,
    Numbers,
}

pub struct Setup {
    original: Config,
    pub mode: Mode,
    seconds: String,
    words: String,
    quote: String,
    file: String,
    punctuation: bool,
    numbers: bool,
    selected: usize,
    replace_on_type: bool,
}

impl Setup {
    pub fn new(config: &Config) -> Self {
        Self {
            original: config.clone(),
            mode: config.test.mode,
            seconds: config.test.seconds.to_string(),
            words: config.test.words.to_string(),
            quote: config.test.quote_length.clone(),
            file: config
                .test
                .file
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            punctuation: config.test.punctuation,
            numbers: config.test.numbers,
            selected: usize::from(matches!(
                config.test.mode,
                Mode::Time | Mode::Words | Mode::Quote
            )),
            replace_on_type: true,
        }
    }
    fn rows(&self) -> Vec<Row> {
        match self.mode {
            Mode::Time | Mode::Words => {
                vec![Row::Mode, Row::Length, Row::Punctuation, Row::Numbers]
            }
            Mode::Quote => vec![Row::Mode, Row::Length],
            Mode::Custom | Mode::Code => vec![Row::Mode, Row::File],
            Mode::Zen => vec![Row::Mode],
        }
    }
    fn row(&self) -> Row {
        self.rows()[self.selected]
    }
    fn length(&self) -> &str {
        match self.mode {
            Mode::Time => &self.seconds,
            Mode::Words => &self.words,
            _ => &self.quote,
        }
    }
    fn choices(&self) -> &[&str] {
        match self.mode {
            Mode::Time => &TIMES,
            Mode::Words => &WORDS,
            _ => &QUOTES,
        }
    }
    fn input(&mut self) -> Option<&mut String> {
        match (self.row(), self.mode) {
            (Row::Length, Mode::Time) => Some(&mut self.seconds),
            (Row::Length, Mode::Words) => Some(&mut self.words),
            (Row::File, _) => Some(&mut self.file),
            _ => None,
        }
    }
    pub fn insert(&mut self, text: &str) {
        if !crate::settings::safe_text(text) {
            return;
        }
        // Enhanced keyboards deliver printable Space as associated text.
        if text == " " && self.row() != Row::File {
            self.key(KeyCode::Char(' '));
            return;
        }
        // Source paths own literal characters, including the mode shortcuts.
        if self.row() != Row::File {
            let mode = match text {
                "t" => Some(Mode::Time),
                "w" => Some(Mode::Words),
                "q" => Some(Mode::Quote),
                "c" => Some(Mode::Custom),
                "d" => Some(Mode::Code),
                "z" => Some(Mode::Zen),
                _ => None,
            };
            if let Some(mode) = mode {
                self.mode = mode;
                self.selected = usize::from(mode != Mode::Zen);
                self.replace_on_type = true;
                return;
            }
            if !text.bytes().all(|byte| byte.is_ascii_digit()) {
                return;
            }
        }
        let replace = self.replace_on_type;
        let limit = if self.row() == Row::File { 4096 } else { 5 };
        if let Some(value) = self.input() {
            if (if replace { 0 } else { value.len() }) + text.len() > limit {
                return;
            }
            if replace {
                value.clear();
            }
            value.push_str(text);
            self.replace_on_type = false;
        }
    }
    pub fn key(&mut self, key: KeyCode) {
        if key == KeyCode::Char(' ') && self.row() == Row::File {
            self.insert(" ");
            return;
        }
        match key {
            KeyCode::Up | KeyCode::BackTab | KeyCode::Down | KeyCode::Tab => {
                let delta = if matches!(key, KeyCode::Up | KeyCode::BackTab) {
                    -1
                } else {
                    1
                };
                self.selected = (self.selected as isize + delta)
                    .rem_euclid(self.rows().len() as isize)
                    as usize;
                self.replace_on_type = true;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                let delta = if key == KeyCode::Left { -1 } else { 1 };
                match self.row() {
                    Row::Mode => {
                        let current = MODES.iter().position(|mode| *mode == self.mode).unwrap();
                        self.mode = MODES
                            [(current as isize + delta).rem_euclid(MODES.len() as isize) as usize];
                    }
                    Row::Length => {
                        let choices = self.choices();
                        let next = if let Some(current) =
                            choices.iter().position(|value| *value == self.length())
                        {
                            (current as isize + delta).rem_euclid(choices.len() as isize) as usize
                        } else if let Ok(current) = self.length().parse::<u32>() {
                            if delta > 0 {
                                choices
                                    .iter()
                                    .position(|v| v.parse::<u32>().unwrap() > current)
                                    .unwrap_or(0)
                            } else {
                                choices
                                    .iter()
                                    .rposition(|v| v.parse::<u32>().unwrap() < current)
                                    .unwrap_or(choices.len() - 1)
                            }
                        } else {
                            0
                        };
                        let value = choices[next].to_owned();
                        match self.mode {
                            Mode::Time => self.seconds = value,
                            Mode::Words => self.words = value,
                            _ => self.quote = value,
                        }
                    }
                    Row::Punctuation => self.punctuation = !self.punctuation,
                    Row::Numbers => self.numbers = !self.numbers,
                    Row::File => {
                        if key == KeyCode::Char(' ') {
                            self.insert(" ");
                        }
                    }
                }
                self.replace_on_type = true;
            }
            KeyCode::Backspace => {
                let replace = self.replace_on_type;
                if let Some(value) = self.input() {
                    if replace {
                        value.clear();
                    } else {
                        value.pop();
                    }
                    self.replace_on_type = false;
                }
            }
            KeyCode::Char(character) => self.insert(&character.to_string()),
            _ => {}
        }
    }
    pub fn edits(&self) -> Result<Vec<Edit>, String> {
        match self.mode {
            Mode::Time
                if !self
                    .seconds
                    .parse::<u32>()
                    .is_ok_and(|n| (1..=3600).contains(&n)) =>
            {
                return Err("Duration must be 1-3600 seconds.".into());
            }
            Mode::Words
                if !self
                    .words
                    .parse::<u32>()
                    .is_ok_and(|n| (1..=10000).contains(&n)) =>
            {
                return Err("Word count must be 1-10000.".into());
            }
            _ => {}
        }
        let mut edits = Vec::new();
        let mode_changed = self.mode != self.original.test.mode;
        if mode_changed {
            edits.extend(crate::settings::mode_edits(
                MODE_NAMES[MODES.iter().position(|mode| *mode == self.mode).unwrap()],
            )?);
        }
        let mut changed = |path: &str, value: &str, original: String| -> Result<(), String> {
            if value != original {
                edits.push(Edit::parse(path, value)?);
            }
            Ok(())
        };
        match self.mode {
            Mode::Time => changed(
                "test.seconds",
                &self.seconds,
                self.original.test.seconds.to_string(),
            )?,
            Mode::Words => changed(
                "test.words",
                &self.words,
                self.original.test.words.to_string(),
            )?,
            Mode::Quote => {
                changed(
                    "test.quote_length",
                    &self.quote,
                    self.original.test.quote_length.clone(),
                )?;
                // Choosing a category must release an explicitly pinned quote.
                if self.quote != self.original.test.quote_length
                    && self.original.test.quote_id.is_some()
                {
                    edits.push(Edit::parse("test.quote_id", "unset")?);
                }
            }
            Mode::Custom | Mode::Code => changed(
                "test.file",
                if self.file.is_empty() {
                    "unset"
                } else {
                    &self.file
                },
                self.original
                    .test
                    .file
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unset".into()),
            )?,
            Mode::Zen => {}
        }
        if matches!(self.mode, Mode::Time | Mode::Words) {
            if self.punctuation != self.original.test.punctuation {
                edits.push(Edit::parse(
                    "test.punctuation",
                    &self.punctuation.to_string(),
                )?);
            }
            if self.numbers != self.original.test.numbers {
                edits.push(Edit::parse("test.numbers", &self.numbers.to_string())?);
            }
        }
        self.original.edited(&edits)?;
        Ok(edits)
    }
    fn help(&self) -> &str {
        match self.row() {
            Row::Mode => "Choose a mode with left/right arrows.",
            Row::Length => match self.mode {
                Mode::Time => "Pick a preset or type 1-3600 seconds.",
                Mode::Words => "Choose a preset or type 1-10000 words.",
                _ => "Choose the length of your next quote.",
            },
            Row::File => "Type a file path; Backspace clears it.",
            Row::Punctuation => "Add punctuation to generated words.",
            Row::Numbers => "Include numbers in generated words.",
        }
    }
}

pub fn render(frame: &mut Frame, setup: &Setup, appearance: &Appearance, message: Option<&str>) {
    let theme = Theme::from_preferences(appearance, ColorDepth::detect(appearance.color));
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.background);
    if area.width < 40 || area.height < 10 {
        super::message(frame, "Resize to at least 40 × 10.", theme.muted);
        return;
    }
    let width = area.width.saturating_sub(2).min(86);
    let x = (area.width - width) / 2;
    let compact = area.height < 20;
    let top = if compact { 0 } else { 2 };
    line(
        frame,
        x,
        top,
        width,
        "Test setup",
        theme.correct.add_modifier(Modifier::BOLD),
    );
    line(
        frame,
        x,
        top + 1,
        width,
        if width < 60 {
            "t time  w words  q quote  z zen"
        } else {
            "t time   w words   q quote   c custom   d code   z zen"
        },
        theme.muted,
    );
    let step = if compact { 1 } else { 2 };
    for (index, row) in setup.rows().iter().enumerate() {
        let selected = index == setup.selected;
        let (label, choices, current): (&str, Vec<String>, String) = match row {
            Row::Mode => (
                "Mode",
                MODE_NAMES.iter().map(|s| (*s).into()).collect(),
                MODE_NAMES[MODES.iter().position(|mode| *mode == setup.mode).unwrap()].into(),
            ),
            Row::Length => {
                let suffix = if setup.mode == Mode::Time { "s" } else { "" };
                let mut choices: Vec<String> = setup
                    .choices()
                    .iter()
                    .map(|v| format!("{v}{suffix}"))
                    .collect();
                let current = format!("{}{suffix}", setup.length());
                if !choices.contains(&current) {
                    choices.push(current.clone());
                }
                (
                    if setup.mode == Mode::Time {
                        "Time"
                    } else if setup.mode == Mode::Words {
                        "Words"
                    } else {
                        "Length"
                    },
                    choices,
                    current,
                )
            }
            Row::File => (
                "File",
                vec![],
                if setup.file.is_empty() {
                    if setup.mode == Mode::Code {
                        "bundled / initial text".into()
                    } else {
                        "initial text / file path".into()
                    }
                } else {
                    setup.file.clone()
                },
            ),
            Row::Punctuation => (
                "Punct.",
                vec!["off".into(), "on".into()],
                if setup.punctuation { "on" } else { "off" }.into(),
            ),
            Row::Numbers => (
                "Numbers",
                vec!["off".into(), "on".into()],
                if setup.numbers { "on" } else { "off" }.into(),
            ),
        };
        let prefix = if *row == Row::Mode && width < 60 {
            format!("{} ", if selected { ">" } else { " " })
        } else {
            format!("{} {label:<7} ", if selected { ">" } else { " " })
        };
        let available = usize::from(width).saturating_sub(prefix.len());
        let content = if choices.is_empty() {
            let tail: String = current
                .chars()
                .rev()
                .take(available.saturating_sub(2))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("[{tail}]")
        } else {
            let marked: Vec<String> = choices
                .iter()
                .map(|v| {
                    if *v == current {
                        format!("[{v}]")
                    } else {
                        v.clone()
                    }
                })
                .collect();
            let active = choices.iter().position(|v| *v == current).unwrap_or(0);
            let mut start = 0;
            while start < active
                && marked[start..=active].join(" ").len()
                    + usize::from(start > 0) * 2
                    + usize::from(active + 1 < marked.len()) * 2
                    > available
            {
                start += 1;
            }
            let mut output = if start > 0 {
                "< ".to_owned()
            } else {
                String::new()
            };
            for (i, value) in marked.iter().enumerate().skip(start) {
                if output.len() + value.len() + usize::from(i + 1 < marked.len()) * 2 > available {
                    output.push('>');
                    break;
                }
                output.push_str(value);
                if i + 1 < marked.len() {
                    output.push(' ');
                }
            }
            output
        };
        line(
            frame,
            x,
            top + 2 + index as u16 * step,
            width,
            format!("{prefix}{content}"),
            if selected {
                theme.accent.add_modifier(Modifier::BOLD)
            } else {
                theme.muted
            },
        );
    }
    let explanation = match setup.mode {
        Mode::Time => "Type until the timer ends.",
        Mode::Words => "Finish the selected number of words.",
        Mode::Quote => "Type a complete quote.",
        Mode::Custom => "Use your own text. A source is required.",
        Mode::Code => "Exact text, tabs and newlines; F5 finishes.",
        Mode::Zen => "Type freely, without a target. F5 finishes.",
    };
    if !compact {
        line(frame, x, top + 11, width, explanation, theme.correct);
        line(
            frame,
            x,
            area.height - 5,
            width,
            "Enter saves your choices and prepares a fresh test.",
            theme.muted,
        );
    }
    line(frame, x, area.height - 4, width, setup.help(), theme.muted);
    line(
        frame,
        x,
        area.height - 3,
        width,
        message.unwrap_or("Changes apply together; Esc discards."),
        if message.is_some() {
            theme.incorrect
        } else {
            theme.muted
        },
    );
    line(
        frame,
        x,
        area.height - 2,
        width,
        "↑↓ / tab row   ←→ change",
        theme.muted,
    );
    line(
        frame,
        x,
        area.height - 1,
        width,
        "enter apply   esc cancel",
        theme.correct,
    );
}
