use super::*;
use crate::settings::{self, CommandAction, Edit};

pub(super) enum Deferred {
    Edits(Vec<Edit>),
    Preset(String),
    SavePreset(String),
    SaveDefaults,
    Export(String),
    OriginalFile(String),
    HistoryFilters(String),
    RetrySave,
}

impl App {
    pub(super) fn restore_preview(&mut self) {
        if let Some(palette) = &self.palette {
            self.config.appearance.theme = palette.original_theme.clone();
        }
    }
    pub(super) fn palette_cancel(&mut self, reader: &Reader, now: u64) -> Result<(), String> {
        if self
            .palette
            .as_ref()
            .is_some_and(|palette| palette.editor.is_some())
        {
            self.restore_preview();
            let palette = self.palette.as_mut().expect("palette");
            palette.editor = None;
            palette.message = None;
            self.dirty = true;
            Ok(())
        } else {
            self.close_palette(reader, now)
        }
    }
    pub(super) fn preview_theme(&mut self) {
        let preview = self.palette.as_ref().and_then(|palette| {
            if let Some(editor) = &palette.editor {
                match &editor.purpose {
                    EditPurpose::Setting(setting) if setting.path == "appearance.theme" => {
                        Some(editor.value.clone())
                    }
                    _ => None,
                }
            } else {
                palette.selected_entry().and_then(|entry| {
                    if let EntryKind::Theme(theme) = &entry.kind {
                        Some(theme.clone())
                    } else {
                        None
                    }
                })
            }
        });
        if let Some(theme) = preview.filter(|theme| ui::THEMES.contains(&theme.as_str())) {
            self.config.appearance.theme = theme;
        } else {
            self.restore_preview();
        }
    }
    pub(super) fn palette_select(&mut self, reader: &Reader, now: u64) -> Result<(), String> {
        let Some(palette) = &self.palette else {
            return Ok(());
        };
        if let Some(editor) = &palette.editor {
            let value = editor.value.clone();
            let purpose = editor.purpose.clone();
            let job = match purpose {
                EditPurpose::Setting(setting) => {
                    let edit = match Edit::parse(setting.path, &value) {
                        Ok(edit) => edit,
                        Err(error) => {
                            self.palette.as_mut().expect("palette").message = Some(error);
                            self.dirty = true;
                            return Ok(());
                        }
                    };
                    let mut edits = vec![edit];
                    if setting.path == "test.mode" {
                        if value == "code" {
                            edits.push(Edit::parse("test.policy", "exact")?);
                            edits.push(Edit::parse("test.completion", "confirm")?);
                        } else if matches!(value.as_str(), "time" | "words" | "quote" | "zen") {
                            edits.push(Edit::parse("test.policy", "prose")?);
                            edits.push(Edit::parse("practice.auto_indent", "false")?);
                            edits.push(Edit::parse("test.normalize_exact", "false")?);
                        }
                    }
                    Deferred::Edits(edits)
                }
                EditPurpose::SavePreset => Deferred::SavePreset(value),
                EditPurpose::Export => Deferred::Export(value),
                EditPurpose::OriginalFile => Deferred::OriginalFile(value),
                EditPurpose::HistoryFilters => Deferred::HistoryFilters(value),
            };
            self.deferred = Some(job);
            self.dirty = true;
            return Ok(());
        }
        let Some(entry) = palette.selected_entry().cloned() else {
            return Ok(());
        };
        match entry.kind {
            EntryKind::Action(action) => {
                if !matches!(
                    action,
                    CommandAction::SaveDefault | CommandAction::Export | CommandAction::RetrySave
                ) {
                    self.restore_preview();
                }
                self.dispatch(action, reader, now)?;
            }
            EntryKind::Preset(name) => self.deferred = Some(Deferred::Preset(name)),
            EntryKind::Theme(theme) => {
                self.deferred = Some(Deferred::Edits(vec![Edit::parse(
                    "appearance.theme",
                    &theme,
                )?]))
            }
            EntryKind::Pack(pack) => {
                self.deferred = Some(Deferred::Edits(vec![Edit::parse("test.language", &pack)?]))
            }
            EntryKind::SavePreset => {
                self.palette.as_mut().expect("palette").editor = Some(Editor {
                    purpose: EditPurpose::SavePreset,
                    value: String::new(),
                    choices: Vec::new(),
                    replace_on_type: true,
                });
            }
            EntryKind::Setting(setting) => {
                self.restore_preview();
                let mut choices: Vec<String> = setting
                    .choices()
                    .iter()
                    .map(|value| (*value).into())
                    .collect();
                if setting.path == "test.language" {
                    choices = crate::content::BUNDLED_PACK_IDS
                        .iter()
                        .map(|value| (*value).into())
                        .chain(self.config.workflow.favorite_packs.clone())
                        .collect();
                    choices.sort();
                    choices.dedup();
                }
                self.palette.as_mut().expect("palette").begin_setting(
                    setting,
                    &self.config,
                    choices,
                );
            }
        }
        self.dirty = true;
        Ok(())
    }
    pub(super) fn apply_deferred(
        &mut self,
        reader: &Reader,
        session: &mut Session,
        now: u64,
    ) -> Result<(), String> {
        let Some(job) = self.deferred.take() else {
            return Ok(());
        };
        self.restore_preview();
        let outcome = match job {
            Deferred::Edits(edits) => self.apply_setting_edits(&edits, reader, session, now),
            Deferred::Preset(name) => match self.config.preset_edits(&name) {
                Ok(edits) => self.apply_setting_edits(&edits, reader, session, now),
                Err(error) => Err(error),
            },
            Deferred::SavePreset(name) => self
                .config
                .save_preset(&self._paths.config, &name)
                .map(|()| "Named preset saved".into()),
            Deferred::SaveDefaults => self
                .config
                .save_defaults(&self._paths.config)
                .map(|()| "Current settings saved as defaults".into()),
            Deferred::Export(path) => self.recovery_export(&path),
            Deferred::OriginalFile(path) => self.original_file(&path),
            Deferred::HistoryFilters(raw) => self.history_filters(&raw),
            Deferred::RetrySave => self.retry_saves(),
        };
        match outcome {
            Ok(message) => {
                if let Some(palette) = &mut self.palette {
                    palette.editor = None;
                    palette.original_theme = self.config.appearance.theme.clone();
                    palette.refresh(&self.config);
                    palette.message = Some(message);
                } else {
                    self.report(message);
                }
            }
            Err(error) => self.report(error),
        }
        self.dirty = true;
        Ok(())
    }
    fn apply_setting_edits(
        &mut self,
        edits: &[Edit],
        reader: &Reader,
        session: &mut Session,
        now: u64,
    ) -> Result<String, String> {
        if self.private_locked
            && edits.iter().any(|edit| {
                edit.path == "privacy.private_session"
                    && edit.value == Some(toml::Value::Boolean(false))
            })
        {
            return Err("Private mode stays active until this session ends".into());
        }
        let candidate = self.config.edited(edits)?;
        let changes_test = edits.iter().any(|edit| {
            settings::find(&edit.path).is_some_and(|setting| setting.affects_test)
                || edit.path == "rules.blind"
        });
        let prepared = if changes_test {
            let mut sources = self.samples.clone();
            sources.reconfigure(&candidate, &self._paths)?;
            let sample = sources.next(&candidate)?;
            Some((sources, sample))
        } else {
            None
        };
        let bindings = Arc::new(settings::Bindings::from_config(&candidate)?);
        let style = |caret| match caret {
            Caret::Bar => CaretStyle::Bar,
            Caret::Block => CaretStyle::Block,
            Caret::Underline => CaretStyle::Underline,
        };
        let old_caret = style(self.config.appearance.caret);
        let old_enhanced = self.config.enhanced_keyboard;
        // Capability failure must not commit an unusable configuration. Both
        // setters are reversible; compensate if the atomic file edit fails.
        session
            .set_enhanced_keyboard(candidate.enhanced_keyboard)
            .map_err(|error| error.to_string())?;
        if let Err(error) = session.set_caret(style(candidate.appearance.caret)) {
            let _ = session.set_enhanced_keyboard(old_enhanced);
            return Err(error.to_string());
        }
        if let Err(error) = self.config.persist_edits(&self._paths.config, edits) {
            let caret = session.set_caret(old_caret);
            let enhanced = session.set_enhanced_keyboard(old_enhanced);
            if caret.is_err() || enhanced.is_err() {
                return Err(format!(
                    "{error}; terminal preference restoration also failed"
                ));
            }
            return Err(error);
        }
        self.private_locked |= self.config.privacy.private_session;
        if self.private_locked {
            self.config.privacy.private_session = true;
            self.config.privacy.save_results = false;
            self.config.privacy.store_custom_text = false;
            self.config.privacy.store_event_trace = false;
        }
        self.bindings = bindings;
        if self.private_locked {
            self.storage.trace = None;
        }
        if let Some((sources, sample)) = prepared {
            self.samples = sources;
            self.sample = sample;
            self.finalized = false;
            self.practice_return = None;
            self.review = None;
            self.details = false;
            self.history = None;
            self.reset_run_storage();
        }
        self.barrier(reader, false)?;
        let _ = now;
        Ok("Setting saved".into())
    }
    pub(super) fn open_original_editor(&mut self) {
        let mut palette = Palette::new(&self.config, false);
        palette.editor = Some(Editor {
            purpose: EditPurpose::OriginalFile,
            value: String::new(),
            choices: Vec::new(),
            replace_on_type: true,
        });
        self.palette = Some(palette);
        self.dirty = true;
    }
    pub(super) fn open_panel(
        &mut self,
        reader: &Reader,
        now: u64,
        configuration: bool,
    ) -> Result<(), String> {
        if self.palette.is_none() {
            self.open_palette(reader, now)?;
        }
        self.restore_preview();
        let return_ready = self
            .palette
            .take()
            .is_some_and(|palette| palette.return_ready);
        let lines = if configuration {
            let mut lines = vec![
                format!("Config: {:?}", self._paths.config),
                format!("Data: {:?}", self._paths.data),
                "Effective values (CLI overrides remain session-local):".into(),
                String::new(),
            ];
            let value = toml::Value::try_from(&self.config).map_err(|error| error.to_string())?;
            for setting in settings::REGISTRY {
                lines.push(format!(
                    "{} = {}",
                    setting.path,
                    settings::value_at(&value, setting.path)
                        .map_or_else(|| "unset".into(), ToString::to_string)
                ));
            }
            lines
        } else {
            [
                "Type the displayed text. The first eligible key starts immediately.",
                "Esc / Ctrl-P: commands; opening commands aborts an active test.",
                "Ctrl-R: new sample. F2: repeat the exact sample as practice.",
                "Enter on Results: next. F3: practice. F4: detailed review.",
                "F5: finish zen or confirm exact text. Ctrl-C: quit.",
                "Backspace removes one allowed grapheme; Ctrl-W removes a word.",
                "Exact mode owns literal Tab and Enter. No paste is scored.",
                "",
                "WPM = credited target units / 5 / elapsed minutes.",
                "Raw WPM = retained manually typed units / 5 / elapsed minutes.",
                "Accuracy counts correct attempts / all attempts, even after repair.",
                "Whole-word credit differs from raw speed multiplied by accuracy.",
                "Failed, assisted, repeated, interrupted and incomplete runs",
                "are excluded from standard local personal bests.",
                "",
                "History saves summaries and samples by default, after completion.",
                "Custom/zen text and per-key traces are not retained by default.",
                "--private disables result persistence for the whole session.",
                "",
                "Unicode scoring uses graphemes; display uses terminal cells.",
                "Full bidi, complex shaping and universal IME timing are unsupported.",
                "Legacy unbracketed paste and external macros are not detectable.",
                "",
                "Settings, themes, presets, history and recovery export are in commands.",
                "Custom command aliases: Configuration → workflow.bindings.",
                "clack --help, clack man, clack completions <shell>, clack doctor",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        };
        self.panel = Some(ui::panel::Panel {
            title: if configuration {
                "Configuration"
            } else {
                "Help"
            }
            .into(),
            lines,
            scroll: 0,
            return_ready,
        });
        self.details = false;
        self.dirty = true;
        Ok(())
    }
    pub(super) fn close_panel(&mut self, reader: &Reader, now: u64) -> Result<(), String> {
        let ready = self.panel.take().is_some_and(|panel| panel.return_ready);
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
}
