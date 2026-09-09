//! Searchable command and setting presentation; definitions come from the registry.
use super::{Appearance, ColorDepth, Geometry, Theme, line};
use crate::{
    config::Config,
    settings::{CommandAction, REGISTRY, Setting},
};
use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone)]
pub enum EntryKind {
    Action(CommandAction),
    Setting(&'static Setting),
    Preset(String),
    SavePreset,
    Theme(String),
    Pack(String),
}
#[derive(Clone)]
pub struct Entry {
    pub label: String,
    pub value: String,
    pub help: String,
    pub search: String,
    pub kind: EntryKind,
}
#[derive(Clone)]
pub enum EditPurpose {
    Setting(&'static Setting),
    SavePreset,
    Export,
    OriginalFile,
    HistoryFilters,
}
pub struct Editor {
    pub purpose: EditPurpose,
    pub value: String,
    pub choices: Vec<String>,
    pub replace_on_type: bool,
}
pub struct Palette {
    pub query: String,
    pub selected: usize,
    pub return_ready: bool,
    pub entries: Vec<Entry>,
    pub editor: Option<Editor>,
    pub original_theme: String,
    pub message: Option<String>,
}
impl Palette {
    pub fn new(config: &Config, return_ready: bool) -> Self {
        let mut palette = Self {
            query: String::new(),
            selected: 0,
            return_ready,
            entries: Vec::new(),
            editor: None,
            original_theme: config.appearance.theme.clone(),
            message: None,
        };
        palette.refresh(config);
        palette
    }
    pub fn refresh(&mut self, config: &Config) {
        use CommandAction::*;
        self.entries.clear();
        for (action, label, help) in [
            (
                NewSample,
                "New sample",
                "Start fresh text with the current settings.",
            ),
            (
                RepeatSample,
                "Repeat sample",
                "Reuse the exact text and settings as practice.",
            ),
            (
                Finish,
                "Finish",
                "Finish zen or confirm exact text with F5 or a configured alias; opening commands aborts an active test.",
            ),
            (
                Details,
                "Details",
                "Review speed history, mistaken attempts and final text.",
            ),
            (
                PracticeMissed,
                "Practice missed",
                "Practice original words with mistakes or omissions, including repaired words.",
            ),
            (
                PracticeSlow,
                "Practice slow",
                "Needs eight eligible correct words without corrections; excludes the first word.",
            ),
            (
                History,
                "History",
                "Browse paginated results for the current matching profile.",
            ),
            (
                Config,
                "Configuration",
                "Inspect effective values and resolved local paths.",
            ),
            (
                Help,
                "Help",
                "View controls, scoring, privacy and compatibility notes.",
            ),
            (
                SaveDefault,
                "Save current as default",
                "Intentionally persist all effective settings, including CLI overrides.",
            ),
            (
                Export,
                "Export current and unsaved results",
                "Write a recovery export to a new file; private text is excluded.",
            ),
            (
                RetrySave,
                "Retry unsaved results",
                "Retry retained snapshots with idempotent commit IDs.",
            ),
            (
                DeleteWord,
                "Delete word",
                "Ctrl-W deletes within the correction range during typing.",
            ),
            (
                NextSample,
                "Next sample",
                "Start another sample using the current settings.",
            ),
            (
                Quit,
                "Quit",
                "Restore the terminal and report any unacknowledged saves.",
            ),
        ] {
            self.entries.push(Entry {
                label: label.into(),
                value: String::new(),
                help: help.into(),
                search: format!("{} {}", label.to_lowercase(), action.name()),
                kind: EntryKind::Action(action),
            });
        }
        let values = toml::Value::try_from(config).expect("typed settings serialize");
        for setting in REGISTRY {
            let value = crate::settings::value_at(&values, setting.path)
                .map_or_else(|| "unset".into(), display_value);
            self.entries.push(Entry {
                label: setting.label.into(),
                value,
                help: setting.help.into(),
                search: format!(
                    "{} {} {}",
                    setting.label.to_lowercase(),
                    setting.path,
                    setting.group
                ),
                kind: EntryKind::Setting(setting),
            });
        }
        for name in config.preset_names() {
            self.entries.push(Entry {
                label: format!("Preset {name}"),
                value: String::new(),
                help: "Apply this named settings data and persist its fields.".into(),
                search: format!("preset {name}").to_lowercase(),
                kind: EntryKind::Preset(name),
            });
        }
        self.entries.push(Entry {
            label: "Save named preset".into(),
            value: String::new(),
            help: "Save the effective configuration as a named reusable preset.".into(),
            search: "save named preset".into(),
            kind: EntryKind::SavePreset,
        });
        for name in super::THEMES {
            self.entries.push(Entry {
                label: format!(
                    "Theme {}{name}",
                    if config
                        .workflow
                        .favorite_themes
                        .iter()
                        .any(|favorite| favorite == name)
                    {
                        "* "
                    } else {
                        ""
                    }
                ),
                value: String::new(),
                help:
                    "Arrow keys preview; Enter saves this theme; Esc restores the previous theme."
                        .into(),
                search: format!("theme {name}"),
                kind: EntryKind::Theme((*name).into()),
            });
        }
        for name in &config.workflow.favorite_packs {
            self.entries.push(Entry {
                label: format!("Favorite pack {name}"),
                value: String::new(),
                help: "Select this installed language pack.".into(),
                search: format!("language favorite pack {name}").to_lowercase(),
                kind: EntryKind::Pack(name.clone()),
            });
        }
    }
    pub fn matches(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                query
                    .split_whitespace()
                    .all(|term| entry.search.contains(term))
            })
            .map(|(index, _)| index)
            .collect()
    }
    pub fn selected_entry(&self) -> Option<&Entry> {
        self.matches()
            .get(self.selected)
            .map(|index| &self.entries[*index])
    }
    pub fn insert(&mut self, text: &str) -> bool {
        if !crate::settings::safe_text(text) {
            let changed =
                self.message.as_deref() != Some("Enter a single line without control characters");
            self.message = Some("Enter a single line without control characters".into());
            return changed;
        }
        let (length, limit) = self
            .editor
            .as_ref()
            .map_or((self.query.len(), 512), |editor| {
                (
                    if editor.replace_on_type {
                        0
                    } else {
                        editor.value.len()
                    },
                    65_536,
                )
            });
        if length.saturating_add(text.len()) > limit {
            let message = format!("Input is limited to {limit} bytes");
            let changed = self.message.as_ref() != Some(&message);
            self.message = Some(message);
            return changed;
        }
        let mut changed = self.message.is_some();
        let value = if let Some(editor) = &mut self.editor {
            if editor.replace_on_type {
                changed |= editor.value != text;
                editor.value.clear();
                editor.replace_on_type = false;
            } else {
                changed |= !text.is_empty();
            }
            &mut editor.value
        } else {
            changed |= self.selected != 0 || !text.is_empty();
            self.selected = 0;
            &mut self.query
        };
        value.push_str(text);
        self.message = None;
        changed
    }
    pub fn backspace(&mut self) -> bool {
        let mut changed = self.message.is_some();
        let value = if let Some(editor) = &mut self.editor {
            if editor.replace_on_type {
                changed |= !editor.value.is_empty();
                editor.value.clear();
                editor.replace_on_type = false;
            }
            &mut editor.value
        } else {
            changed |= self.selected != 0;
            self.selected = 0;
            &mut self.query
        };
        if let Some((start, _)) = value.grapheme_indices(true).next_back() {
            value.truncate(start);
            changed = true;
        }
        self.message = None;
        changed
    }
    pub fn move_by(&mut self, delta: isize) -> bool {
        let mut changed = self.message.is_some();
        self.message = None;
        if let Some(editor) = &mut self.editor {
            if editor.choices.is_empty() {
                return changed;
            }
            let current = editor
                .choices
                .iter()
                .position(|value| value == &editor.value)
                .unwrap_or(0);
            let next =
                (current as isize + delta).rem_euclid(editor.choices.len() as isize) as usize;
            changed |= editor.value != editor.choices[next];
            editor.value.clone_from(&editor.choices[next]);
            editor.replace_on_type = true;
        } else {
            let previous = self.selected;
            self.selected = self
                .selected
                .saturating_add_signed(delta)
                .min(self.matches().len().saturating_sub(1));
            changed |= previous != self.selected;
        }
        changed
    }
    pub fn begin_setting(
        &mut self,
        setting: &'static Setting,
        config: &Config,
        choices: Vec<String>,
    ) {
        let value = setting
            .value(config)
            .map_or_else(|| "unset".into(), |value| display_value(&value));
        self.editor = Some(Editor {
            purpose: EditPurpose::Setting(setting),
            value,
            choices,
            replace_on_type: true,
        });
        self.message = None;
    }
}
fn display_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

