//! Shared typed setting metadata and command recognition for every adapter.
use crate::config::Config;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Boolean,
    Integer(i64, i64),
    Number(f64, f64),
    OptionalNumber(f64, f64),
    Choice(&'static [&'static str]),
    Text(usize),
    OptionalText(usize),
    Strings(usize),
    Width,
    Pace,
    Bindings,
    Presets,
}

#[derive(Debug, Clone, Copy)]
pub struct Setting {
    pub path: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: Kind,
    pub ordinary: bool,
    pub affects_test: bool,
}

macro_rules! setting {
    ($path:literal, $group:literal, $label:literal, $kind:expr, $ordinary:expr, $test:expr, $help:literal) => {
        Setting {
            path: $path,
            group: $group,
            label: $label,
            kind: $kind,
            ordinary: $ordinary,
            affects_test: $test,
            help: $help,
        }
    };
}
pub const REGISTRY: &[Setting] = &[
    setting!(
        "schema_version",
        "format",
        "Configuration version",
        Kind::Integer(1, 1),
        false,
        false,
        "Version of the typed configuration format."
    ),
    setting!(
        "test.mode",
        "test",
        "Mode",
        Kind::Choice(&["time", "words", "quote", "custom", "code", "zen"]),
        true,
        true,
        "Select a test; scoring changes start a new sample."
    ),
    setting!(
        "test.seconds",
        "test",
        "Duration",
        Kind::Integer(1, 3600),
        true,
        true,
        "Timed-test duration in whole seconds."
    ),
    setting!(
        "test.words",
        "test",
        "Word count",
        Kind::Integer(1, 10000),
        true,
        true,
        "Exact requested number of generated words."
    ),
    setting!(
        "test.language",
        "test",
        "Language pack",
        Kind::Text(64),
        true,
        true,
        "Stable installed pack ID; use languages list for available packs."
    ),
    setting!(
        "test.punctuation",
        "test",
        "Punctuation",
        Kind::Boolean,
        true,
        true,
        "Apply versioned punctuation to generated words only."
    ),
    setting!(
        "test.numbers",
        "test",
        "Numbers",
        Kind::Boolean,
        true,
        true,
        "Replace generated words using the configured number probability."
    ),
    setting!(
        "test.policy",
        "test",
        "Input policy",
        Kind::Choice(&["prose", "exact"]),
        true,
        true,
        "Prose compares words; exact preserves literal tabs and newlines."
    ),
    setting!(
        "test.normalize_exact",
        "test",
        "Normalize exact Unicode",
        Kind::Boolean,
        false,
        true,
        "Explicitly enable canonical normalization for exact text."
    ),
    setting!(
        "test.completion",
        "test",
        "Exact completion",
        Kind::Choice(&["confirm", "auto"]),
        true,
        true,
        "Confirm allows tail correction before F5; auto is a distinct profile."
    ),
    setting!(
        "test.quote_length",
        "test",
        "Quote length",
        Kind::Choice(&["short", "medium", "long", "extended"]),
        true,
        true,
        "Select the local quote word-count category."
    ),
    setting!(
        "test.quote_id",
        "test",
        "Quote ID",
        Kind::OptionalText(128),
        false,
        true,
        "Select a stable local quote ID, or unset for category selection."
    ),
    setting!(
        "test.file",
        "test",
        "Source file",
        Kind::OptionalText(4096),
        true,
        true,
        "UTF-8 custom or code source; contents are validated before typing."
    ),
    setting!(
        "test.generator_parameters.sentence_min_words",
        "generation",
        "Minimum sentence words",
        Kind::Integer(1, 64),
        false,
        true,
        "Lower sentence bound; cannot exceed the upper bound."
    ),
    setting!(
        "test.generator_parameters.sentence_max_words",
        "generation",
        "Maximum sentence words",
        Kind::Integer(1, 64),
        false,
        true,
        "Upper sentence bound for generated punctuation."
    ),
    setting!(
        "test.generator_parameters.comma_percent",
        "generation",
        "Comma probability",
        Kind::Integer(0, 100),
        false,
        true,
        "Interior comma probability as a whole percent."
    ),
    setting!(
        "test.generator_parameters.number_percent",
        "generation",
        "Number probability",
        Kind::Integer(0, 100),
        false,
        true,
        "Generated-token number replacement probability."
    ),
    setting!(
        "test.generator_parameters.number_min_digits",
        "generation",
        "Minimum number digits",
        Kind::Integer(1, 4),
        false,
        true,
        "Lower digit bound; numbers never have leading zeroes."
    ),
    setting!(
        "test.generator_parameters.number_max_digits",
        "generation",
        "Maximum number digits",
        Kind::Integer(1, 4),
        false,
        true,
        "Upper digit bound for generated numbers."
    ),
    setting!(
        "appearance.theme",
        "display",
        "Theme",
        Kind::Choice(crate::ui::THEMES),
        true,
        false,
        "Terminal-inherited or shipped independent theme."
    ),
    setting!(
        "appearance.focus",
        "display",
        "Focus",
        Kind::Choice(&["auto", "always", "off"]),
        true,
        false,
        "Auto hides secondary UI while typing; always shows only text and caret."
    ),
    setting!(
        "appearance.width",
        "display",
        "Text width",
        Kind::Width,
        true,
        false,
        "Auto caps at72 cells; explicit width40–120 is clamped to the terminal."
    ),
    setting!(
        "appearance.alignment",
        "display",
        "Alignment",
        Kind::Choice(&["center", "top"]),
        true,
        false,
        "Centered or top-aligned typing block."
    ),
    setting!(
        "appearance.lines",
        "display",
        "Text rows",
        Kind::Integer(1, 5),
        true,
        false,
        "Visible logical text rows; compact terminals use one."
    ),
    setting!(
        "appearance.line_spacing",
        "display",
        "Line spacing",
        Kind::Integer(0, 1),
        true,
        false,
        "Zero or one blank row between visible text rows."
    ),
    setting!(
        "appearance.caret",
        "display",
        "Caret",
        Kind::Choice(&["bar", "block", "underline"]),
        true,
        false,
        "Steady native caret with a styled-cell fallback."
    ),
    setting!(
        "appearance.tab_stop",
        "display",
        "Tab stops",
        Kind::Integer(1, 16),
        true,
        false,
        "Display-cell tab stops; a tab remains one scored unit."
    ),
    setting!(
        "appearance.color",
        "display",
        "Color policy",
        Kind::Choice(&["auto", "always", "never"]),
        true,
        false,
        "Auto honors NO_COLOR; explicit always overrides it."
    ),
    setting!(
        "appearance.error_underline",
        "display",
        "Underline errors",
        Kind::Boolean,
        true,
        false,
        "Non-color error cue; monochrome retains a visible distinction."
    ),
    setting!(
        "appearance.typed_bold",
        "display",
        "Bold typed text",
        Kind::Boolean,
        true,
        false,
        "Optional emphasis for retained correct output."
    ),
    setting!(
        "appearance.ascii_markers",
        "display",
        "ASCII markers",
        Kind::Boolean,
        true,
        false,
        "Use simple ASCII whitespace/continuation markers when fonts lack symbols."
    ),
    setting!(
        "status.progress",
        "status",
        "Time and progress",
        Kind::Boolean,
        true,
        false,
        "Show the timer or completed/requested word count independently."
    ),
    setting!(
        "status.wpm",
        "status",
        "Live speed",
        Kind::Boolean,
        true,
        false,
        "Show cumulative speed, updated at no more than4Hz."
    ),
    setting!(
        "status.accuracy",
        "status",
        "Live accuracy",
        Kind::Boolean,
        true,
        false,
        "Show cumulative attempt accuracy independently."
    ),
    setting!(
        "status.speed_unit",
        "status",
        "Speed unit",
        Kind::Choice(&["wpm", "cpm"]),
        true,
        false,
        "WPM uses five credited units; CPM shows credited units per minute."
    ),
    setting!(
        "rules.difficulty",
        "rules",
        "Difficulty",
        Kind::Choice(&["normal", "expert", "master"]),
        true,
        true,
        "Expert fails at incorrect commit; master fails on definite wrong input."
    ),
    setting!(
        "rules.backspace",
        "rules",
        "Backspace policy",
        Kind::Choice(&["mistakes", "current", "full", "none"]),
        true,
        true,
        "Correction boundary; historical attempts remain after deletion."
    ),
    setting!(
        "rules.stop_on_error",
        "rules",
        "Stop on error",
        Kind::Choice(&["off", "letter", "word"]),
        true,
        true,
        "Letter blocks wrong advancement; word blocks incorrect submission."
    ),
    setting!(
        "rules.blind",
        "rules",
        "Blind appearance",
        Kind::Boolean,
        true,
        false,
        "Hide error styling until Results without changing scoring or failures."
    ),
    setting!(
        "rules.minimum_wpm",
        "rules",
        "Minimum WPM",
        Kind::OptionalNumber(0.0, 1000.0),
        true,
        true,
        "Optional positive threshold; evaluated after the defined warmup."
    ),
    setting!(
        "rules.minimum_accuracy",
        "rules",
        "Minimum accuracy",
        Kind::OptionalNumber(0.0, 100.0),
        true,
        true,
        "Optional positive percentage threshold after20 finalized attempts."
    ),
    setting!(
        "practice.pace",
        "practice",
        "Pace caret",
        Kind::Pace,
        true,
        true,
        "Off, matching personal_best, or fixed1–1000 WPM; assistance excludes standard records."
    ),
    setting!(
        "practice.auto_indent",
        "practice",
        "Code auto-indent",
        Kind::Boolean,
        true,
        true,
        "Insert expected indentation as assisted units in exact text."
    ),
    setting!(
        "practice.selection",
        "practice",
        "Practice selection",
        Kind::Choice(&["missed", "slow"]),
        true,
        false,
        "Default immediate-practice choice; each uses original target words."
    ),
    setting!(
        "privacy.save_results",
        "privacy",
        "Save results",
        Kind::Boolean,
        true,
        false,
        "Persist summaries and samples after completion, subject to private-session lock."
    ),
    setting!(
        "privacy.store_custom_text",
        "privacy",
        "Store custom text",
        Kind::Boolean,
        true,
        false,
        "Explicitly opt in to private custom-text persistence."
    ),
    setting!(
        "privacy.store_event_trace",
        "privacy",
        "Store diagnostic trace",
        Kind::Boolean,
        false,
        false,
        "Explicitly opt in to bounded diagnostic trace capture; disabled in private sessions."
    ),
    setting!(
        "privacy.private_session",
        "privacy",
        "Private session",
        Kind::Boolean,
        true,
        false,
        "Disable result persistence and diagnostic text capture for the whole session."
    ),
    setting!(
        "workflow.favorite_themes",
        "workflow",
        "Favorite themes",
        Kind::Strings(32),
        true,
        false,
        "Ordered favorite shipped themes."
    ),
    setting!(
        "workflow.favorite_packs",
        "workflow",
        "Favorite packs",
        Kind::Strings(64),
        true,
        false,
        "Ordered favorite installed pack IDs."
    ),
    setting!(
        "workflow.bindings",
        "workflow",
        "Command bindings",
        Kind::Bindings,
        true,
        false,
        "Command-name to safe chord aliases; conflicts and literal typing keys are rejected."
    ),
    setting!(
        "workflow.result_details",
        "workflow",
        "Result details default",
        Kind::Boolean,
        true,
        false,
        "Open completed results with their detailed review by default."
    ),
    setting!(
        "storage.journal_mode",
        "storage",
        "SQLite journal mode",
        Kind::Choice(&["auto", "wal", "delete"]),
        false,
        false,
        "Auto uses the platform/filesystem policy; explicit modes remain configurable."
    ),
    setting!(
        "storage.pending_limit",
        "storage",
        "Pending result bound",
        Kind::Integer(1, 64),
        false,
        false,
        "Maximum immutable result snapshots retained while storage is unavailable."
    ),
    setting!(
        "enhanced_keyboard",
        "terminal",
        "Enhanced keyboard",
        Kind::Boolean,
        false,
        false,
        "Opt-in enhanced reporting; baseline controls remain available."
    ),
    setting!(
        "presets",
        "workflow",
        "Named presets",
        Kind::Presets,
        true,
        false,
        "Named typed data patches, never shell commands or executable plugins."
    ),
];

