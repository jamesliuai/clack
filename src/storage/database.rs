use super::{
    Best, BestOutcome, Classification, ErrorKind, Filter, HistoryEntry, HistoryPage, JournalMode,
    Options, Page, Record, SQL_SCHEMA_VERSION, Statistics, StorageError, format_utc_millis,
    model::{MAX_RECORD_BYTES, MAX_WORD_JSON_BYTES, count_values, ratio_cmp, validate_result_id},
};
use crate::{
    content,
    engine::{Mode, Policy, ResultSnapshot},
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, TransactionBehavior, params, params_from_iter,
    types::Value,
};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path, time::Duration};

const APPLICATION_ID: i64 = 0x5459_5031;
const HEADER_LIMIT: usize = 64 * 1024;
const SCHEMA: &str = r#"
CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,applied_at_utc_ms INTEGER NOT NULL);
CREATE TABLE results(
 id TEXT PRIMARY KEY,created_at_utc_ms INTEGER NOT NULL,profile_key TEXT NOT NULL,
 mode TEXT NOT NULL,source_id TEXT NOT NULL,outcome TEXT NOT NULL,eligible INTEGER NOT NULL,
 elapsed_us INTEGER NOT NULL CHECK(elapsed_us>=0),credited_units INTEGER NOT NULL CHECK(credited_units>=0),
 retained_units INTEGER NOT NULL CHECK(retained_units>=0),attempts_total INTEGER NOT NULL CHECK(attempts_total>=0),
 attempts_correct INTEGER NOT NULL CHECK(attempts_correct>=0),deletion_count INTEGER NOT NULL CHECK(deletion_count>=0),
 final_correct INTEGER NOT NULL CHECK(final_correct>=0),final_incorrect INTEGER NOT NULL CHECK(final_incorrect>=0),
 final_extra INTEGER NOT NULL CHECK(final_extra>=0),final_missed INTEGER NOT NULL CHECK(final_missed>=0),
 header_json TEXT NOT NULL,sparkline_json TEXT NOT NULL,word_summaries_omitted INTEGER NOT NULL);
