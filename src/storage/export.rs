use super::{
    EXPORT_VERSION, ErrorKind, Filter, Page, ReadStore, Record, StorageError, format_utc_millis,
};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Json,
    Jsonl,
    Csv,
}
impl ExportFormat {
    pub fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "json" => Ok(Self::Json),
            "jsonl" => Ok(Self::Jsonl),
            "csv" => Ok(Self::Csv),
            _ => Err(StorageError::new(
                ErrorKind::Invalid,
                "export format expects json, jsonl, or csv",
            )),
        }
    }
}

/// Recovery export of a bounded pending set. No database is required.
pub fn export_records<'a>(
    writer: impl Write,
    format: ExportFormat,
    records: impl IntoIterator<Item = &'a Record>,
    include_text: bool,
) -> Result<(), StorageError> {
    let mut sink = Sink::new(writer, format)?;
    for record in records {
        record.validate()?;
        sink.record(&record.export_view(include_text))?;
    }
    sink.finish()
}
impl ReadStore {
    /// Streams one consistent read transaction in pages; never collects lifetime history.
    pub fn export_to(
        &self,
        writer: impl Write,
        format: ExportFormat,
        filter: &Filter,
        include_text: bool,
    ) -> Result<u64, StorageError> {
        filter.validate()?;
        let transaction = self
            .connection
            .as_ref()
            .map(|connection| connection.unchecked_transaction())
            .transpose()
            .map_err(StorageError::sql)?;
        let mut sink = Sink::new(writer, format)?;
        let mut offset = 0;
        let mut count = 0;
        loop {
            let page = self.history(filter, Page { limit: 200, offset })?;
            for entry in page.results {
                let review = self.review(&entry.id)?.ok_or_else(|| {
                    StorageError::new(
                        ErrorKind::Corrupt,
                        "history result disappeared inside a read transaction",
                    )
                })?;
                sink.record(&review.record.export_view(include_text))?;
                count += 1;
            }
            if let Some(next) = page.next_offset {
                offset = next;
            } else {
                break;
            }
        }
        sink.finish()?;
        if let Some(transaction) = transaction {
            transaction.commit().map_err(StorageError::sql)?;
        }
        Ok(count)
    }
}

enum Sink<W: Write> {
    Json { writer: W, first: bool },
    Jsonl(W),
    Csv(Box<csv::Writer<W>>),
}
impl<W: Write> Sink<W> {
    fn new(mut writer: W, format: ExportFormat) -> Result<Self, StorageError> {
        Ok(match format {
            ExportFormat::Json => {
                write!(
                    writer,
                    "{{\"export_version\":{EXPORT_VERSION},\"results\":["
                )
                .map_err(StorageError::io)?;
                Self::Json {
                    writer,
                    first: true,
                }
            }
            ExportFormat::Jsonl => Self::Jsonl(writer),
            ExportFormat::Csv => {
                let mut writer = csv::WriterBuilder::new()
                    .terminator(csv::Terminator::CRLF)
                    .from_writer(writer);
                writer
                    .write_record([
                        "export_version",
                        "id",
                        "created_at_utc",
                        "mode",
                        "source_id",
                        "profile_key",
                        "outcome",
                        "elapsed_us",
                        "credited_units",
                        "retained_units",
                        "attempts_total",
                        "attempts_correct",
                        "deletion_count",
                        "final_correct",
                        "final_incorrect",
                        "final_extra",
                        "final_missed",
                        "wpm",
                        "raw_wpm",
                        "accuracy",
                        "personal_best_eligible",
                        "effective_test_spec_json",
                        "integrity_json",
                        "word_summaries_omitted",
                        "full_text",
                        "word_summaries_json",
                        "event_trace_json",
                        "text_scope",
                    ])
                    .map_err(csv_error)?;
                Self::Csv(Box::new(writer))
            }
        })
    }
    fn record(&mut self, record: &Record) -> Result<(), StorageError> {
        match self {
            Self::Json { writer, first } => {
                if !*first {
                    writer.write_all(b",").map_err(StorageError::io)?;
                }
                serde_json::to_writer(&mut *writer, record).map_err(json_error)?;
                *first = false;
            }
            Self::Jsonl(writer) => {
                serde_json::to_writer(&mut *writer, record).map_err(json_error)?;
                writer.write_all(b"\n").map_err(StorageError::io)?;
            }
            Self::Csv(writer) => {
                let s = record.snapshot();
                let c = s.counts;
                let fields = vec![
                    EXPORT_VERSION.to_string(),
                    record.id().into(),
                    format_utc_millis(record.created_at_utc_ms()),
                    enum_string(s.spec.mode),
                    s.spec.source_id.clone(),
                    s.profile_key.clone(),
                    enum_string(s.outcome),
                    s.elapsed_us.to_string(),
                    c.credited_units.to_string(),
                    c.retained_units.to_string(),
                    c.attempts_total.to_string(),
                    c.attempts_correct.to_string(),
                    c.deletion_count.to_string(),
                    c.final_correct.to_string(),
                    c.final_incorrect.to_string(),
                    c.final_extra.to_string(),
                    c.final_missed.to_string(),
                    optional(s.metrics.wpm),
                    optional(s.metrics.raw_wpm),
                    optional(s.metrics.accuracy),
                    s.personal_best_eligible.to_string(),
                    serde_json::to_string(&s.spec).map_err(json_error)?,
                    serde_json::to_string(&s.integrity).map_err(json_error)?,
                    record.word_summaries_omitted().to_string(),
                    record.full_text().unwrap_or("").to_owned(),
                    serde_json::to_string(&s.words).map_err(json_error)?,
                    record
                        .event_trace()
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(json_error)?
                        .unwrap_or_default(),
                    record.text_scope().map(enum_string).unwrap_or_default(),
                ];
                writer
                    .write_record(fields.into_iter().map(csv_cell))
                    .map_err(csv_error)?;
            }
        }
        Ok(())
    }
    fn finish(self) -> Result<(), StorageError> {
        match self {
            Self::Json { mut writer, .. } => {
                writer.write_all(b"]}\n").map_err(StorageError::io)?;
                writer.flush().map_err(StorageError::io)
            }
            Self::Jsonl(mut writer) => writer.flush().map_err(StorageError::io),
            Self::Csv(mut writer) => writer.flush().map_err(StorageError::io),
        }
    }
}
fn optional(value: Option<f64>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}
fn enum_string(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .expect("serializable enum")
        .as_str()
        .expect("string enum")
        .to_owned()
}
fn csv_cell(value: String) -> String {
    // Spreadsheet clients may execute formulas after ordinary CSV unquoting.
    // Leading whitespace can be ignored by those clients, so inspect the trimmed cell.
    if value
        .trim_start_matches(char::is_whitespace)
        .starts_with(['=', '+', '-', '@'])
        || value.starts_with(['\t', '\r'])
    {
        format!("'{value}")
    } else {
        value
    }
}
fn csv_error(_: csv::Error) -> StorageError {
    StorageError::new(ErrorKind::Io, "CSV export could not be written")
}
fn json_error(_: serde_json::Error) -> StorageError {
    StorageError::new(ErrorKind::Io, "JSON export could not be written")
}
