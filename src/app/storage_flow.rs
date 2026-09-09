//! UI coordination only. SQLite and all history reads belong to the one worker.
use super::*;
use crate::storage::{
    self, Best, BestOutcome, Event, EventTrace, Filter, Options, Page, Persistence, Record, Store,
    TraceEvent, TraceKind,
};
#[cfg(not(windows))]
use std::fs::OpenOptions;
use std::{io::Write, path::Path};

#[derive(Default)]
pub(super) struct StorageFlow {
    pub store: Option<Store>,
    pub first_frame: bool,
    pub requested: bool,
    attempted: bool,
    configured: Option<(bool, String, usize)>,
    pub visible_epoch: Option<u64>,
    current_ticket: Option<(u64, u64)>,
    last_record: Option<Record>,
    overflow: Option<(u64, Record)>,
    lost_unsaved: usize,
    failure: Option<String>,
    pub trace: Option<EventTrace>,
    pub stored_review: Option<storage::Review>,
    pace_request: Option<(u64, String)>,
    pace_cache: Option<(String, Option<Best>)>,
    pub pace_checked: bool,
}

impl App {
    pub(super) fn report(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some(palette) = &mut self.palette {
            palette.message = Some(message);
        } else if self.details {
            if let Some(review) = &mut self.review {
                review.notice = Some(message);
            }
        } else if let Some(history) = &mut self.history {
            history.message = Some(message);
        } else if let Some(panel) = &mut self.panel {
            panel
                .lines
                .insert(panel.scroll.min(panel.lines.len()), message);
        } else {
            self.notice = Some(message);
        }
        self.dirty = true;
    }

    pub(super) fn reset_run_storage(&mut self) {
        self.storage.visible_epoch = None;
        self.storage.current_ticket = None;
        self.storage.trace = None;
        self.storage.pace_checked = false;
        self.storage.stored_review = None;
    }

    fn storage_options(&self) -> Result<Options, String> {
        let mut options = Options::new(self._paths.database.clone());
        options.read_only = self.private_locked || !self.config.privacy.save_results;
        options.pending_limit = self.config.storage.pending_limit;
        options.journal = storage::JournalMode::parse(&self.config.storage.journal_mode)
            .map_err(|error| error.to_string())?;
        Ok(options)
    }

    // Called only after a complete usable frame, or after a later bounded input batch.
    pub(super) fn service_storage(&mut self) {
        if !self.storage.first_frame {
            return;
        }
        let wanted = (
            self.private_locked || !self.config.privacy.save_results,
            self.config.storage.journal_mode.clone(),
            self.config.storage.pending_limit,
        );
        let needed =
            !wanted.0 || self.storage.requested || self.config.practice.pace == "personal_best";
        if self.storage.store.is_none()
            && needed
            && (!self.storage.attempted || self.storage.configured.as_ref() != Some(&wanted))
        {
            self.storage.attempted = true;
            self.storage.configured = Some(wanted.clone());
            match self.storage_options().and_then(|options| {
                Store::start_after_frame(options, thread::current())
                    .map_err(|error| error.to_string())
            }) {
                Ok(store) => {
                    self.storage.store = Some(store);
                    self.storage.configured = Some(wanted.clone());
                }
                Err(error) => {
                    self.storage.failure = Some(error.clone());
                    self.report(format!("Storage unavailable · {error}"));
                    return;
                }
            }
        }
        if self.storage.store.is_some() && self.storage.configured.as_ref() != Some(&wanted) {
            match self.storage_options().and_then(|options| {
                self.storage
                    .store
                    .as_mut()
                    .expect("store")
                    .reconfigure(options)
                    .map_err(|error| error.to_string())
            }) {
                Ok(()) => {
                    self.storage.configured = Some(wanted);
                    self.storage.failure = None;
                }
                Err(error) => {
                    self.storage.failure = Some(error);
                }
            }
        }
        let events = self
            .storage
            .store
            .as_mut()
            .map_or_else(Vec::new, Store::poll);
        for event in events {
            self.storage_event(event);
        }
        if self
            .storage
            .configured
            .as_ref()
            .is_some_and(|options| !options.0)
            && self
                .storage
                .store
                .as_ref()
                .is_some_and(|store| store.pending_count() < self.config.storage.pending_limit)
            && let Some((epoch, record)) = self.storage.overflow.take()
        {
            self.submit_record(epoch, record);
        }
        if self
            .history
            .as_ref()
            .is_some_and(|history| history.loading && history.page_request.is_none())
            && let Err(error) = self.request_history_page(true)
        {
            self.report(error);
            if let Some(history) = &mut self.history {
                history.loading = false;
            }
        }
        self.service_pace();
    }