CREATE INDEX results_profile_history ON results(profile_key,created_at_utc_ms DESC,id DESC);
CREATE INDEX results_date ON results(created_at_utc_ms DESC,id DESC);
CREATE INDEX results_mode_date ON results(mode,created_at_utc_ms DESC,id DESC);
CREATE INDEX results_source_date ON results(source_id,created_at_utc_ms DESC,id DESC);
CREATE INDEX results_outcome_date ON results(outcome,created_at_utc_ms DESC,id DESC);
CREATE INDEX results_profile_eligible ON results(profile_key,eligible,credited_units,elapsed_us);
CREATE TABLE samples(result_id TEXT NOT NULL REFERENCES results(id) ON DELETE CASCADE,bucket_index INTEGER NOT NULL,duration_us INTEGER NOT NULL,credited_units INTEGER NOT NULL,retained_units INTEGER NOT NULL,attempts_total INTEGER NOT NULL,attempts_correct INTEGER NOT NULL,errors INTEGER NOT NULL,sample_json TEXT NOT NULL,PRIMARY KEY(result_id,bucket_index));
CREATE TABLE word_summaries(result_id TEXT NOT NULL REFERENCES results(id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,token_identity TEXT NOT NULL,attempts INTEGER NOT NULL,errors INTEGER NOT NULL,elapsed_us INTEGER NOT NULL,summary_json TEXT NOT NULL,PRIMARY KEY(result_id,ordinal));
CREATE TABLE private_content(result_id TEXT PRIMARY KEY REFERENCES results(id) ON DELETE CASCADE,text_body TEXT NOT NULL,scope_json TEXT NOT NULL);
CREATE TABLE event_traces(result_id TEXT PRIMARY KEY REFERENCES results(id) ON DELETE CASCADE,trace_json TEXT NOT NULL);
CREATE TABLE profile_bests(profile_key TEXT PRIMARY KEY,result_id TEXT NOT NULL REFERENCES results(id),credited_units INTEGER NOT NULL,elapsed_us INTEGER NOT NULL CHECK(elapsed_us>0));
CREATE TABLE profile_stats(profile_key TEXT NOT NULL,outcome TEXT NOT NULL,result_count INTEGER NOT NULL,speed_sample_count INTEGER NOT NULL,target_speed_sample_count INTEGER NOT NULL,elapsed_us INTEGER NOT NULL,target_elapsed_us INTEGER NOT NULL,credited_units INTEGER NOT NULL,retained_units INTEGER NOT NULL,attempts_total INTEGER NOT NULL,attempts_correct INTEGER NOT NULL,target_attempts_total INTEGER NOT NULL,target_attempts_correct INTEGER NOT NULL,PRIMARY KEY(profile_key,outcome)) STRICT;
"#;

pub struct ReadStore {
    // Field drop order is deliberate: close SQLite before releasing the Windows
    // directory handle that prevents a validated parent from being replaced.
    pub(crate) connection: Option<Connection>,
    #[cfg(windows)]
    _private_directory: Option<clack_private_fs::PrivateDirectory>,
}
impl ReadStore {
    /// Opens only an existing database. Missing history is an empty result set.
    /// No directories, SQL schema, migrations, or journal-mode changes are made.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        match fs::metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    connection: None,
                    #[cfg(windows)]
                    _private_directory: None,
                });
            }
            Err(error) => return Err(StorageError::io(error)),
            Ok(_) => {}
        }
        #[cfg(windows)]
        let private_directory = private_database_directory(path)?;
        #[cfg(windows)]
        validate_private_database_files(path)?;
        let connection = Connection::open_with_flags(
            canonical_database_path(path)?,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(StorageError::sql)?;
        connection
            .busy_timeout(Duration::from_millis(250))
            .map_err(StorageError::sql)?;
        connection
            .pragma_update(None, "query_only", true)
            .map_err(StorageError::sql)?;
        verify_schema(&connection)?;
        Ok(Self {
            connection: Some(connection),
            #[cfg(windows)]
            _private_directory: Some(private_directory),
        })
    }
    pub fn history(&self, filter: &Filter, page: Page) -> Result<HistoryPage, StorageError> {
        filter.validate()?;
        page.validate()?;
        let Some(connection) = &self.connection else {
            return Ok(HistoryPage::default());
        };
        let (clause, mut values) = where_clause(filter);
        values.push(Value::Integer(i64::from(page.limit) + 1));
        values.push(Value::Integer(page.offset as i64));
        let sql = format!(
            "SELECT id,created_at_utc_ms,CASE WHEN length(header_json)<={HEADER_LIMIT} THEN header_json END,CASE WHEN length(sparkline_json)<=8192 THEN sparkline_json END,word_summaries_omitted FROM results {clause} ORDER BY created_at_utc_ms DESC,id DESC LIMIT ? OFFSET ?"
        );
        let mut statement = connection.prepare(&sql).map_err(StorageError::sql)?;
        let mut rows = statement
            .query(params_from_iter(values))
            .map_err(StorageError::sql)?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().map_err(StorageError::sql)? {
            let id: String = row.get(0).map_err(StorageError::sql)?;
            let stamp: i64 = row.get(1).map_err(StorageError::sql)?;
            let header: String = row.get(2).map_err(|_| corrupt_record())?;
            let snapshot: ResultSnapshot = decode(&header)?;
            let sparkline: Vec<Option<f64>> =
                decode(&row.get::<_, String>(3).map_err(|_| corrupt_record())?)?;
            if sparkline.len() > 32 || sparkline.iter().flatten().any(|value| !value.is_finite()) {
                return Err(corrupt_record());
            }
            let omitted: i64 = row.get(4).map_err(StorageError::sql)?;
            if omitted < 0 {
                return Err(corrupt_record());
            }
            let record = Record {
                record_version: super::EXPORT_VERSION,
                id: id.clone(),
                created_at_utc_ms: stamp,
                snapshot,
                full_text: None,
                text_scope: None,
                event_trace: None,
                word_summaries_omitted: omitted as u64,
            };
            record.validate().map_err(|_| corrupt_record())?;
            if !record.snapshot.words.is_empty() || !record.snapshot.samples.is_empty() {
                return Err(corrupt_record());
            }
            results.push(HistoryEntry {
                id,
                created_at_utc_ms: stamp,
                created_at_utc: format_utc_millis(stamp),
                snapshot: record.snapshot,
                sparkline,
                word_summaries_omitted: omitted as u64,
            });
        }
        let next_offset = if results.len() > page.limit as usize {
            results.pop();
            Some(page.offset + u64::from(page.limit))
        } else {
            None
        };
        Ok(HistoryPage {
            results,
            next_offset,
        })
    }
    pub fn best(&self, profile_key: &str) -> Result<Option<Best>, StorageError> {
        Filter::current(profile_key).validate()?;
        self.connection
            .as_ref()
            .map_or(Ok(None), |connection| best(connection, profile_key))
    }
    pub fn stats(&self, filter: &Filter) -> Result<Statistics, StorageError> {
        filter.validate()?;
        let Some(connection) = &self.connection else {
            return Ok(Statistics::default());
        };
        let (sql, values) = if let Some(profile_key) = &filter.profile_key
            && filter.mode.is_none()
            && filter.language.is_none()
            && filter.classification.is_none()
            && filter.from_utc_ms.is_none()
            && filter.to_utc_ms.is_none()
        {
            let mut clause = "WHERE profile_key=?".to_owned();
            let mut values = vec![Value::Text(profile_key.clone())];
            if let Some(outcome) = filter.outcome {
                clause.push_str(" AND outcome=?");
                values.push(Value::Text(enum_text(outcome)));
            }
            (
                format!(
                    "SELECT COALESCE(SUM(result_count),0),COALESCE(SUM(speed_sample_count),0),COALESCE(SUM(target_speed_sample_count),0),COALESCE(SUM(elapsed_us),0),COALESCE(SUM(target_elapsed_us),0),COALESCE(SUM(credited_units),0),COALESCE(SUM(retained_units),0),COALESCE(SUM(attempts_total),0),COALESCE(SUM(attempts_correct),0),COALESCE(SUM(target_attempts_total),0),COALESCE(SUM(target_attempts_correct),0) FROM profile_stats {clause}"
                ),
                values,
            )
        } else {
            let (clause, values) = where_clause(filter);
            (
                format!(
                    "SELECT COUNT(*),COALESCE(SUM(elapsed_us>0 AND attempts_total>0),0),COALESCE(SUM(elapsed_us>0 AND attempts_total>0 AND mode!='zen'),0),COALESCE(SUM(CASE WHEN attempts_total>0 THEN elapsed_us ELSE 0 END),0),COALESCE(SUM(CASE WHEN attempts_total>0 AND mode!='zen' THEN elapsed_us ELSE 0 END),0),COALESCE(SUM(CASE WHEN elapsed_us>0 AND mode!='zen' THEN credited_units ELSE 0 END),0),COALESCE(SUM(CASE WHEN elapsed_us>0 THEN retained_units ELSE 0 END),0),COALESCE(SUM(attempts_total),0),COALESCE(SUM(attempts_correct),0),COALESCE(SUM(CASE WHEN mode!='zen' THEN attempts_total ELSE 0 END),0),COALESCE(SUM(CASE WHEN mode!='zen' THEN attempts_correct ELSE 0 END),0) FROM results {clause}"
                ),
                values,
            )
        };
        let values: Vec<i64> = connection
            .query_row(&sql, params_from_iter(values), |row| {
                (0..11).map(|index| row.get(index)).collect()
            })
            .map_err(StorageError::sql)?;
        if values.iter().any(|value| *value < 0) {
            return Err(corrupt_record());
        }
        let v: Vec<u64> = values.into_iter().map(|v| v as u64).collect();
        let mut result = Statistics {
            result_count: v[0],
            speed_sample_count: v[1],
            target_speed_sample_count: v[2],
            elapsed_us: v[3],
            target_elapsed_us: v[4],
            credited_units: v[5],
            retained_units: v[6],
            attempts_total: v[7],
            attempts_correct: v[8],
            target_attempts_total: v[9],
            target_attempts_correct: v[10],
            ..Statistics::default()
        };
        result.finish();
        Ok(result)
    }
    pub fn review(&self, id: &str) -> Result<Option<Review>, StorageError> {
        validate_result_id(id)?;
        let Some(connection) = &self.connection else {
            return Ok(None);
        };
        let row:Option<(i64,String,i64)>=connection.query_row(&format!("SELECT created_at_utc_ms,CASE WHEN length(header_json)<={HEADER_LIMIT} THEN header_json END,word_summaries_omitted FROM results WHERE id=?"),[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(StorageError::sql)?;
        let Some((stamp, header, omitted)) = row else {
            return Ok(None);
        };
        if omitted < 0 {
            return Err(corrupt_record());
        }
        let mut record = Record {
            record_version: super::EXPORT_VERSION,
            id: id.into(),
            created_at_utc_ms: stamp,
            snapshot: decode(&header)?,
            full_text: None,
            text_scope: None,
            event_trace: None,
            word_summaries_omitted: omitted as u64,
        };
        let mut total = header.len();
        record.snapshot.samples = read_children(
            connection,
            "samples",
            "sample_json",
            "bucket_index",
            id,
            (crate::engine::SAMPLE_CAPACITY, 65_536),
            &mut total,
        )?;
        record.snapshot.words = read_children(
            connection,
            "word_summaries",
            "summary_json",
            "ordinal",
            id,
            (super::WORD_SUMMARY_LIMIT, MAX_WORD_JSON_BYTES),
            &mut total,
        )?;
        let body:Option<(String,String)>=connection.query_row("SELECT CASE WHEN length(CAST(text_body AS BLOB))<=?2 THEN text_body END,CASE WHEN length(scope_json)<=32 THEN scope_json END FROM private_content WHERE result_id=?1",params![id,content::MAX_CANONICAL_CUSTOM_BYTES as i64],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(StorageError::sql)?;
        if let Some((text, scope)) = body {
            total += text.len();
            record.full_text = Some(text);
            record.text_scope = Some(decode(&scope)?);
        }
        let trace:Option<String>=connection.query_row("SELECT CASE WHEN length(CAST(trace_json AS BLOB))<=4194304 THEN trace_json END FROM event_traces WHERE result_id=?",[id],|row|row.get(0)).optional().map_err(StorageError::sql)?;
        total += trace.as_ref().map_or(0, String::len);
        if total > MAX_RECORD_BYTES {
            return Err(corrupt_record());
        }
        record.event_trace = trace.as_deref().map(decode).transpose()?;
        record.validate().map_err(|_| corrupt_record())?;
        let content = if record.full_text.is_some() {
            ReviewContent::Stored
        } else if record.snapshot.spec.approved_content {
            ReviewContent::Bundled
        } else if matches!(record.snapshot.spec.mode, Mode::Custom | Mode::Code) {
            ReviewContent::OriginalRequired {
                content_hash: record.snapshot.spec.content_hash.clone(),
            }
        } else {
            ReviewContent::Unavailable
        };
        Ok(Some(Review {
            record,
            content,
            verified_original: None,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ReviewContent {
    Stored,
    Bundled,
    OriginalRequired { content_hash: String },
    OriginalVerified,
    Unavailable,
}
#[derive(Debug, Clone)]
pub struct Review {
    pub record: Record,
    pub content: ReviewContent,
    pub verified_original: Option<String>,
}
impl Review {
    pub fn attach_original(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        let spec = &self.record.snapshot.spec;
        if !matches!(spec.mode, Mode::Custom | Mode::Code) {
            return Err(StorageError::new(
                ErrorKind::Invalid,
                "original-file matching applies to custom and code history",
            ));
        }
        let policy = if spec.policy == Policy::Exact {
            content::InputPolicy::Exact
        } else {
            content::InputPolicy::Prose
        };
        let prepared = content::prepare_custom(bytes, policy, spec.normalize).map_err(|_| {
            StorageError::new(
                ErrorKind::Invalid,
                "original source is not valid under the saved input policy",
            )
        })?;
        if prepared.content_hash != spec.content_hash {
            return Err(StorageError::new(
                ErrorKind::Invalid,
                "original source hash does not match this historical result",
            ));
        }
        self.verified_original = Some(prepared.text);
        self.content = ReviewContent::OriginalVerified;
        Ok(())
    }
}

pub(crate) struct Writer {
    pub reader: ReadStore,
    pub journal: String,
}
impl Writer {
    pub fn open(options: &Options) -> Result<Self, StorageError> {
        options.validate()?;
        let parent = options
            .database
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if !parent.exists() {
            #[cfg(windows)]
            clack_private_fs::create_dir_all(parent).map_err(windows_private_error)?;
            #[cfg(not(windows))]
            {
                let mut builder = fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(parent).map_err(StorageError::io)?;
            }
        }
        #[cfg(windows)]
        let private_directory = private_database_directory(&options.database)?;
        match fs::symlink_metadata(&options.database) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(StorageError::new(
                    ErrorKind::Io,
                    "history database path must not be a symbolic link",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                #[cfg(windows)]
                let opened = clack_private_fs::create_new_file(&options.database);
                #[cfg(not(windows))]
                let opened = {
                    let mut open = fs::OpenOptions::new();
                    open.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        open.mode(0o600);
                    }
                    open.open(&options.database)
                };
                match opened {
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(StorageError::io(error)),
                }
            }
            Err(error) => return Err(StorageError::io(error)),
            _ => {}
        }
        #[cfg(windows)]
        validate_private_database_files(&options.database)?;
        let mut connection = Connection::open_with_flags(
            canonical_database_path(&options.database)?,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(StorageError::sql)?;
        connection
            .busy_timeout(options.busy_budget)
            .map_err(StorageError::sql)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(StorageError::sql)?;
        if version > i64::from(SQL_SCHEMA_VERSION) || version < 0 {
            return Err(StorageError::new(
                ErrorKind::UnsupportedSchema,
                "history schema is newer than this application; database left unchanged",
            ));
        }
        if version == 0 {
            let tables: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |row| row.get(0),
                )
                .map_err(StorageError::sql)?;
            if tables != 0 {
                return Err(StorageError::new(
                    ErrorKind::UnsupportedSchema,
                    "unrecognized history schema; database left unchanged",
                ));
            }
        } else {
            verify_schema(&connection)?;
        }
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(StorageError::sql)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(StorageError::sql)?;
        #[cfg(target_os = "macos")]
        connection
            .pragma_update(None, "fullfsync", true)
            .map_err(StorageError::sql)?;
        let wal = options.journal == JournalMode::Wal
            || (options.journal == JournalMode::Auto && known_local(parent));
        if wal && !sqlite_wal_patched() {
            return Err(StorageError::new(
                ErrorKind::Unavailable,
                "bundled SQLite lacks the WAL-reset fix; use a patched release or delete journal mode",
            ));
        }
        let journal: String = connection
            .query_row(
                if wal {
                    "PRAGMA journal_mode=WAL"
                } else {
                    "PRAGMA journal_mode=DELETE"
                },
                [],
                |row| row.get(0),
            )
            .map_err(StorageError::sql)?;
        if options.journal == JournalMode::Wal && journal != "wal" {
            return Err(StorageError::new(
                ErrorKind::Unavailable,
                "WAL is unavailable on this storage; configure delete journal mode",
            ));
        }
        if version == 0 {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(StorageError::sql)?;
            transaction
                .execute_batch(SCHEMA)
                .map_err(StorageError::sql)?;
            for (name, classification) in [
                ("standard", Classification::Standard),
                ("practice", Classification::Practice),
                ("paste", Classification::PasteAttempted),
                ("assisted", Classification::AssistedCode),
            ] {
                transaction.execute_batch(&format!(
                    "CREATE INDEX results_{name}_history ON results(created_at_utc_ms DESC,id DESC) WHERE {}",
                    classification.predicate()
                )).map_err(StorageError::sql)?;
            }
            transaction.execute("INSERT INTO schema_migrations(version,applied_at_utc_ms) VALUES(1,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",[]).map_err(StorageError::sql)?;
            transaction
                .pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(StorageError::sql)?;
            transaction
                .pragma_update(None, "user_version", SQL_SCHEMA_VERSION)
                .map_err(StorageError::sql)?;
            transaction.commit().map_err(StorageError::sql)?;
        }
        Ok(Self {
            reader: ReadStore {
                connection: Some(connection),
                #[cfg(windows)]
                _private_directory: Some(private_directory),
            },
            journal,
        })
    }
    pub fn save(&mut self, record: &Record) -> Result<BestOutcome, StorageError> {
        record.validate()?;
        let snapshot = &record.snapshot;
        let elapsed = integer(snapshot.elapsed_us)?;
        let c = count_values(&snapshot.counts)
            .map(integer)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let eligible = snapshot.personal_best_eligible && snapshot.eligible_for_standard_best();
        let mut header = snapshot.clone();
        header.samples.clear();
        header.words.clear();
        header.personal_best_eligible = eligible;
        let header = encode(&header)?;
        if header.len() > HEADER_LIMIT {
            return Err(StorageError::new(
                ErrorKind::Invalid,
                "result header exceeds 64 KiB",
            ));
        }
        let stride = snapshot.samples.len().div_ceil(32).max(1);
        let sparkline: Vec<_> = snapshot
            .samples
            .iter()
            .step_by(stride)
            .take(32)
            .map(|sample| {
                if snapshot.spec.mode == Mode::Zen {
                    sample.metrics.raw_wpm
                } else {
                    sample.metrics.wpm
                }
            })
            .collect();
        let sparkline = encode(&sparkline)?;
        let connection = self.reader.connection.as_mut().expect("writer connection");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::sql)?;
        if transaction
            .query_row("SELECT 1 FROM results WHERE id=?", [&record.id], |_| Ok(()))
            .optional()
            .map_err(StorageError::sql)?
            .is_some()
        {
            return Ok(BestOutcome::AlreadySaved);
        }
        transaction.execute("INSERT INTO results(id,created_at_utc_ms,profile_key,mode,source_id,outcome,eligible,elapsed_us,credited_units,retained_units,attempts_total,attempts_correct,deletion_count,final_correct,final_incorrect,final_extra,final_missed,header_json,sparkline_json,word_summaries_omitted) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",params![record.id,record.created_at_utc_ms,snapshot.profile_key,enum_text(snapshot.spec.mode),snapshot.spec.source_id,enum_text(snapshot.outcome),eligible,elapsed,c[0],c[1],c[2],c[3],c[4],c[5],c[6],c[7],c[8],header,sparkline,integer(record.word_summaries_omitted)?]).map_err(StorageError::sql)?;
        {
            let mut insert = transaction
                .prepare_cached("INSERT INTO samples VALUES(?,?,?,?,?,?,?,?,?)")
                .map_err(StorageError::sql)?;
            for sample in &snapshot.samples {
                insert
                    .execute(params![
                        record.id,
                        integer(sample.bucket_index)?,
                        integer(sample.duration_us)?,
                        integer(sample.counts.credited_units)?,
                        integer(sample.counts.retained_units)?,
                        integer(sample.attempts)?,
                        integer(sample.attempts.saturating_sub(sample.errors))?,
                        integer(sample.errors)?,
                        encode(sample)?
                    ])
                    .map_err(StorageError::sql)?;
            }
        }
        {
            let mut insert = transaction
                .prepare_cached("INSERT INTO word_summaries VALUES(?,?,?,?,?,?,?)")
                .map_err(StorageError::sql)?;
            for (index, word) in snapshot.words.iter().enumerate() {
                insert
                    .execute(params![
                        record.id,
                        index as i64,
                        content::content_hash(word.token.as_bytes()),
                        integer(word.attempts)?,
                        integer(word.errors)?,
                        integer(word.elapsed_us)?,
                        encode(word)?
                    ])
                    .map_err(StorageError::sql)?;
            }
        }
        if let Some(text) = &record.full_text {
            transaction
                .execute(
                    "INSERT INTO private_content VALUES(?,?,?)",
                    params![record.id, text, encode(&record.text_scope)?],
                )
                .map_err(StorageError::sql)?;
        }
        if let Some(trace) = &record.event_trace {
            transaction
                .execute(
                    "INSERT INTO event_traces VALUES(?,?)",
                    params![record.id, encode(trace)?],
                )
                .map_err(StorageError::sql)?;
        }
        let comparison = if !eligible {
            BestOutcome::Ineligible
        } else {
            match best(&transaction, &snapshot.profile_key)? {
                None => BestOutcome::NewBest,
                Some(previous) => match ratio_cmp(
                    snapshot.counts.credited_units,
                    snapshot.elapsed_us,
                    previous.credited_units,
                    previous.elapsed_us,
                ) {
                    std::cmp::Ordering::Greater => BestOutcome::NewBest,
                    std::cmp::Ordering::Equal => BestOutcome::Tied,
                    std::cmp::Ordering::Less => BestOutcome::NotBest,
                },
            }
        };
        if comparison == BestOutcome::NewBest {
            transaction.execute("INSERT INTO profile_bests VALUES(?,?,?,?) ON CONFLICT(profile_key) DO UPDATE SET result_id=excluded.result_id,credited_units=excluded.credited_units,elapsed_us=excluded.elapsed_us",params![snapshot.profile_key,record.id,c[0],elapsed]).map_err(StorageError::sql)?;
        }
        let valid = snapshot.elapsed_us > 0 && snapshot.counts.attempts_total > 0;
        let target = snapshot.spec.mode != Mode::Zen;
        transaction.execute("INSERT INTO profile_stats VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(profile_key,outcome) DO UPDATE SET result_count=result_count+excluded.result_count,speed_sample_count=speed_sample_count+excluded.speed_sample_count,target_speed_sample_count=target_speed_sample_count+excluded.target_speed_sample_count,elapsed_us=elapsed_us+excluded.elapsed_us,target_elapsed_us=target_elapsed_us+excluded.target_elapsed_us,credited_units=credited_units+excluded.credited_units,retained_units=retained_units+excluded.retained_units,attempts_total=attempts_total+excluded.attempts_total,attempts_correct=attempts_correct+excluded.attempts_correct,target_attempts_total=target_attempts_total+excluded.target_attempts_total,target_attempts_correct=target_attempts_correct+excluded.target_attempts_correct",params![snapshot.profile_key,enum_text(snapshot.outcome),1,i64::from(valid),i64::from(valid&&target),if valid{elapsed}else{0},if valid&&target{elapsed}else{0},if valid&&target{c[0]}else{0},if valid{c[1]}else{0},c[2],c[3],if target{c[2]}else{0},if target{c[3]}else{0}]).map_err(StorageError::sql)?;
        transaction.commit().map_err(StorageError::sql)?;
        Ok(comparison)
    }
}

fn verify_schema(connection: &Connection) -> Result<(), StorageError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(StorageError::sql)?;
    let application: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(StorageError::sql)?;
    if version != i64::from(SQL_SCHEMA_VERSION) || application != APPLICATION_ID {
        return Err(StorageError::new(
            ErrorKind::UnsupportedSchema,
            "history schema is unsupported; no migration or repair was performed",
        ));
    }
    let applied: Option<i64> = connection
        .query_row(
            "SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::sql)?;
    if applied != Some(version) {
        return Err(corrupt_record());
    }
    Ok(())
}
fn best(connection: &Connection, profile: &str) -> Result<Option<Best>, StorageError> {
    let row: Option<(String, i64, i64)> = connection
        .query_row(
            "SELECT result_id,credited_units,elapsed_us FROM profile_bests WHERE profile_key=?",
            [profile],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(StorageError::sql)?;
    row.map(|(id, units, elapsed)| {
        if units < 0 || elapsed <= 0 || validate_result_id(&id).is_err() {
            Err(corrupt_record())
        } else {
            Ok(Best {
                result_id: id,
                credited_units: units as u64,
                elapsed_us: elapsed as u64,
            })
        }
    })
    .transpose()
}
fn where_clause(filter: &Filter) -> (String, Vec<Value>) {
    let mut conditions = Vec::new();
    let mut values = Vec::new();
    for (column, value) in [
        ("profile_key", filter.profile_key.clone()),
        ("mode", filter.mode.map(enum_text)),
        ("source_id", filter.language.clone()),
        ("outcome", filter.outcome.map(enum_text)),
    ] {
        if let Some(value) = value {
            conditions.push(format!("{column}=?"));
            values.push(Value::Text(value));
        }
    }
    if let Some(classification) = filter.classification {
        conditions.push(classification.predicate().into());
    }
    if let Some(value) = filter.from_utc_ms {
        conditions.push("created_at_utc_ms>=?".into());
        values.push(Value::Integer(value));
    }
    if let Some(value) = filter.to_utc_ms {
        conditions.push("created_at_utc_ms<?".into());
        values.push(Value::Integer(value));
    }
    (
        if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        },
        values,
    )
}
fn read_children<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    table: &str,
    column: &str,
    order: &str,
    id: &str,
    bounds: (usize, usize),
    total: &mut usize,
) -> Result<Vec<T>, StorageError> {
    let (limit, item_bytes) = bounds;
    let sql = format!(
        "SELECT CASE WHEN length(CAST({column} AS BLOB))<={item_bytes} THEN {column} END FROM {table} WHERE result_id=? ORDER BY {order} LIMIT ?"
    );
    let mut statement = connection.prepare(&sql).map_err(StorageError::sql)?;
    let mut rows = statement
        .query(params![id, (limit + 1) as i64])
        .map_err(StorageError::sql)?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().map_err(StorageError::sql)? {
        let text: String = row.get(0).map_err(|_| corrupt_record())?;
        *total += text.len();
        if *total > MAX_RECORD_BYTES || values.len() == limit {
            return Err(corrupt_record());
        }
        values.push(decode(&text)?);
    }
    Ok(values)
}
fn known_local(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        nix::sys::statfs::statfs(path)
            .is_ok_and(|info| matches!(info.filesystem_type_name(), "apfs" | "hfs"))
    }
    #[cfg(target_os = "linux")]
    {
        use nix::sys::statfs::*;
        statfs(path).is_ok_and(|info| {
            [
                EXT4_SUPER_MAGIC,
                BTRFS_SUPER_MAGIC,
                TMPFS_MAGIC,
                OVERLAYFS_SUPER_MAGIC,
            ]
            .contains(&info.filesystem_type())
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        false
    }
}
#[cfg(windows)]
fn windows_private_error(_: io::Error) -> StorageError {
    StorageError::new(
        ErrorKind::Io,
        "Windows history requires a private owned directory and private database/journal files; choose a new --data-dir",
    )
}
#[cfg(windows)]
fn private_database_directory(
    path: &Path,
) -> Result<clack_private_fs::PrivateDirectory, StorageError> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    clack_private_fs::require_private_directory(parent).map_err(windows_private_error)
}
#[cfg(windows)]
fn validate_private_database_files(path: &Path) -> Result<(), StorageError> {
    // A protected parent gives future SQLite sidecars private inheritable ACLs.
    // Existing files can have their own explicit grants, so validate them too.
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        let file = std::path::PathBuf::from(name);
        match fs::symlink_metadata(&file) {
            Ok(_) => {
                clack_private_fs::require_private_file(&file).map_err(windows_private_error)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(StorageError::io(error)),
        }
    }
    Ok(())
}
fn canonical_database_path(path: &Path) -> Result<std::path::PathBuf, StorageError> {
    // SQLITE_OPEN_NOFOLLOW also rejects symlinks in parent components. macOS's
    // normal /var temporary path is itself a symlink to /private/var. Resolve
    // only the parent, retaining the final component for SQLite's race-safe check.
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        StorageError::new(ErrorKind::Invalid, "history database path needs a filename")
    })?;
    Ok(fs::canonicalize(parent)
        .map_err(StorageError::io)?
        .join(name))
}
fn sqlite_wal_patched() -> bool {
    let version = rusqlite::version_number();
    version >= 3_051_003 || version == 3_050_007 || version == 3_044_006
}
fn integer(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| {
        StorageError::new(
            ErrorKind::Invalid,
            "result counter exceeds SQLite's exact signed-integer range",
        )
    })
}
fn enum_text(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .expect("serializable enum")
        .as_str()
        .expect("string enum")
        .to_owned()
}
fn encode(value: &impl Serialize) -> Result<String, StorageError> {
    serde_json::to_string(value)
        .map_err(|_| StorageError::new(ErrorKind::Invalid, "result cannot be serialized"))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, StorageError> {
    serde_json::from_str(value).map_err(|_| corrupt_record())
}
fn corrupt_record() -> StorageError {
    StorageError::new(
        ErrorKind::Corrupt,
        "stored history metadata is invalid; database was not repaired or replaced",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Engine, TestSpec};
    use crate::storage::Persistence;
    fn fixture() -> (tempfile::TempDir, Writer) {
        let directory = tempfile::tempdir().unwrap();
        let mut options = Options::new(directory.path().join(if cfg!(windows) {
            "private/history.sqlite3"
        } else {
            "history.sqlite3"
        }));
        options.journal = JournalMode::Delete;
        let writer = Writer::open(&options).unwrap();
        (directory, writer)
    }
    fn private_record(text: &str) -> Record {
        let spec = TestSpec {
            mode: Mode::Custom,
            policy: Policy::Exact,
            normalize: false,
            source_id: "custom".into(),
            approved_content: false,
            content_hash: content::content_hash(text.as_bytes()),
            ..TestSpec::default()
        };
        let snapshot = Engine::new(spec, "x").unwrap().snapshot();
        Record::prepare(
            snapshot,
            Persistence {
                store_custom_text: true,
                ..Persistence::default()
            },
            Some(text),
            None,
        )
        .unwrap()
        .unwrap()
    }
    #[test]
    fn acknowledged_connection_uses_full_durability_and_patched_sqlite() {
        let (_directory, writer) = fixture();
        let connection = writer.reader.connection.as_ref().unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            connection
                .pragma_query_value(None, "fullfsync", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(sqlite_wal_patched());
        verify_schema(connection).unwrap();
    }
    #[test]
    fn sqlite_full_rolls_back_result_children_stats_and_can_retry_same_record() {
        let (_directory, mut writer) = fixture();
        let result = private_record(&"a ".repeat(300_000));
        let connection = writer.reader.connection.as_ref().unwrap();
        let pages: i64 = connection
            .pragma_query_value(None, "page_count", |row| row.get(0))
            .unwrap();
        let _: i64 = connection
            .query_row(&format!("PRAGMA max_page_count={pages}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        let error = writer.save(&result).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Full);
        assert_eq!(
            writer
                .reader
                .stats(&Filter::default())
                .unwrap()
                .result_count,
            0
        );
        let connection = writer.reader.connection.as_ref().unwrap();
        for table in [
            "results",
            "samples",
            "word_summaries",
            "private_content",
            "event_traces",
            "profile_stats",
            "profile_bests",
        ] {
            assert_eq!(
                connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        let _: i64 = connection
            .query_row("PRAGMA max_page_count=100000", [], |row| row.get(0))
            .unwrap();
        assert_eq!(writer.save(&result).unwrap(), BestOutcome::Ineligible);
        assert_eq!(
            writer
                .reader
                .stats(&Filter::default())
                .unwrap()
                .result_count,
            1
        );
    }
    #[test]
    fn sqlite_read_only_rejects_transaction_without_partial_save() {
        let (_directory, mut writer) = fixture();
        let result = private_record("private fixture");
        writer
            .reader
            .connection
            .as_ref()
            .unwrap()
            .pragma_update(None, "query_only", true)
            .unwrap();
        assert_eq!(writer.save(&result).unwrap_err().kind, ErrorKind::ReadOnly);
        assert_eq!(
            writer
                .reader
                .stats(&Filter::default())
                .unwrap()
                .result_count,
            0
        );
        writer
            .reader
            .connection
            .as_ref()
            .unwrap()
            .pragma_update(None, "query_only", false)
            .unwrap();
        assert_eq!(writer.save(&result).unwrap(), BestOutcome::Ineligible);
    }
    #[test]
    fn overflowing_aggregate_never_rounds_to_real_and_never_acknowledges_partial_result() {
        let (_directory, mut writer) = fixture();
        let first = private_record("first");
        writer.save(&first).unwrap();
        writer
            .reader
            .connection
            .as_ref()
            .unwrap()
            .execute("UPDATE profile_stats SET result_count=?", [i64::MAX])
            .unwrap();
        let mut second = first.clone();
        second.id = content::content_hash(b"different stable fixture id");
        assert!(writer.save(&second).is_err());
        let connection = writer.reader.connection.as_ref().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM results", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT typeof(result_count) FROM profile_stats",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "integer"
        );
    }
}
