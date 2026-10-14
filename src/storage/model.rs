use crate::{
    content,
    engine::{Counts, Mode, Outcome, ResultSnapshot},
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use unicode_segmentation::UnicodeSegmentation;

pub const SQL_SCHEMA_VERSION: u32 = 1;
pub const EXPORT_VERSION: u32 = 1;
pub const TRACE_EVENT_LIMIT: usize = 8192;
pub const TRACE_TEXT_BYTES: usize = 128;
pub const WORD_SUMMARY_LIMIT: usize = 65_536;
pub const WORD_DIAGNOSTIC_JSON_BYTES: usize = 24 * 1024 * 1024;
pub(crate) const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_WORD_JSON_BYTES: usize = 4 * content::MAX_CANONICAL_CUSTOM_BYTES + 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Invalid,
    Busy,
    ReadOnly,
    Full,
    Corrupt,
    UnsupportedSchema,
    Io,
    Unavailable,
    Capacity,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageError {
    pub kind: ErrorKind,
    pub message: String,
}
impl StorageError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub(crate) fn io(_: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, "local history storage I/O failed")
    }
    pub(crate) fn sql(error: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode;
        let kind = match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => ErrorKind::Busy,
            Some(ErrorCode::ReadOnly) => ErrorKind::ReadOnly,
            Some(ErrorCode::DiskFull) => ErrorKind::Full,
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase | ErrorCode::TooBig) => {
                ErrorKind::Corrupt
            }
            _ => ErrorKind::Io,
        };
        Self::new(
            kind,
            match kind {
                ErrorKind::Busy => "history database is busy; retry when it is available",
                ErrorKind::ReadOnly => "history database is read-only",
                ErrorKind::Full => "history storage is full",
                ErrorKind::Corrupt => {
                    "history database is corrupt or not a SQLite database; it was not repaired or replaced"
                }
                _ => "history database operation failed",
            },
        )
    }
}
impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for StorageError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JournalMode {
    #[default]
    Auto,
    Wal,
    Delete,
}
impl JournalMode {
    pub fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "auto" => Ok(Self::Auto),
            "wal" => Ok(Self::Wal),
            "delete" => Ok(Self::Delete),
            _ => Err(StorageError::new(
                ErrorKind::Invalid,
                "journal mode expects auto, wal, or delete",
            )),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Options {
    pub database: PathBuf,
    pub read_only: bool,
    pub journal: JournalMode,
    pub pending_limit: usize,
    pub busy_budget: Duration,
}
impl Options {
    pub fn new(database: PathBuf) -> Self {
        Self {
            database,
            read_only: false,
            journal: JournalMode::Auto,
            pending_limit: 16,
            busy_budget: Duration::from_millis(250),
        }
    }
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        if !(1..=64).contains(&self.pending_limit) || self.busy_budget > Duration::from_millis(250)
        {
            Err(StorageError::new(
                ErrorKind::Invalid,
                "storage pending limit expects 1–64 and busy budget at most 250 ms",
            ))
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Persistence {
    pub save_results: bool,
    pub store_custom_text: bool,
    pub store_event_trace: bool,
}
impl Default for Persistence {
    fn default() -> Self {
        Self {
            save_results: true,
            store_custom_text: false,
            store_event_trace: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceKind {
    Text,
    Backspace,
    DeleteWord,
    Finish,
    PasteAttempt,
    FocusLost,
    Abort,
    Interrupt,
    Overload,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceEvent {
    pub received_us: u64,
    pub sequence: u64,
    pub kind: TraceKind,
    pub text: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventTrace {
    pub events: Vec<TraceEvent>,
    pub events_total: u64,
    pub truncated: bool,
}

/// A stored body is never assumed to be the entire transcript merely because
/// the caller opted in. Zen discards old output after its editable window fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextScope {
    CompleteTarget,
    PreparedTarget,
    CompleteOutput,
    RetainedWindow,
}
impl TextScope {
    fn for_text(snapshot: &ResultSnapshot, text: &str) -> Self {
        match snapshot.spec.mode {
            Mode::Zen if snapshot.counts.retained_units > text.graphemes(true).count() as u64 => {
                Self::RetainedWindow
            }
            Mode::Zen => Self::CompleteOutput,
            Mode::Custom | Mode::Code | Mode::Quote => Self::CompleteTarget,
            Mode::Time | Mode::Words => Self::PreparedTarget,
        }
    }
}

/// Immutable after preparation. Accessors expose shared references only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub(crate) record_version: u32,
    pub(crate) id: String,
    pub(crate) created_at_utc_ms: i64,
    pub(crate) snapshot: ResultSnapshot,
    pub(crate) full_text: Option<String>,
    pub(crate) text_scope: Option<TextScope>,
    pub(crate) event_trace: Option<EventTrace>,
    pub(crate) word_summaries_omitted: u64,
}

/// Privacy-filtered result ownership for deferred preparation. Diagnostic
/// budgeting, serialization checks, and validation run once on the storage
/// worker. An explicit recovery export can complete the same preparation.
#[derive(Debug)]
pub struct Preparation {
    draft: Mutex<Option<Record>>,
    prepared: OnceLock<Result<Arc<Record>, StorageError>>,
}
impl Preparation {
    fn new(mut record: Record) -> Result<Self, StorageError> {
        let invalid = || {
            StorageError::new(
                ErrorKind::Invalid,
                "result draft exceeds bounded pending storage capacity",
            )
        };
        // Admission uses counts and byte lengths only. Display, score, hash and
        // JSON validation still belong to the worker. Optional diagnostics must
        // not retain an arbitrarily large unfinished record behind a stalled DB.
        let mut fixed_bytes = std::mem::size_of::<Record>();
        for value in [
            record.id.as_str(),
            record.snapshot.app_version.as_str(),
            record.snapshot.profile_key.as_str(),
            record.snapshot.spec.source_id.as_str(),
            record.snapshot.spec.source_revision.as_str(),
            record.snapshot.spec.content_hash.as_str(),
            record.snapshot.reason.as_deref().unwrap_or_default(),
            record
                .snapshot
                .spec
                .practice_reason
                .as_deref()
                .unwrap_or_default(),
        ] {
            if value.len() > 256 {
                return Err(invalid());
            }
            fixed_bytes = fixed_bytes.saturating_add(value.len());
        }
        if record.snapshot.samples.len() > crate::engine::SAMPLE_CAPACITY {
            return Err(invalid());
        }
        record.snapshot.samples.shrink_to_fit();
        fixed_bytes = fixed_bytes.saturating_add(
            record
                .snapshot
                .samples
                .len()
                .saturating_mul(std::mem::size_of::<crate::engine::Sample>()),
        );
        if let Some(text) = &record.full_text {
            if text.len() > content::MAX_CANONICAL_CUSTOM_BYTES {
                return Err(invalid());
            }
            fixed_bytes = fixed_bytes.saturating_add(text.len());
        }
        if let Some(trace) = &mut record.event_trace {
            if trace.events.len() > TRACE_EVENT_LIMIT {
                return Err(invalid());
            }
            trace.events.shrink_to_fit();
            fixed_bytes = fixed_bytes.saturating_add(
                trace
                    .events
                    .len()
                    .saturating_mul(std::mem::size_of::<TraceEvent>()),
            );
            for event in &trace.events {
                let bytes = event.text.as_ref().map_or(0, String::len);
                if bytes > TRACE_TEXT_BYTES {
                    return Err(invalid());
                }
                fixed_bytes = fixed_bytes.saturating_add(bytes);
            }
        }
        if fixed_bytes > MAX_RECORD_BYTES {
            return Err(invalid());
        }
        let word_budget = WORD_DIAGNOSTIC_JSON_BYTES.min(MAX_RECORD_BYTES - fixed_bytes);
        let original_words = record.snapshot.words.len();
        let mut diagnostic_bytes = 0usize;
        let mut retained_words = 0;
        for word in record.snapshot.words.iter().take(WORD_SUMMARY_LIMIT) {
            if word.token.len() > content::MAX_CANONICAL_CUSTOM_BYTES
                || word.entered.len() > content::MAX_CANONICAL_CUSTOM_BYTES
            {
                break;
            }
            let bytes = std::mem::size_of::<crate::engine::WordSummary>()
                .saturating_add(word.token.len())
                .saturating_add(word.entered.len());
            if bytes > word_budget.saturating_sub(diagnostic_bytes) {
                break;
            }
            diagnostic_bytes += bytes;
            retained_words += 1;
        }
        record.word_summaries_omitted = original_words.saturating_sub(retained_words) as u64;
        record.snapshot.words.truncate(retained_words);
        if record.snapshot.words.capacity() != retained_words {
            record.snapshot.words.shrink_to_fit();
        }
        Ok(Self {
            draft: Mutex::new(Some(record)),
            prepared: OnceLock::new(),
        })
    }
    /// Completes preparation once, retaining the same immutable record and ID
    /// for retries and recovery. This is a worker or explicit-export operation.
    pub fn prepare(&self) -> Result<Arc<Record>, StorageError> {
        self.prepared
            .get_or_init(|| {
                let record = self
                    .draft
                    .lock()
                    .map_err(|_| {
                        StorageError::new(ErrorKind::Unavailable, "result preparation stopped")
                    })?
                    .take()
                    .ok_or_else(|| {
                        StorageError::new(ErrorKind::Unavailable, "result preparation stopped")
                    })?;
                record.finish_preparation().map(Arc::new)
            })
            .clone()
    }
    /// Returns only completed, validated records without waiting for work.
    pub fn prepared_record(&self) -> Option<&Record> {
        self.prepared_arc().map(AsRef::as_ref)
    }
    pub(crate) fn prepared_arc(&self) -> Option<&Arc<Record>> {
        self.prepared.get()?.as_ref().ok()
    }
}

impl Record {
    pub fn prepare(
        snapshot: ResultSnapshot,
        privacy: Persistence,
        full_target: Option<&str>,
        trace: Option<EventTrace>,
    ) -> Result<Option<Self>, StorageError> {
        if !privacy.save_results {
            return Ok(None);
        }
        Self::prepare_at(snapshot, privacy, full_target, trace, utc_now()?)
    }
    /// Applies the privacy boundary before enqueueing. Expensive diagnostic
    /// retention and validation are deferred until the worker consumes it.
    pub fn prepare_async(
        snapshot: ResultSnapshot,
        privacy: Persistence,
        full_target: Option<String>,
        trace: Option<EventTrace>,
    ) -> Result<Option<Preparation>, StorageError> {
        if !privacy.save_results {
            return Ok(None);
        }
        Self::draft_at(snapshot, privacy, full_target, trace, utc_now()?)
            .map(Preparation::new)
            .transpose()
    }
    pub fn prepare_at(
        snapshot: ResultSnapshot,
        privacy: Persistence,
        full_target: Option<&str>,
        trace: Option<EventTrace>,
        created_at_utc_ms: i64,
    ) -> Result<Option<Self>, StorageError> {
        if !privacy.save_results {
            return Ok(None);
        }
        Self::draft_at(
            snapshot,
            privacy,
            if privacy.store_custom_text {
                full_target.map(str::to_owned)
            } else {
                None
            },
            trace,
            created_at_utc_ms,
        )
        .map(Self::finish_preparation)
        .transpose()
    }
    fn draft_at(
        mut snapshot: ResultSnapshot,
        privacy: Persistence,
        full_target: Option<String>,
        trace: Option<EventTrace>,
        created_at_utc_ms: i64,
    ) -> Option<Self> {
        if !privacy.save_results {
            return None;
        }
        if matches!(snapshot.spec.mode, Mode::Custom | Mode::Code | Mode::Zen)
            && !snapshot.spec.approved_content
        {
            snapshot.spec.source_id = match snapshot.spec.mode {
                Mode::Code => "code",
                Mode::Zen => "zen",
                _ => "custom",
            }
            .into();
            snapshot.spec.source_revision = "1".into();
        }
        snapshot.profile_key = snapshot.spec.profile_key();
        snapshot.personal_best_eligible &= snapshot.eligible_for_standard_best();
        if !snapshot.spec.approved_content && !privacy.store_custom_text {
            snapshot.words.clear();
        }
        if !privacy.store_event_trace {
            for word in &mut snapshot.words {
                word.entered.clear();
            }
        }
        let full_text = if privacy.store_custom_text {
            full_target
        } else {
            None
        };
        let event_trace = if privacy.store_event_trace {
            trace
        } else {
            None
        };
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let nonce = format!(
            "{}:{nanos}:{}:{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            snapshot.profile_key
        );
        Some(Self {
            record_version: EXPORT_VERSION,
            id: content::content_hash(nonce.as_bytes()),
            created_at_utc_ms,
            snapshot,
            full_text,
            text_scope: None,
            event_trace,
            word_summaries_omitted: 0,
        })
    }
    fn finish_preparation(mut self) -> Result<Self, StorageError> {
        self.text_scope = self
            .full_text
            .as_deref()
            .map(|text| TextScope::for_text(&self.snapshot, text));
        let mut words = std::mem::take(&mut self.snapshot.words);
        let original_words = words.len();
        // Optional diagnostics must not make an otherwise valid summary
        // unsavable. Reserve space for the largest possible omission count,
        // then retain a prefix within both the diagnostic and whole-record
        // budgets. The public omission count makes this truncation explicit.
        let fixed_bytes = bounded_json_len(&self, MAX_RECORD_BYTES)?;
        let word_budget = WORD_DIAGNOSTIC_JSON_BYTES.min(
            MAX_RECORD_BYTES
                .saturating_sub(fixed_bytes)
                .saturating_sub(20),
        );
        let mut diagnostic_bytes = 0;
        let mut retained_words = 0;
        for word in words.iter().take(WORD_SUMMARY_LIMIT) {
            if word.token.len() > content::MAX_CANONICAL_CUSTOM_BYTES
                || word.entered.len() > content::MAX_CANONICAL_CUSTOM_BYTES
            {
                break;
            }
            let Ok(size) = bounded_json_len(word, MAX_WORD_JSON_BYTES) else {
                break;
            };
            // One comma per element conservatively includes array separators.
            if diagnostic_bytes + size + 1 > word_budget {
                break;
            }
            diagnostic_bytes += size + 1;
            retained_words += 1;
        }
        self.word_summaries_omitted = self
            .word_summaries_omitted
            .saturating_add(original_words.saturating_sub(retained_words) as u64);
        words.truncate(retained_words);
        self.snapshot.words = words;
        self.validate()?;
        Ok(self)
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    pub fn snapshot(&self) -> &ResultSnapshot {
        &self.snapshot
    }
    pub fn full_text(&self) -> Option<&str> {
        self.full_text.as_deref()
    }
    pub fn text_scope(&self) -> Option<TextScope> {
        self.text_scope
    }
    pub fn event_trace(&self) -> Option<&EventTrace> {
        self.event_trace.as_ref()
    }
    pub fn word_summaries_omitted(&self) -> u64 {
        self.word_summaries_omitted
    }
    pub fn export_view(&self, include_text: bool) -> Self {
        let mut result = self.clone();
        if !include_text {
            result.full_text = None;
            result.text_scope = None;
            result.event_trace = None;
            if !result.snapshot.spec.approved_content {
                result.snapshot.words.clear();
            }
            for word in &mut result.snapshot.words {
                word.entered.clear();
            }
        }
        result
    }
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        let invalid = || {
            StorageError::new(
                ErrorKind::Invalid,
                "result metadata or diagnostic content exceeds validated storage bounds",
            )
        };
        if self.record_version != EXPORT_VERSION
            || self.id.len() != 64
            || !self.id.bytes().all(|b| b.is_ascii_hexdigit())
            || self.created_at_utc_ms < -62_135_596_800_000
            || self.created_at_utc_ms > 253_402_300_799_999
        {
            return Err(invalid());
        }
        self.snapshot.spec.validate().map_err(|_| invalid())?;
        if self.snapshot.profile_key != self.snapshot.spec.profile_key()
            || self.snapshot.samples.len() > crate::engine::SAMPLE_CAPACITY
            || self.snapshot.words.len() > WORD_SUMMARY_LIMIT
        {
            return Err(invalid());
        }
        for value in [
            &self.snapshot.app_version,
            &self.snapshot.spec.source_id,
            &self.snapshot.spec.source_revision,
            &self.snapshot.spec.content_hash,
        ] {
            if value.len() > 256
                || value.contains(['\n', '\r', '\t'])
                || content::validate_text(value, content::InputPolicy::Prose).is_err()
            {
                return Err(invalid());
            }
        }
        for value in [
            self.snapshot.reason.as_deref(),
            self.snapshot.spec.practice_reason.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.len() > 256
                || content::validate_text(value, content::InputPolicy::Prose).is_err()
            {
                return Err(invalid());
            }
        }
        if !finite_metrics(&self.snapshot.metrics)
            || self
                .snapshot
                .samples
                .iter()
                .any(|sample| !finite_metrics(&sample.metrics))
        {
            return Err(invalid());
        }
        if self.snapshot.counts.attempts_correct > self.snapshot.counts.attempts_total {
            return Err(invalid());
        }
        if let Some(text) = &self.full_text {
            let text_valid = if self.snapshot.spec.mode == Mode::Zen {
                valid_entered_text(text)
            } else {
                content::validate_text(text, content::InputPolicy::Exact).is_ok()
            };
            if text.len() > content::MAX_CANONICAL_CUSTOM_BYTES
                || !text_valid
                || self.text_scope != Some(TextScope::for_text(&self.snapshot, text))
                || (matches!(self.snapshot.spec.mode, Mode::Custom | Mode::Code)
                    && content::content_hash(text.as_bytes()) != self.snapshot.spec.content_hash)
            {
                return Err(invalid());
            }
        } else if self.text_scope.is_some() {
            return Err(invalid());
        }
        if let Some(trace) = &self.event_trace {
            if trace.events.len() > TRACE_EVENT_LIMIT
                || trace.events_total < trace.events.len() as u64
                || (trace.events_total > trace.events.len() as u64 && !trace.truncated)
            {
                return Err(invalid());
            }
            let mut previous = None;
            for event in &trace.events {
                if previous.is_some_and(|(sequence, time)| {
                    event.sequence <= sequence || event.received_us < time
                }) {
                    return Err(invalid());
                }
                if let Some(text) = &event.text
                    && (!matches!(event.kind, TraceKind::Text)
                        || text.len() > TRACE_TEXT_BYTES
                        || text.chars().any(|c| {
                            !matches!(c, '\t' | '\n') && content::validate_input_scalar(c).is_err()
                        }))
                {
                    return Err(invalid());
                }
                previous = Some((event.sequence, event.received_us));
            }
        }
        let mut diagnostic_bytes = 0;
        for word in &self.snapshot.words {
            if word.token.len() > content::MAX_CANONICAL_CUSTOM_BYTES
                || word.entered.len() > content::MAX_CANONICAL_CUSTOM_BYTES
                || content::validate_text(&word.token, content::InputPolicy::Exact).is_err()
                || !valid_entered_text(&word.entered)
            {
                return Err(invalid());
            }
            diagnostic_bytes += bounded_json_len(word, MAX_WORD_JSON_BYTES)?;
        }
        if diagnostic_bytes > WORD_DIAGNOSTIC_JSON_BYTES {
            return Err(invalid());
        }
        bounded_json_len(self, MAX_RECORD_BYTES)?;
        Ok(())
    }
}
fn utc_now() -> Result<i64, StorageError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            StorageError::new(
                ErrorKind::Invalid,
                "system UTC time precedes the Unix epoch",
            )
        })?
        .as_millis();
    i64::try_from(millis).map_err(|_| {
        StorageError::new(
            ErrorKind::Invalid,
            "system UTC time is outside the supported range",
        )
    })
}

fn valid_entered_text(text: &str) -> bool {
    text.chars()
        .all(|character| content::validate_input_scalar(character).is_ok())
        && text
            .graphemes(true)
            .all(|grapheme| grapheme.chars().count() <= content::MAX_GRAPHEME_SCALARS)
}
pub(crate) fn bounded_json_len(
    value: &impl Serialize,
    limit: usize,
) -> Result<usize, StorageError> {
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes) {
                return Err(std::io::Error::other("serialized record exceeds its bound"));
            }
            self.bytes += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { bytes: 0, limit };
    serde_json::to_writer(&mut counter, value).map_err(|_| {
        StorageError::new(
            ErrorKind::Invalid,
            "result serialization exceeds validated storage bounds",
        )
    })?;
    Ok(counter.bytes)
}
fn finite_metrics(metrics: &crate::engine::Metrics) -> bool {
    [
        metrics.wpm,
        metrics.raw_wpm,
        metrics.accuracy,
        metrics.cpm,
        metrics.consistency,
    ]
    .into_iter()
    .flatten()
    .all(f64::is_finite)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filter {
    pub profile_key: Option<String>,
    pub mode: Option<Mode>,
    pub language: Option<String>,
    pub outcome: Option<Outcome>,
    pub classification: Option<Classification>,
    pub from_utc_ms: Option<i64>,
    pub to_utc_ms: Option<i64>,
}

/// Classification is independent of how a run ended. Practice flags may overlap
/// a failed/interrupted outcome; an unassisted failure is not itself a practice
/// reason. Zero-duration unassisted completions have no standard record and no
/// invented practice reason, and remain available without this optional filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Standard,
    Practice,
    PasteAttempted,
    AssistedCode,
}
impl Classification {
    pub(crate) fn predicate(self) -> &'static str {
        match self {
            Self::Standard => "eligible=1",
            Self::Practice => {
                "(outcome='incomplete' OR mode='zen' OR json_extract(header_json,'$.spec.explicit_seed')=1 OR json_extract(header_json,'$.spec.repeated')=1 OR json_type(header_json,'$.spec.practice_reason')='text' OR json_extract(header_json,'$.spec.pace_wpm') IS NOT NULL OR json_extract(header_json,'$.spec.auto_indent')=1 OR json_extract(header_json,'$.integrity.paste_attempted')=1)"
            }
            Self::PasteAttempted => "json_extract(header_json,'$.integrity.paste_attempted')=1",
            Self::AssistedCode => "json_extract(header_json,'$.spec.auto_indent')=1",
        }
    }
}
impl Filter {
    pub fn current(profile_key: impl Into<String>) -> Self {
        Self {
            profile_key: Some(profile_key.into()),
            ..Self::default()
        }
    }
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        if self
            .profile_key
            .as_ref()
            .is_some_and(|key| key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()))
            || self.language.as_ref().is_some_and(|id| {
                id.len() > 128
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            })
            || self
                .from_utc_ms
                .zip(self.to_utc_ms)
                .is_some_and(|(a, b)| a > b)
        {
            return Err(StorageError::new(
                ErrorKind::Invalid,
                "invalid history profile, language or date range",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub limit: u32,
    pub offset: u64,
}
impl Default for Page {
    fn default() -> Self {
        Self {
            limit: 20,
            offset: 0,
        }
    }
}
impl Page {
    pub(crate) fn validate(self) -> Result<(), StorageError> {
        if !(1..=1000).contains(&self.limit) || self.offset > i64::MAX as u64 {
            Err(StorageError::new(
                ErrorKind::Invalid,
                "history page expects limit 1–1000 and a valid offset",
            ))
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub created_at_utc_ms: i64,
    pub created_at_utc: String,
    pub snapshot: ResultSnapshot,
    pub sparkline: Vec<Option<f64>>,
    pub word_summaries_omitted: u64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryPage {
    pub results: Vec<HistoryEntry>,
    pub next_offset: Option<u64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Statistics {
    pub result_count: u64,
    pub speed_sample_count: u64,
    pub target_speed_sample_count: u64,
    pub elapsed_us: u64,
    pub target_elapsed_us: u64,
    pub credited_units: u64,
    pub retained_units: u64,
    pub attempts_total: u64,
    pub attempts_correct: u64,
    pub target_attempts_total: u64,
    pub target_attempts_correct: u64,
    pub aggregate_wpm: Option<f64>,
    pub aggregate_raw_wpm: Option<f64>,
    pub aggregate_accuracy: Option<f64>,
}
impl Statistics {
    pub(crate) fn finish(&mut self) {
        self.aggregate_wpm = (self.target_elapsed_us > 0)
            .then(|| 12_000_000.0 * self.credited_units as f64 / self.target_elapsed_us as f64);
        self.aggregate_raw_wpm = (self.elapsed_us > 0)
            .then(|| 12_000_000.0 * self.retained_units as f64 / self.elapsed_us as f64);
        self.aggregate_accuracy = (self.target_attempts_total > 0).then(|| {
            100.0 * self.target_attempts_correct as f64 / self.target_attempts_total as f64
        });
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Best {
    pub result_id: String,
    pub credited_units: u64,
    pub elapsed_us: u64,
}
impl Best {
    pub fn wpm(&self) -> f64 {
        12_000_000.0 * self.credited_units as f64 / self.elapsed_us as f64
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BestOutcome {
    NewBest,
    NotBest,
    Tied,
    Ineligible,
    AlreadySaved,
}
pub(crate) fn ratio_cmp(a: u64, b: u64, c: u64, d: u64) -> std::cmp::Ordering {
    (u128::from(a) * u128::from(d)).cmp(&(u128::from(c) * u128::from(b)))
}
pub(crate) fn validate_result_id(id: &str) -> Result<(), StorageError> {
    if id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(StorageError::new(ErrorKind::Invalid, "invalid result ID"))
    }
}
pub(crate) fn count_values(counts: &Counts) -> [u64; 9] {
    [
        counts.credited_units,
        counts.retained_units,
        counts.attempts_total,
        counts.attempts_correct,
        counts.deletion_count,
        counts.final_correct,
        counts.final_incorrect,
        counts.final_extra,
        counts.final_missed,
    ]
}

#[cfg(test)]
mod preparation_tests {
    use super::*;
    use crate::engine::{Engine, Metrics, Sample, TestSpec, WordSummary};

    fn draft() -> Record {
        Record::draft_at(
            Engine::new(TestSpec::default(), "cat").unwrap().snapshot(),
            Persistence {
                store_event_trace: true,
                ..Persistence::default()
            },
            None,
            None,
            0,
        )
        .unwrap()
    }

    #[test]
    fn draft_admission_rejects_oversized_fixed_data_before_worker_validation() {
        let mut record = draft();
        record.snapshot.app_version = "a".repeat(257);
        assert_eq!(
            Preparation::new(record).unwrap_err().kind,
            ErrorKind::Invalid
        );

        let mut record = draft();
        record.snapshot.reason = Some("a".repeat(257));
        assert!(Preparation::new(record).is_err());

        let mut record = draft();
        record.full_text = Some("a".repeat(content::MAX_CANONICAL_CUSTOM_BYTES + 1));
        assert!(Preparation::new(record).is_err());

        let mut record = draft();
        record.snapshot.samples = vec![
            Sample {
                bucket_index: 0,
                duration_us: 1,
                counts: Counts::default(),
                attempts: 0,
                errors: 0,
                metrics: Metrics::default(),
            };
            crate::engine::SAMPLE_CAPACITY + 1
        ];
        assert!(Preparation::new(record).is_err());

        let event = TraceEvent {
            received_us: 0,
            sequence: 0,
            kind: TraceKind::Text,
            text: Some("a".repeat(TRACE_TEXT_BYTES)),
        };
        let mut record = draft();
        record.event_trace = Some(EventTrace {
            events: vec![event.clone(); TRACE_EVENT_LIMIT + 1],
            events_total: TRACE_EVENT_LIMIT as u64 + 1,
            truncated: false,
        });
        assert!(Preparation::new(record).is_err());

        let mut record = draft();
        record.event_trace = Some(EventTrace {
            events: vec![TraceEvent {
                text: Some("a".repeat(TRACE_TEXT_BYTES + 1)),
                ..event
            }],
            events_total: 1,
            truncated: false,
        });
        assert!(Preparation::new(record).is_err());
    }

    #[test]
    fn admitted_word_diagnostics_have_a_bounded_prefix_and_conserve_omissions() {
        let mut record = draft();
        let original_words = 4_000;
        record.snapshot.words = vec![
            WordSummary {
                token: "\"".repeat(4_096),
                entered: "\"".repeat(4_096),
                expected_units: 4_096,
                ..WordSummary::default()
            };
            original_words
        ];
        let preparation = Preparation::new(record).unwrap();
        let initially_retained;
        {
            let draft = preparation.draft.lock().unwrap();
            let draft = draft.as_ref().unwrap();
            initially_retained = draft.snapshot.words.len();
            assert!(initially_retained < original_words);
            assert!(draft.snapshot.words.capacity() <= WORD_SUMMARY_LIMIT);
            let raw_bytes: usize = draft
                .snapshot
                .words
                .iter()
                .map(|word| {
                    std::mem::size_of::<WordSummary>() + word.token.len() + word.entered.len()
                })
                .sum();
            assert!(raw_bytes <= WORD_DIAGNOSTIC_JSON_BYTES);
            assert_eq!(
                draft.word_summaries_omitted as usize + initially_retained,
                original_words
            );
        }
        let prepared = preparation.prepare().unwrap();
        assert!(
            prepared.snapshot.words.len() < initially_retained,
            "worker must also enforce JSON escaping costs"
        );
        assert_eq!(
            prepared.word_summaries_omitted as usize + prepared.snapshot.words.len(),
            original_words
        );
        assert!(Arc::ptr_eq(&prepared, &preparation.prepare().unwrap()));
    }

    #[test]
    fn word_count_and_unused_diagnostic_capacity_are_trimmed_before_enqueue() {
        let mut record = draft();
        let original_words = WORD_SUMMARY_LIMIT + 7;
        record.snapshot.words = vec![
            WordSummary {
                token: "a".into(),
                ..WordSummary::default()
            };
            original_words
        ];
        let preparation = Preparation::new(record).unwrap();
        let retained = preparation.draft.lock().unwrap();
        let retained = retained.as_ref().unwrap();
        assert_eq!(retained.snapshot.words.len(), WORD_SUMMARY_LIMIT);
        assert_eq!(retained.word_summaries_omitted, 7);
        assert!(retained.snapshot.words.capacity() <= WORD_SUMMARY_LIMIT);

        let mut record = draft();
        record.snapshot.words.reserve(WORD_SUMMARY_LIMIT / 2);
        let preparation = Preparation::new(record).unwrap();
        let draft = preparation.draft.lock().unwrap();
        assert_eq!(draft.as_ref().unwrap().snapshot.words.capacity(), 0);
    }
}