    pub(super) fn persist_finished(&mut self, pending: &PendingResult) {
        let privacy = Persistence {
            save_results: pending.config.privacy.save_results
                && !pending.config.privacy.private_session,
            store_custom_text: pending.config.privacy.store_custom_text,
            store_event_trace: pending.config.privacy.store_event_trace,
        };
        match Record::prepare(
            pending.result.clone(),
            privacy,
            pending.full_text.as_deref(),
            pending.trace.clone(),
        ) {
            Ok(Some(record)) => {
                self.storage.last_record = Some(record.clone());
                self.submit_record(pending.epoch, record);
            }
            Ok(None) => {
                self.storage.last_record = None;
            }
            Err(error) => {
                self.storage.lost_unsaved = self.storage.lost_unsaved.saturating_add(1);
                self.storage.failure = Some(error.to_string());
                if self.storage.visible_epoch == Some(pending.epoch) {
                    self.report(format!("Unsaved · {error}"));
                }
            }
        }
    }

    fn submit_record(&mut self, epoch: u64, record: Record) {
        let submission = if let Some(store) = &mut self.storage.store {
            store.submit(record)
        } else {
            self.keep_overflow(epoch, record);
            return;
        };
        match submission {
            Ok(ticket) => {
                self.storage.current_ticket = Some((ticket, epoch));
                if self.storage.visible_epoch == Some(epoch) {
                    self.notice = Some("Saving…".into());
                    self.dirty = true;
                }
            }
            Err(rejected) => {
                self.storage.failure = Some(rejected.error.to_string());
                self.keep_overflow(epoch, rejected.record);
                if self.storage.visible_epoch == Some(epoch) {
                    self.report(format!("Unsaved · {}", rejected.error));
                }
            }
        }
    }

    fn keep_overflow(&mut self, epoch: u64, record: Record) {
        if self.storage.overflow.replace((epoch, record)).is_some() {
            self.storage.lost_unsaved = self.storage.lost_unsaved.saturating_add(1);
            self.report("Unsaved capacity exceeded · oldest unqueued result discarded; export pending results before continuing");
        }
    }

    fn storage_event(&mut self, event: Event) {
        match event {
            Event::Ready { .. } => {
                self.storage.failure = None;
            }
            Event::Unavailable(error) => {
                self.storage.failure = Some(error.to_string());
                if self.sample.engine.state() != State::Running {
                    self.report(format!("Storage unavailable · {error}"));
                }
            }
            Event::Saved { ticket, best, .. } => {
                if best == BestOutcome::NewBest {
                    self.storage.pace_cache = None;
                }
                if self.storage.current_ticket.is_some_and(|(id, epoch)| {
                    id == ticket && self.storage.visible_epoch == Some(epoch)
                }) {
                    self.notice = match best {
                        BestOutcome::NewBest => Some("Personal best · saved".into()),
                        BestOutcome::Tied => Some("Personal best tied · saved".into()),
                        _ => None,
                    };
                    // A save acknowledgement is not a reason to redraw active typing.
                    self.dirty |= self.sample.engine.state() != State::Running;
                }
            }
            Event::Unsaved { ticket, error } => {
                self.storage.failure = Some(error.to_string());
                if self.storage.current_ticket.is_some_and(|(id, epoch)| {
                    id == ticket && self.storage.visible_epoch == Some(epoch)
                }) {
                    self.report(format!("Unsaved · {error}"));
                }
            }
            Event::History { request_id, page } => {
                if let Some(history) = &mut self.history
                    && history.page_request == Some(request_id)
                {
                    history.page = page;
                    history.selected = history
                        .selected
                        .min(history.page.results.len().saturating_sub(1));
                    history.loading = false;
                    history.page_request = None;
                    self.dirty = true;
                }
            }
            Event::Stats {
                request_id,
                statistics,
            } => {
                if let Some(history) = &mut self.history
                    && history.stats_request == Some(request_id)
                {
                    history.statistics = Some(statistics);
                    history.stats_request = None;
                    self.dirty = true;
                }
            }
            Event::Review { request_id, review } => {
                if self
                    .history
                    .as_ref()
                    .is_some_and(|history| history.review_request == Some(request_id))
                {
                    if let Some(history) = &mut self.history {
                        history.review_request = None;
                    }
                    if let Some(review) = review {
                        self.storage.stored_review = Some(*review);
                        self.show_stored_review();
                    } else {
                        self.report("That result is no longer available");
                    }
                }
            }
            Event::Best { request_id, best } => {
                if self
                    .storage
                    .pace_request
                    .as_ref()
                    .is_some_and(|(id, _)| *id == request_id)
                {
                    let (_, key) = self
                        .storage
                        .pace_request
                        .take()
                        .expect("matching pace request");
                    self.storage.pace_cache = Some((key, best));
                    self.storage.pace_checked = false;
                }
            }
            Event::QueryFailed { request_id, error } => {
                if self
                    .storage
                    .pace_request
                    .as_ref()
                    .is_some_and(|(id, _)| *id == request_id)
                {
                    self.storage.pace_request = None;
                    self.storage.pace_checked = true;
                    if self.sample.engine.state() == State::Ready {
                        self.report(format!("Pace unavailable · {error}"));
                    }
                }
                if let Some(history) = &mut self.history
                    && (history.page_request == Some(request_id)
                        || history.stats_request == Some(request_id)
                        || history.review_request == Some(request_id))
                {
                    history.loading = false;
                    history.message = Some(error.to_string());
                    self.dirty = true;
                }
            }
        }
    }