pub fn find(path: &str) -> Option<&'static Setting> {
    REGISTRY.iter().find(|setting| setting.path == path)
}
pub fn value_at<'a>(value: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    path.split('.').try_fold(value, |item, key| item.get(key))
}
pub fn safe_text(text: &str) -> bool {
    !text.chars().any(|c| {
        c.is_control()
            || matches!(c as u32, 0x061c | 0x200e..=0x200f | 0x202a..=0x202e | 0x2066..=0x2069)
    })
}
impl Setting {
    pub fn value(&self, config: &Config) -> Option<toml::Value> {
        toml::Value::try_from(config)
            .ok()
            .and_then(|value| value_at(&value, self.path).cloned())
    }
    /// Defaults are read from the canonical typed defaults, never a second set
    /// of hand-maintained palette/CLI literals.
    pub fn default_value(&self) -> Option<toml::Value> {
        self.value(&Config::default())
    }
    pub fn choices(&self) -> &'static [&'static str] {
        match self.kind {
            Kind::Boolean => &["false", "true"],
            Kind::Choice(values) => values,
            Kind::Width => &["auto", "40", "60", "72", "80", "100", "120"],
            Kind::Pace => &["off", "personal_best", "40", "60", "80", "100", "120"],
            Kind::OptionalNumber(..) | Kind::OptionalText(_) => &["unset"],
            _ => &[],
        }
    }
    pub fn parse(&self, raw: &str) -> Result<Option<toml::Value>, String> {
        let raw = raw.trim();
        if matches!(self.kind, Kind::OptionalNumber(..) | Kind::OptionalText(_))
            && matches!(raw, "unset" | "off")
        {
            return Ok(None);
        }
        let value = match self.kind {
            Kind::Choice(_) | Kind::Text(_) | Kind::OptionalText(_) | Kind::Pace => {
                if raw.starts_with('"') || raw.starts_with('\'') {
                    parse_literal(raw).map_err(|error| format!("{}: {error}", self.path))?
                } else {
                    toml::Value::String(raw.into())
                }
            }
            Kind::Width if raw == "auto" => toml::Value::String("auto".into()),
            _ => parse_literal(raw).map_err(|error| format!("{}: {error}", self.path))?,
        };
        self.validate(Some(&value))?;
        Ok(Some(value))
    }
    pub fn next_value(&self, config: &Config) -> Option<toml::Value> {
        let choices = self.choices();
        if choices.is_empty() {
            return None;
        }
        let current = self
            .value(config)
            .map(|v| match v {
                toml::Value::String(s) => s,
                _ => v.to_string(),
            })
            .unwrap_or_else(|| "unset".into());
        let next = choices
            .iter()
            .position(|v| *v == current)
            .map_or(0, |i| (i + 1) % choices.len());
        self.parse(choices[next]).ok().flatten()
    }
    pub fn validate(&self, value: Option<&toml::Value>) -> Result<(), String> {
        let Some(value) = value else {
            return if matches!(self.kind, Kind::OptionalNumber(..) | Kind::OptionalText(_)) {
                Ok(())
            } else {
                Err(format!("{}: required typed setting is missing", self.path))
            };
        };
        let number = || {
            value
                .as_float()
                .or_else(|| value.as_integer().map(|v| v as f64))
        };
        let valid = match self.kind {
            Kind::Boolean => value.is_bool(),
            Kind::Integer(min, max) => value.as_integer().is_some_and(|v| (min..=max).contains(&v)),
            Kind::Number(min, max) => {
                number().is_some_and(|v| v.is_finite() && (min..=max).contains(&v))
            }
            Kind::OptionalNumber(min, max) => {
                number().is_some_and(|v| v.is_finite() && v > min && v <= max)
            }
            Kind::Choice(values) => value.as_str().is_some_and(|v| values.contains(&v)),
            Kind::Text(max) | Kind::OptionalText(max) => value
                .as_str()
                .is_some_and(|v| !v.is_empty() && v.len() <= max && safe_text(v)),
            Kind::Strings(max) => value.as_array().is_some_and(|v| {
                v.len() <= max
                    && v.iter().all(|s| {
                        s.as_str()
                            .is_some_and(|s| !s.is_empty() && s.len() <= 128 && safe_text(s))
                    })
            }),
            Kind::Width => {
                value.as_str() == Some("auto")
                    || value.as_integer().is_some_and(|v| (40..=120).contains(&v))
            }
            Kind::Pace => value.as_str().is_some_and(|v| {
                matches!(v, "off" | "personal_best")
                    || v.parse::<f64>()
                        .is_ok_and(|v| v.is_finite() && (1.0..=1000.0).contains(&v))
            }),
            Kind::Bindings | Kind::Presets => value.as_table().is_some_and(|t| t.len() <= 64),
        };
        if valid {
            Ok(())
        } else {
            Err(format!(
                "{}={value:?}: expected {}",
                self.path,
                self.expected()
            ))
        }
    }
    fn expected(&self) -> String {
        match self.kind {
            Kind::Boolean => "true or false".into(),
            Kind::Integer(min, max) => format!("integer {min}–{max}"),
            Kind::Number(min, max) => format!("finite number {min}–{max}"),
            Kind::OptionalNumber(min, max) => {
                format!("finite number greater than {min} and at most {max}, or unset")
            }
            Kind::Choice(values) => values.join(", "),
            Kind::Text(max) | Kind::OptionalText(max) => {
                format!("nonempty control-free text at most {max} bytes")
            }
            Kind::Strings(max) => format!("at most {max} control-free strings"),
            Kind::Width => "auto or integer40–120".into(),
            Kind::Pace => "off, personal_best, or WPM1–1000".into(),
            Kind::Bindings => "at most64 command-to-chord bindings".into(),
            Kind::Presets => "at most64 typed data presets".into(),
        }
    }
}
fn parse_literal(raw: &str) -> Result<toml::Value, String> {
    let document: toml::Table = toml::from_str(&format!("value = {raw}"))
        .map_err(|_| "expected one TOML value (quote text, use [..] for lists)".to_owned())?;
    if document.len() != 1 {
        return Err("expected exactly one TOML value".into());
    }
    document
        .get("value")
        .cloned()
        .ok_or_else(|| "expected a TOML value".into())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub path: String,
    pub value: Option<toml::Value>,
}
impl Edit {
    pub fn parse(path: &str, raw: &str) -> Result<Self, String> {
        let setting = find(path).ok_or_else(|| format!("unknown setting {path:?}"))?;
        Ok(Self {
            path: path.into(),
            value: setting.parse(raw)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandAction {
    Palette,
    NewSample,
    RepeatSample,
    NextSample,
    Finish,
    DeleteWord,
    Quit,
    Details,
    Practice,
    PracticeMissed,
    PracticeSlow,
    History,
    Help,
    Config,
    SaveDefault,
    Export,
    RetrySave,
}
impl CommandAction {
    pub const ALL: &'static [Self] = &[
        Self::Palette,
        Self::NewSample,
        Self::RepeatSample,
        Self::NextSample,
        Self::Finish,
        Self::DeleteWord,
        Self::Quit,
        Self::Details,
        Self::Practice,
        Self::PracticeMissed,
        Self::PracticeSlow,
        Self::History,
        Self::Help,
        Self::Config,
        Self::SaveDefault,
        Self::Export,
        Self::RetrySave,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Palette => "palette",
            Self::NewSample => "new_sample",
            Self::RepeatSample => "repeat_sample",
            Self::NextSample => "next_sample",
            Self::Finish => "finish",
            Self::DeleteWord => "delete_word",
            Self::Quit => "quit",
            Self::Details => "details",
            Self::Practice => "practice",
            Self::PracticeMissed => "practice_missed",
            Self::PracticeSlow => "practice_slow",
            Self::History => "history",
            Self::Help => "help",
            Self::Config => "config",
            Self::SaveDefault => "save_default",
            Self::Export => "export",
            Self::RetrySave => "retry_save",
        }
    }
    pub fn parse(name: &str) -> Result<Self, String> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.name() == name)
            .ok_or_else(|| format!("workflow.bindings: unknown command {name:?}"))
    }
    fn allowed(self, context: BindingContext) -> bool {
        match self {
            Self::NextSample
            | Self::Details
            | Self::Practice
            | Self::PracticeMissed
            | Self::PracticeSlow => context == BindingContext::Results,
            Self::DeleteWord | Self::Finish => {
                matches!(context, BindingContext::Ready | BindingContext::Running)
            }
            _ => true,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingContext {
    Ready,
    Running,
    Results,
    Overlay,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Chord {
    code: KeyCode,
    modifiers: KeyModifiers,
}
impl Chord {
    fn parse(text: &str) -> Result<Self, String> {
        let lower = text.trim().to_ascii_lowercase();
        if lower.len() > 64 {
            return Err("binding chord exceeds64 bytes".into());
        }
        let mut parts: Vec<_> = lower.split('+').collect();
        let key = parts.pop().unwrap_or("");
        let mut modifiers = KeyModifiers::NONE;
        for part in parts {
            let flag = match part {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => return Err(format!("unknown binding modifier {part:?}")),
            };
            if modifiers.contains(flag) {
                return Err("duplicate binding modifier".into());
            }
            modifiers.insert(flag);
        }
        let code = if key == "esc" {
            KeyCode::Esc
        } else if let Some(number) = key
            .strip_prefix('f')
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=24).contains(n))
        {
            KeyCode::F(number)
        } else if key.len() == 1
            && key.as_bytes()[0].is_ascii_lowercase()
            && modifiers == KeyModifiers::CONTROL
        {
            if matches!(key, "h" | "i" | "j" | "m") {
                return Err("Ctrl-H/I/J/M are ambiguous with Backspace/Tab/Enter".into());
            }
            KeyCode::Char(key.chars().next().expect("one ASCII byte"))
        } else {
            return Err("use Esc, F1–F24, or an unambiguous Ctrl-letter; printable keys, Tab, and Enter belong to typing".into());
        };
        if code == KeyCode::Esc && !modifiers.is_empty() {
            return Err("modified Escape is not a portable unambiguous chord".into());
        }
        Ok(Self { code, modifiers })
    }
    fn matches(&self, key: &KeyEvent) -> bool {
        let essential_control = self.modifiers == KeyModifiers::CONTROL
            && matches!(self.code, KeyCode::Char('c' | 'r' | 'p' | 'w'));
        let mut modifiers = key.modifiers;
        if essential_control {
            modifiers.remove(KeyModifiers::SHIFT);
        }
        let code = match key.code {
            KeyCode::Char(c) if modifiers == KeyModifiers::CONTROL => {
                KeyCode::Char(c.to_ascii_lowercase())
            }
            other => other,
        };
        code == self.code && modifiers == self.modifiers
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    chords: Vec<(Chord, CommandAction)>,
}
impl Default for Bindings {
    fn default() -> Self {
        use CommandAction::*;
        let mut chords = Vec::with_capacity(32);
        for (text, action) in [
            ("esc", Palette),
            ("ctrl+p", Palette),
            ("ctrl+r", NewSample),
            ("f2", RepeatSample),
            ("f3", Practice),
            ("f4", Details),
            ("f5", Finish),
            ("ctrl+w", DeleteWord),
            ("ctrl+c", Quit),
        ] {
            chords.push((Chord::parse(text).expect("constant valid chord"), action));
        }
        chords.push((
            Chord {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
            },
            NextSample,
        ));
        Self { chords }
    }
}
impl Bindings {
    pub fn from_config(config: &Config) -> Result<Self, String> {
        Self::from_map(&config.workflow.bindings)
    }
    pub fn from_map(map: &BTreeMap<String, String>) -> Result<Self, String> {
        if map.len() > 64 {
            return Err("workflow.bindings: at most64 aliases are supported".into());
        }
        let mut bindings = Self::default();
        for (name, text) in map {
            let action = CommandAction::parse(name)?;
            let chord = Chord::parse(text)
                .map_err(|error| format!("workflow.bindings.{name}={text:?}: {error}"))?;
            if let Some((_, existing)) = bindings
                .chords
                .iter()
                .find(|(existing, _)| existing == &chord)
            {
                if *existing != action {
                    return Err(format!(
                        "workflow.bindings.{name}={text:?}: conflicts with {}",
                        existing.name()
                    ));
                }
            } else {
                bindings.chords.push((chord, action));
            }
        }
        Ok(bindings)
    }
    pub fn resolve(&self, key: &KeyEvent, context: BindingContext) -> Option<CommandAction> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        self.chords
            .iter()
            .find(|(chord, action)| action.allowed(context) && chord.matches(key))
            .map(|(_, action)| *action)
    }
}