pub fn render(frame: &mut Frame, palette: &Palette, appearance: &Appearance) {
    let theme = Theme::from_preferences(appearance, ColorDepth::detect(appearance.color));
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.background);
    let Some(geometry) = Geometry::new(area, appearance) else {
        super::message(frame, "Resize to at least 40 × 10.", theme.muted);
        return;
    };
    let (x, width) = (geometry.text.x, geometry.text.width);
    let y = 2;
    if let Some(editor) = &palette.editor {
        let (label, help) = match &editor.purpose {
            EditPurpose::Setting(setting) => (setting.label, setting.help),
            EditPurpose::SavePreset => (
                "Save named preset",
                "Enter a simple name; existing names are updated atomically.",
            ),
            EditPurpose::Export => (
                "Export recovery results",
                "Enter a new JSONL file path. Existing files are never overwritten.",
            ),
            EditPurpose::OriginalFile => (
                "Original source file",
                "The normalized content must match the historical content hash.",
            ),
            EditPurpose::HistoryFilters => (
                "History filters",
                "↑↓ examples; edit profile, classification, outcome, mode, language, from/to UTC.",
            ),
        };
        line(frame, x, y, width, label, theme.correct);
        let value: String = editor
            .value
            .graphemes(true)
            .rev()
            .take(usize::from(width.saturating_sub(3)))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        line(frame, x, y + 2, width, format!("> {value}"), theme.correct);
        let selected = editor
            .choices
            .iter()
            .position(|value| value == &editor.value)
            .unwrap_or(0);
        let first = selected.saturating_sub(3);
        for (offset, (index, choice)) in editor
            .choices
            .iter()
            .enumerate()
            .skip(first)
            .take(7.min(usize::from(area.height.saturating_sub(10))))
            .enumerate()
        {
            line(
                frame,
                x,
                y + 4 + offset as u16,
                width,
                format!("{} {choice}", if index == selected { ">" } else { " " }),
                if index == selected {
                    theme.accent
                } else {
                    theme.muted
                },
            );
        }
        line(
            frame,
            x,
            area.height - 3,
            width,
            palette.message.as_deref().unwrap_or(help),
            theme.muted,
        );
        line(
            frame,
            x,
            area.height - 2,
            width,
            "enter apply  ↑↓ choices  esc cancel",
            theme.muted,
        );
        frame.set_cursor_position((
            x + 2
                + unicode_width::UnicodeWidthStr::width(value.as_str())
                    .min(usize::from(width.saturating_sub(3))) as u16,
            y + 2,
        ));
        return;
    }
    line(
        frame,
        x,
        y,
        width,
        format!("> {}", palette.query),
        theme.correct,
    );
    let matches = palette.matches();
    let visible = 7.min(usize::from(area.height.saturating_sub(8))).max(1);
    let first = palette.selected.saturating_sub(visible - 1);
    for (offset, index) in matches.iter().skip(first).take(visible.max(1)).enumerate() {
        let entry = &palette.entries[*index];
        let selected = first + offset == palette.selected;
        line(
            frame,
            x,
            y + 2 + offset as u16,
            width,
            format!(
                "{} {}{}",
                if selected { ">" } else { " " },
                entry.label,
                if entry.value.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", entry.value)
                }
            ),
            if selected { theme.accent } else { theme.muted },
        );
    }
    let help = palette
        .message
        .as_deref()
        .or_else(|| palette.selected_entry().map(|entry| entry.help.as_str()))
        .unwrap_or("No matching command or setting");
    line(frame, x, area.height - 3, width, help, theme.muted);
    line(
        frame,
        x,
        area.height - 2,
        width,
        "enter select  ↑↓ move  esc close",
        theme.muted,
    );
    frame.set_cursor_position((
        x + 2
            + unicode_width::UnicodeWidthStr::width(palette.query.as_str())
                .min(usize::from(width.saturating_sub(3))) as u16,
        y,
    ));
}