    fn service_pace(&mut self) {
        if self.config.practice.pace != "personal_best"
            || self.storage.pace_checked
            || self.sample.engine.spec().mode == crate::engine::Mode::Zen
            || self.sample.engine.spec().repeated
        {
            return;
        }
        let mut spec = self.sample.engine.spec().clone();
        spec.pace_wpm = None;
        let key = spec.profile_key();
        if let Some((cached, best)) = &self.storage.pace_cache
            && *cached == key
        {
            self.storage.pace_checked = true;
            if self.sample.engine.state() != State::Ready {
                return;
            }
            if let Some(best) = best {
                let pace = best.wpm();
                if !(1.0..=1000.0).contains(&pace) {
                    self.report("Matching best is outside the supported pace range (1–1000 WPM)");
                    return;
                }
                spec.pace_wpm = Some(pace);
                match crate::engine::Engine::new(spec, &self.sample.text) {
                    Ok(engine) => {
                        self.sample.engine = engine;
                        self.notice = Some(format!(
                            "Personal-best pace · {pace:.1} wpm · assisted practice"
                        ));
                        self.dirty = true;
                    }
                    Err(error) => self.report(error),
                }
            } else {
                self.report("No matching saved best · this sample has no pace assistance");
            }
        } else if self.storage.pace_request.is_none()
            && let Some(store) = &mut self.storage.store
        {
            match store.request_best(&key) {
                Ok(id) => self.storage.pace_request = Some((id, key)),
                Err(error) => {
                    self.storage.pace_checked = true;
                    self.report(format!("Pace unavailable · {error}"));
                }
            }
        }
    }

    pub(super) fn unsaved_count(&self) -> usize {
        self.storage.store.as_ref().map_or(0, Store::pending_count)
            + usize::from(self.storage.overflow.is_some())
            + self.storage.lost_unsaved
    }
    pub(super) fn visible_notice(&self) -> Option<String> {
        let failed = self.storage.lost_unsaved > 0
            || self.storage.overflow.is_some()
            || self.storage.store.as_ref().is_some_and(|store| {
                store
                    .pending()
                    .any(|(_, _, state)| matches!(state, storage::PendingState::Unsaved(_)))
            });
        if failed {
            let status = format!(
                "{} unsaved · commands: retry / export",
                self.unsaved_count()
            );
            Some(
                self.notice
                    .as_ref()
                    .map_or_else(|| status.clone(), |notice| format!("{notice} · {status}")),
            )
        } else {
            self.notice.clone()
        }
    }

    pub(super) fn retry_saves(&mut self) -> Result<String, String> {
        if self.private_locked || !self.config.privacy.save_results {
            return Err("Result saving is disabled in this session".into());
        }
        let options = self.storage_options()?;
        if let Some(store) = &mut self.storage.store {
            store
                .reconfigure(options)
                .map_err(|error| error.to_string())?;
            store.retry_unsaved();
        }
        self.storage.requested = true;
        self.storage.attempted = false;
        Ok(format!(
            "Retry requested · {} unacknowledged results",
            self.unsaved_count()
        ))
    }

    pub(super) fn flush_storage(&mut self, interrupted: bool) -> usize {
        self.service_storage();
        if let Some(store) = &mut self.storage.store {
            store.flush_for(Duration::from_millis(if interrupted { 100 } else { 750 }));
        }
        let count = self.unsaved_count();
        // Dropping disconnects its reply channel. A stalled fsync may finish later;
        // it has no terminal handle and cannot emit output after restoration.
        self.storage.store.take();
        count
    }

    pub(super) fn trace_event(&mut self, event: &crate::terminal::Envelope) {
        if self.private_locked
            || !self.config.privacy.save_results
            || !self.config.privacy.store_event_trace
            || !self.accepted
            || !self.armed
            || self.palette.is_some()
            || self.details
            || self.panel.is_some()
            || self.history.is_some()
            || !matches!(self.sample.engine.state(), State::Ready | State::Running)
        {
            return;
        }
        let (kind, text) = match &event.kind {
            InputKind::Key {
                key,
                associated_text,
            } => {
                use crate::settings::{BindingContext, CommandAction};
                let context = if self.sample.engine.state() == State::Ready {
                    BindingContext::Ready
                } else {
                    BindingContext::Running
                };
                if let Some(action) = self.bindings.resolve(key, context) {
                    (
                        match action {
                            CommandAction::Finish => TraceKind::Finish,
                            CommandAction::DeleteWord => TraceKind::DeleteWord,
                            CommandAction::Quit => TraceKind::Interrupt,
                            _ => TraceKind::Abort,
                        },
                        None,
                    )
                } else if let Some(text) = associated_text {
                    (TraceKind::Text, Some(text.clone()))
                } else if key.modifiers.intersects(
                    KeyModifiers::CONTROL
                        | KeyModifiers::ALT
                        | KeyModifiers::SUPER
                        | KeyModifiers::META
                        | KeyModifiers::HYPER,
                ) {
                    return;
                } else {
                    match key.code {
                        KeyCode::Char(ch) => (TraceKind::Text, Some(ch.to_string())),
                        KeyCode::Tab if self.sample.engine.spec().policy == Policy::Exact => {
                            (TraceKind::Text, Some("\t".into()))
                        }
                        KeyCode::Enter
                            if self.sample.engine.spec().policy == Policy::Exact
                                || self.sample.engine.spec().mode == crate::engine::Mode::Zen =>
                        {
                            (TraceKind::Text, Some("\n".into()))
                        }
                        KeyCode::Backspace => (TraceKind::Backspace, None),
                        _ => return,
                    }
                }
            }
            InputKind::Paste => (TraceKind::PasteAttempt, None),
            InputKind::FocusLost => (TraceKind::FocusLost, None),
            InputKind::Error(_) => (TraceKind::Interrupt, None),
            _ => return,
        };
        let trace = self.storage.trace.get_or_insert_with(|| EventTrace {
            events: Vec::with_capacity(storage::TRACE_EVENT_LIMIT),
            ..EventTrace::default()
        });
        trace.events_total = trace.events_total.saturating_add(1);
        if trace.events.len() >= storage::TRACE_EVENT_LIMIT {
            trace.truncated = true;
            return;
        }
        let text = text.map(|mut text| {
            if text.len() > storage::TRACE_TEXT_BYTES {
                let mut end = storage::TRACE_TEXT_BYTES;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
                trace.truncated = true;
            }
            text
        });
        trace.events.push(TraceEvent {
            received_us: event.received_us,
            sequence: event.sequence,
            kind,
            text,
        });
    }

    pub(super) fn open_history(&mut self, reader: &Reader, now: u64) -> Result<(), String> {
        if self.palette.is_none() {
            self.open_palette(reader, now)?;
        }
        self.restore_preview();
        let return_ready = self
            .palette
            .take()
            .is_some_and(|palette| palette.return_ready);
        self.panel = None;
        self.details = false;
        self.history = Some(ui::history::History::new(
            self.sample.engine.spec().profile_key(),
            return_ready,
        ));
        self.storage.requested = true;
        self.dirty = true;
        Ok(())
    }
    pub(super) fn close_history(&mut self, reader: &Reader, now: u64) -> Result<(), String> {
        let ready = self
            .history
            .take()
            .is_some_and(|history| history.return_ready);
        self.storage.stored_review = None;
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
    fn request_history_page(&mut self, stats: bool) -> Result<(), String> {
        let Some(store) = &mut self.storage.store else {
            return Ok(());
        };
        let Some(history) = &mut self.history else {
            return Ok(());
        };
        history.page_request = Some(
            store
                .request_history(
                    history.filter.clone(),
                    Page {
                        limit: 20,
                        offset: history.offset,
                    },
                )
                .map_err(|error| error.to_string())?,
        );
        if stats {
            history.stats_request = Some(
                store
                    .request_stats(history.filter.clone())
                    .map_err(|error| error.to_string())?,
            );
        }
        history.loading = true;
        history.message = None;
        Ok(())
    }
    pub(super) fn history_key(&mut self, key: KeyCode) -> Result<(), String> {
        let Some(history) = &mut self.history else {
            return Ok(());
        };
        let previous = (
            history.selected,
            history.offset,
            history.loading,
            history.message.clone(),
        );
        match key {
            KeyCode::Up => history.move_by(-1),
            KeyCode::Down => history.move_by(1),
            KeyCode::Home => history.selected = 0,
            KeyCode::Left | KeyCode::PageUp => {
                history.selected = 0;
                if history.offset > 0 {
                    history.offset = history.offset.saturating_sub(20);
                    self.request_history_page(false)?;
                }
            }
            KeyCode::Right | KeyCode::PageDown => {
                if let Some(next) = history.page.next_offset {
                    history.offset = next;
                    history.selected = 0;
                    self.request_history_page(false)?;
                }
            }
            KeyCode::Enter => {
                if let Some(entry) = history.page.results.get(history.selected)
                    && let Some(store) = &mut self.storage.store
                {
                    history.review_request = Some(
                        store
                            .request_review(&entry.id)
                            .map_err(|error| error.to_string())?,
                    );
                }
            }
            KeyCode::Char('f') => {
                let mut palette = Palette::new(&self.config, false);
                palette.editor = Some(Editor {
                    purpose: EditPurpose::HistoryFilters,
                    value: "{ profile = \"all\" }".into(),
                    choices: [
                        "{ profile = \"all\" }",
                        "{ profile = \"current\" }",
                        "{ profile = \"all\", classification = \"standard\" }",
                        "{ profile = \"all\", classification = \"practice\" }",
                        "{ profile = \"all\", classification = \"paste_attempted\" }",
                        "{ profile = \"all\", classification = \"assisted_code\" }",
                        "{ profile = \"all\", outcome = \"interrupted\" }",
                    ]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                    replace_on_type: true,
                });
                self.palette = Some(palette);
            }
            _ => {}
        }
        self.dirty |= self.palette.is_some()
            || self.history.as_ref().is_some_and(|history| {
                previous
                    != (
                        history.selected,
                        history.offset,
                        history.loading,
                        history.message.clone(),
                    )
            });
        Ok(())
    }
    pub(super) fn history_filters(&mut self, raw: &str) -> Result<String, String> {
        #[derive(serde::Deserialize, Default)]
        #[serde(default, deny_unknown_fields)]
        struct Values {
            profile: Option<String>,
            mode: Option<crate::engine::Mode>,
            language: Option<String>,
            outcome: Option<Outcome>,
            classification: Option<storage::Classification>,
            from: Option<String>,
            to: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct Wrapper {
            filter: Values,
        }
        let values: Wrapper = toml::from_str(&format!("filter = {raw}")).map_err(|_| {
            "Use a TOML table: { profile = \"all\", mode = \"time\", from = \"2026-01-01\" }"
                .to_owned()
        })?;
        let values = values.filter;
        let profile = match values.profile.as_deref().unwrap_or("current") {
            "all" => None,
            "current" => Some(self.sample.engine.spec().profile_key()),
            key => Some(key.to_owned()),
        };
        let filter = Filter {
            profile_key: profile,
            mode: values.mode,
            language: values.language,
            outcome: values.outcome,
            classification: values.classification,
            from_utc_ms: values
                .from
                .as_deref()
                .map(|value| storage::parse_utc_bound(value, false))
                .transpose()
                .map_err(|error| error.to_string())?,
            to_utc_ms: values
                .to
                .as_deref()
                .map(|value| storage::parse_utc_bound(value, true))
                .transpose()
                .map_err(|error| error.to_string())?,
        };
        // Validate by enqueueing before replacing the currently visible selection.
        let store = self
            .storage
            .store
            .as_mut()
            .ok_or("History worker is unavailable")?;
        let request = store
            .request_history(filter.clone(), Page::default())
            .map_err(|error| error.to_string())?;
        let history = self.history.as_mut().ok_or("History is not open")?;
        history.filter = filter.clone();
        history.offset = 0;
        history.selected = 0;
        history.loading = true;
        history.page_request = Some(request);
        history.stats_request = Some(
            store
                .request_stats(filter)
                .map_err(|error| error.to_string())?,
        );
        self.palette = None;
        Ok("History filter applied".into())
    }
    fn show_stored_review(&mut self) {
        let Some(stored) = &self.storage.stored_review else {
            return;
        };
        let replay = replay_review(stored);
        let mut review = ui::review::Review::new(
            replay
                .clone()
                .unwrap_or_else(|| stored.record.snapshot().clone()),
        );
        review.entered_available = replay.is_some() || stored.record.event_trace().is_some();
        review.original_required = matches!(
            stored.content,
            storage::ReviewContent::OriginalRequired { .. }
        );
        if review.result.words.is_empty() {
            let text = stored
                .verified_original
                .clone()
                .or_else(|| stored.record.full_text().map(str::to_owned));
            if text.is_some() {
                review.original_required = false;
            }
            review.set_original_text(text);
        }
        if stored.record.word_summaries_omitted() > 0 {
            review.notice = Some(format!(
                "{} token summaries omitted by retention limit",
                stored.record.word_summaries_omitted()
            ));
        }
        if stored.record.text_scope() == Some(storage::TextScope::RetainedWindow) {
            review.notice =
                Some("Stored Zen output is the retained window; older text was discarded".into());
        }
        self.review = Some(review);
        self.details = true;
        self.palette = None;
        self.dirty = true;
    }
    pub(super) fn original_file(&mut self, path: &str) -> Result<String, String> {
        let file = std::fs::File::open(path).map_err(|_| "Could not open original source file")?;
        let bytes = crate::sample::read_bounded(file, 1024 * 1024)
            .map_err(|_| "Could not read bounded original source file")?;
        self.storage
            .stored_review
            .as_mut()
            .ok_or("No historical custom result is selected")?
            .attach_original(&bytes)
            .map_err(|error| error.to_string())?;
        self.show_stored_review();
        if let Some(review) = &mut self.review {
            review.tab = ui::review::Tab::Text;
        }
        Ok("Original hash verified; entered text is shown only when retained".into())
    }
    pub(super) fn recovery_export(&mut self, path: &str) -> Result<String, String> {
        if self.private_locked {
            return Err("Private mode does not export session results".into());
        }
        let mut records: Vec<&Record> =
            self.storage.store.as_ref().map_or_else(Vec::new, |store| {
                store.pending().map(|(_, record, _)| record).collect()
            });
        if let Some((_, record)) = &self.storage.overflow {
            records.push(record);
        }
        if let Some(record) = &self.storage.last_record
            && !records.iter().any(|item| item.id() == record.id())
        {
            records.push(record);
        }
        if records.is_empty() {
            return Err("No retained results are available to export".into());
        }
        let target = Path::new(path);
        #[cfg(windows)]
        let created = clack_private_fs::create_new_file(target);
        #[cfg(not(windows))]
        let created = {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(target)
        };
        let file = created.map_err(|error| format!("Cannot create new recovery file: {error}"))?;
        let mut writer = BufWriter::new(file);
        let result = storage::export_records(
            &mut writer,
            storage::ExportFormat::Jsonl,
            records.iter().copied(),
            false,
        )
        .map_err(|error| error.to_string())
        .and_then(|()| writer.flush().map_err(|error| error.to_string()))
        .and_then(|()| {
            writer
                .get_ref()
                .sync_all()
                .map_err(|error| error.to_string())
        });
        if let Err(error) = result {
            drop(writer);
            let _ = std::fs::remove_file(target);
            return Err(format!("Recovery export failed: {error}"));
        }
        Ok(format!(
            "Exported {} retained results{}",
            records.len(),
            if self.storage.lost_unsaved > 0 {
                "; older capacity-discarded results are unavailable"
            } else {
                ""
            }
        ))
    }
}

// An explicitly retained complete diagnostic trace may reconstruct review with
// the same engine. A partial trace or any score mismatch never invents a diff.
fn replay_review(stored: &storage::Review) -> Option<ResultSnapshot> {
    let trace = stored.record.event_trace()?;
    if trace.truncated {
        return None;
    }
    let target = stored
        .verified_original
        .as_deref()
        .or_else(|| stored.record.full_text())?;
    let saved = stored.record.snapshot();
    if saved.spec.mode == crate::engine::Mode::Zen {
        return None;
    }
    let mut engine = crate::engine::Engine::new(saved.spec.clone(), target).ok()?;
    for event in &trace.events {
        let action = match event.kind {
            TraceKind::Text => Action::Text(event.text.as_deref()?),
            TraceKind::Backspace => Action::Backspace,
            TraceKind::DeleteWord => Action::DeleteWord,
            TraceKind::Finish => Action::Finish,
            TraceKind::PasteAttempt => Action::Paste,
            TraceKind::FocusLost => Action::FocusLost,
            TraceKind::Abort => Action::Abort,
            TraceKind::Interrupt => Action::Interrupt("recorded interruption"),
            TraceKind::Overload => Action::Overload,
        };
        engine.apply(action, event.received_us);
    }
    if engine.state() != State::Results {
        let end = engine.started_at()?.checked_add(saved.elapsed_us)?;
        engine.apply(
            match saved.outcome {
                Outcome::Interrupted => Action::Interrupt("recorded interruption"),
                Outcome::Aborted => Action::Abort,
                _ => Action::Tick,
            },
            end,
        );
    }
    let mut replay = engine.snapshot();
    if replay.counts != saved.counts
        || replay.elapsed_us != saved.elapsed_us
        || replay.outcome != saved.outcome
    {
        return None;
    }
    replay.integrity = saved.integrity.clone();
    replay.reason = saved.reason.clone();
    replay.personal_best_eligible = saved.personal_best_eligible;
    Some(replay)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stored_trace(truncated: bool, omit_correction: bool) -> storage::Review {
        let source = "cat dog";
        let spec = crate::engine::TestSpec {
            mode: crate::engine::Mode::Custom,
            source_id: "custom".into(),
            content_hash: crate::content::content_hash(source.as_bytes()),
            approved_content: false,
            ..crate::engine::TestSpec::default()
        };
        let mut engine = crate::engine::Engine::new(spec, source).unwrap();
        let mut trace = EventTrace::default();
        for (sequence, (time, kind, text)) in [
            (1_000_000, TraceKind::Text, Some("cax")),
            (1_200_000, TraceKind::Backspace, None),
            (1_300_000, TraceKind::Text, Some("t ")),
            (2_000_000, TraceKind::Text, Some("dog")),
        ]
        .into_iter()
        .enumerate()
        {
            engine.apply(
                match kind {
                    TraceKind::Backspace => Action::Backspace,
                    _ => Action::Text(text.unwrap()),
                },
                time,
            );
            if !omit_correction || sequence != 1 {
                trace.events.push(TraceEvent {
                    sequence: sequence as u64,
                    received_us: time,
                    kind,
                    text: text.map(str::to_owned),
                });
            }
        }
        trace.events_total = trace.events.len() as u64;
        trace.truncated = truncated;
        let record = Record::prepare(
            engine.snapshot(),
            Persistence {
                save_results: true,
                store_custom_text: false,
                store_event_trace: true,
            },
            None,
            Some(trace),
        )
        .unwrap()
        .unwrap();
        storage::Review {
            record,
            content: storage::ReviewContent::OriginalVerified,
            verified_original: Some(source.into()),
        }
    }
    #[test]
    fn retained_trace_and_verified_original_replay_through_the_real_engine() {
        let stored = stored_trace(false, false);
        assert!(stored.record.snapshot().words.is_empty());
        let replay = replay_review(&stored).unwrap();
        assert_eq!(replay.counts, stored.record.snapshot().counts);
        assert_eq!(replay.words[0].entered, "cat");
        assert!(replay.words[0].corrected);
        assert_eq!(replay.words[0].errors, 1);
    }
    #[test]
    fn partial_or_inconsistent_trace_never_invents_a_historical_diff() {
        assert!(replay_review(&stored_trace(true, false)).is_none());
        assert!(replay_review(&stored_trace(false, true)).is_none());
        let mut stored = stored_trace(false, false);
        stored.verified_original = None;
        assert!(replay_review(&stored).is_none());
    }
}
