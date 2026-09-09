//! Storage acceptance tests use actual SQLite transactions and the production
//! worker. Disk-full injection lives beside Writer so SQLite itself returns FULL.
use clack::{
    content,
    engine::{Counts, Metrics, Mode, Outcome, Policy, ResultSnapshot, TestSpec},
    storage::{
        self, BestOutcome, Classification, ErrorKind, Event, EventTrace, ExportFormat, Filter,
        JournalMode, Options, Page, PendingState, Persistence, ReadStore, Record, ReviewContent,
        Store, TraceEvent, TraceKind, export_records,
    },
};
use rusqlite::Connection;
use std::{
    fs,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const DATE: i64 = 1_783_123_200_000;
fn snapshot(units: u64, elapsed: u64) -> ResultSnapshot {
    let pack = content::bundled_pack("english_200").unwrap();
    let spec = TestSpec {
        source_revision: pack.metadata.revision.clone(),
        content_hash: pack.metadata.content_hash.clone(),
        ..TestSpec::default()
    };
    let counts = Counts {
        credited_units: units,
        retained_units: units,
        attempts_total: units,
        attempts_correct: units,
        final_correct: units,
        ..Counts::default()
    };
    ResultSnapshot {
        export_version: 1,
        app_version: "1.0.0".into(),
        profile_key: spec.profile_key(),
        spec,
        outcome: Outcome::Complete,
        reason: None,
        elapsed_us: elapsed,
        counts,
        metrics: Metrics::calculate(counts, elapsed, false, false),
        integrity: Default::default(),
        samples: vec![clack::engine::Sample {
            bucket_index: 1,
            duration_us: elapsed,
            counts,
            attempts: units,
            errors: 0,
            metrics: Metrics::calculate(counts, elapsed, false, false),
        }],
        words: vec![clack::engine::WordSummary {
            token: "cat".into(),
            entered: "cat".into(),
            attempts: 3,
            expected_units: 3,
            completed_correct: true,
            ..Default::default()
        }],
        personal_best_eligible: true,
    }
}
fn record(units: u64, elapsed: u64, date: i64) -> Record {
    Record::prepare_at(
        snapshot(units, elapsed),
        Persistence::default(),
        None,
        None,
        date,
    )
    .unwrap()
    .unwrap()
}
fn private_snapshot(text: &str) -> ResultSnapshot {
    let mut result = snapshot(10, 1_000_000);
    result.spec.mode = Mode::Custom;
    result.spec.policy = Policy::Prose;
    result.spec.approved_content = false;
    result.spec.source_id = "/private/account-source.txt".into();
    result.spec.source_revision = "private-source-path".into();
    result.spec.content_hash = content::content_hash(text.as_bytes());
    result.profile_key = result.spec.profile_key();
    result.words[0].token = text.into();
    result.words[0].entered = "private_wrong_input".into();
    result
}
struct Fixture {
    // Keep the isolated database directory alive; direct path access is Unix-only.
    _directory: TempDir,
    options: Options,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut options = Options::new(directory.path().join("data/history.sqlite3"));
        options.journal = JournalMode::Delete;
        Self {
            _directory: directory,
            options,
        }
    }
    fn start(&self) -> Store {
        Store::start_after_frame(self.options.clone(), thread::current()).unwrap()
    }
    fn ready(&self) -> Store {
        let mut store = self.start();
        wait(&mut store, |event| matches!(event, Event::Ready { .. }));
        store
    }
    fn reader(&self) -> ReadStore {
        ReadStore::open(&self.options.database).unwrap()
    }
}
fn wait(store: &mut Store, predicate: impl Fn(&Event) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    loop {
        let events = store.poll();
        let done = events.iter().any(&predicate);
        seen.extend(events);
        if done {
            return seen;
        }
        assert!(
            Instant::now() < deadline,
            "worker did not respond: {seen:?}"
        );
        thread::park_timeout(Duration::from_millis(5));
    }
}
fn save(store: &mut Store, record: Record) -> BestOutcome {
    let ticket = store.submit(record).unwrap();
    let events = wait(
        store,
        |event| matches!(event,Event::Saved{ticket:found,..}|Event::Unsaved{ticket:found,..} if *found==ticket),
    );
    events
        .into_iter()
        .find_map(|event| match event {
            Event::Saved {
                ticket: found,
                best,
                ..
            } if found == ticket => Some(best),
            Event::Unsaved {
                ticket: found,
                error,
            } if found == ticket => panic!("result was not saved: {error}"),
            _ => None,
        })
        .unwrap()
}

#[test]
fn missing_read_only_history_is_empty_and_creates_nothing() {
    let fixture = Fixture::new();
    let store = fixture.reader();
    assert!(
        store
            .history(&Filter::default(), Page::default())
            .unwrap()
            .results
            .is_empty()
    );
    assert_eq!(store.stats(&Filter::default()).unwrap().result_count, 0);
    let mut bytes = Vec::new();
    assert_eq!(
        store
            .export_to(&mut bytes, ExportFormat::Json, &Filter::default(), false)
            .unwrap(),
        0
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["results"],
        serde_json::json!([])
    );
    assert!(!fixture.options.database.parent().unwrap().exists());
}

#[test]
fn read_only_worker_queries_missing_or_existing_history_without_creating_or_saving() {
    let fixture = Fixture::new();
    let mut options = fixture.options.clone();
    options.read_only = true;
    let mut store = Store::start_after_frame(options.clone(), thread::current()).unwrap();
    wait(
        &mut store,
        |event| matches!(event,Event::Ready{journal_mode} if journal_mode=="read_only"),
    );
    let request = store
        .request_history(Filter::default(), Page::default())
        .unwrap();
    let events = wait(
        &mut store,
        |event| matches!(event,Event::History{request_id,..} if *request_id==request),
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event,Event::History{page,..} if page.results.is_empty()))
    );
    let rejected = store.submit(record(10, 1_000_000, DATE)).unwrap_err();
    assert_eq!(rejected.error.kind, ErrorKind::ReadOnly);
    assert_eq!(store.pending_count(), 0);
    assert!(!fixture.options.database.parent().unwrap().exists());
    drop(store);
    let mut writer = fixture.ready();
    save(&mut writer, record(10, 1_000_000, DATE));
    drop(writer);
    let before = fs::read(&fixture.options.database).unwrap();
    let mut store = Store::start_after_frame(options, thread::current()).unwrap();
    wait(&mut store, |event| matches!(event, Event::Ready { .. }));
    let request = store.request_stats(Filter::default()).unwrap();
    let events = wait(
        &mut store,
        |event| matches!(event,Event::Stats{request_id,..} if *request_id==request),
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event,Event::Stats{statistics,..} if statistics.result_count==1))
    );
    drop(store);
    assert_eq!(fs::read(&fixture.options.database).unwrap(), before);
}

#[test]
fn an_existing_read_only_worker_observes_history_created_by_another_process() {
    let fixture = Fixture::new();
    let mut options = fixture.options.clone();
    options.read_only = true;
    let mut reader = Store::start_after_frame(options, thread::current()).unwrap();
    wait(&mut reader, |event| matches!(event, Event::Ready { .. }));
    assert!(!fixture.options.database.parent().unwrap().exists());
    let request = reader.request_stats(Filter::default()).unwrap();
    let events = wait(
        &mut reader,
        |event| matches!(event, Event::Stats { request_id, .. } if *request_id == request),
    );
    assert!(events.iter().any(|event| {
        matches!(event, Event::Stats { statistics, .. } if statistics.result_count == 0)
    }));

    let saved = record(25, 5_000_000, DATE);
    let id = saved.id().to_owned();
    let profile = saved.snapshot().profile_key.clone();
    let mut writer = fixture.ready();
    save(&mut writer, saved);
    let request = reader
        .request_history(Filter::default(), Page::default())
        .unwrap();
    let events = wait(
        &mut reader,
        |event| matches!(event, Event::History { request_id, .. } if *request_id == request),
    );
    assert!(events.iter().any(|event| {
        matches!(event, Event::History { page, .. } if page.results.len() == 1 && page.results[0].id == id)
    }));
    let request = reader.request_best(profile).unwrap();
    let events = wait(
        &mut reader,
        |event| matches!(event, Event::Best { request_id, .. } if *request_id == request),
    );
    assert!(events.iter().any(|event| {
        matches!(event, Event::Best { best: Some(best), .. } if best.result_id == id)
    }));
    assert_eq!(
        reader
            .submit(record(5, 1_000_000, DATE))
            .unwrap_err()
            .error
            .kind,
        ErrorKind::ReadOnly
    );
}

#[test]
fn saturated_worker_reconfiguration_leaves_the_previous_frontend_policy_intact() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    // Stop consuming replies until both bounded queues have no capacity. A
    // brief transient full job queue is insufficient: wait for no progress.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_progress = Instant::now();
    let mut requests = 0;
    loop {
        match store.request_stats(Filter::default()) {
            Ok(_) => {
                requests += 1;
                last_progress = Instant::now();
            }
            Err(error) => {
                assert_eq!(error.kind, ErrorKind::Busy);
                if last_progress.elapsed() >= Duration::from_millis(30) {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        assert!(
            Instant::now() < deadline,
            "queues never reached backpressure"
        );
    }
    assert!(requests > 0);
    store.submit(record(5, 1_000_000, DATE)).unwrap();
    store.submit(record(10, 2_000_000, DATE + 1)).unwrap();
    assert!(
        store
            .pending()
            .all(|(_, _, state)| *state == PendingState::Queued)
    );
    let mut rejected_options = fixture.options.clone();
    rejected_options.read_only = true;
    rejected_options.pending_limit = 1;
    assert_eq!(
        store.reconfigure(rejected_options).unwrap_err().kind,
        ErrorKind::Busy
    );
    // Neither the read-only flag nor the smaller pending limit may apply when
    // the FIFO reconfiguration message has not entered the worker queue.
    store.submit(record(15, 3_000_000, DATE + 2)).unwrap();
    assert_eq!(store.pending_count(), 3);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut acknowledgements = Vec::new();
    while store.pending_count() != 0 {
        for event in store.poll() {
            if let Event::Saved { ticket, .. } = event {
                acknowledgements.push(ticket);
            }
        }
        assert!(
            Instant::now() < deadline,
            "saturated saves were not drained"
        );
        thread::park_timeout(Duration::from_millis(2));
    }
    acknowledgements.sort_unstable();
    acknowledgements.dedup();
    assert_eq!(acknowledgements.len(), 3);
    assert_eq!(
        fixture
            .reader()
            .stats(&Filter::default())
            .unwrap()
            .result_count,
        3
    );
}

#[test]
fn paginated_export_holds_a_consistent_snapshot_while_another_writer_commits() {
    struct ConcurrentSink<'a> {
        bytes: Vec<u8>,
        store: &'a mut Store,
        concurrent_record: Option<Record>,
    }
    impl std::io::Write for ConcurrentSink<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            // The first record is already being serialized, so the first
            // 200-row page has established the export's read snapshot.
            if self.bytes.len() >= 1024
                && let Some(record) = self.concurrent_record.take()
            {
                save(self.store, record);
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut fixture = Fixture::new();
    fixture.options.journal = JournalMode::Wal;
    let mut store = fixture.ready();
    let mut original_ids = std::collections::BTreeSet::new();
    for index in 0..201 {
        let result = record(5, 1_000_000, DATE + index);
        original_ids.insert(result.id().to_owned());
        save(&mut store, result);
    }
    let concurrent = record(10, 1_000_000, DATE + 10_000);
    let concurrent_id = concurrent.id().to_owned();
    let reader = fixture.reader();
    let mut sink = ConcurrentSink {
        bytes: Vec::new(),
        store: &mut store,
        concurrent_record: Some(concurrent),
    };
    assert_eq!(
        reader
            .export_to(&mut sink, ExportFormat::Json, &Filter::default(), false)
            .unwrap(),
        201
    );
    assert!(
        sink.concurrent_record.is_none(),
        "concurrent commit did not run"
    );
    let json: serde_json::Value = serde_json::from_slice(&sink.bytes).unwrap();
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 201);
    let exported_ids: std::collections::BTreeSet<_> = results
        .iter()
        .map(|value| value["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(exported_ids, original_ids);
    assert!(!exported_ids.contains(&concurrent_id));
    // A fresh read after export sees the committed record. The fixed snapshot
    // applies only for the duration of that one export transaction.
    assert_eq!(reader.stats(&Filter::default()).unwrap().result_count, 202);
    assert_eq!(
        reader
            .history(&Filter::default(), Page::default())
            .unwrap()
            .results[0]
            .id,
        concurrent_id
    );
}

#[test]
fn reconfiguration_reuses_the_worker_and_preserves_save_then_read_only_order() {
    let fixture = Fixture::new();
    let mut options = fixture.options.clone();
    options.read_only = true;
    let mut store = Store::start_after_frame(options.clone(), thread::current()).unwrap();
    wait(&mut store, |event| matches!(event, Event::Ready { .. }));
    assert!(!fixture.options.database.parent().unwrap().exists());
    store.reconfigure(fixture.options.clone()).unwrap();
    wait(
        &mut store,
        |event| matches!(event, Event::Ready { journal_mode } if journal_mode=="delete"),
    );
    let ticket = store.submit(record(25, 5_000_000, DATE)).unwrap();
    store.reconfigure(options).unwrap();
    assert_eq!(
        store
            .submit(record(10, 1_000_000, DATE))
            .unwrap_err()
            .error
            .kind,
        ErrorKind::ReadOnly
    );
    let events = wait(
        &mut store,
        |event| matches!(event, Event::Ready { journal_mode } if journal_mode=="read_only"),
    );
    let saved = events
        .iter()
        .position(|event| matches!(event, Event::Saved { ticket: found, .. } if *found==ticket))
        .unwrap();
    let reopened = events
        .iter()
        .position(
            |event| matches!(event, Event::Ready { journal_mode } if journal_mode=="read_only"),
        )
        .unwrap();
    assert!(saved < reopened);
    assert_eq!(
        fixture
            .reader()
            .stats(&Filter::default())
            .unwrap()
            .result_count,
        1
    );
    store.reconfigure(fixture.options.clone()).unwrap();
    wait(
        &mut store,
        |event| matches!(event, Event::Ready { journal_mode } if journal_mode=="delete"),
    );
    save(&mut store, record(10, 1_000_000, DATE));
    assert_eq!(
        fixture
            .reader()
            .stats(&Filter::default())
            .unwrap()
            .result_count,
        2
    );
}

#[test]
fn reducing_pending_capacity_preserves_owned_snapshots_and_later_retry() {
    let mut fixture = Fixture::new();
    fixture.options.busy_budget = Duration::from_millis(10);
    let mut store = fixture.ready();
    let connection = Connection::open(&fixture.options.database).unwrap();
    connection.execute_batch("BEGIN EXCLUSIVE").unwrap();
    for _ in 0..3 {
        store.submit(record(10, 1_000_000, DATE)).unwrap();
    }
    let mut smaller = fixture.options.clone();
    smaller.pending_limit = 1;
    store.reconfigure(smaller).unwrap();
    assert_eq!(store.pending_count(), 3);
    assert_eq!(
        store
            .submit(record(10, 1_000_000, DATE))
            .unwrap_err()
            .error
            .kind,
        ErrorKind::Capacity
    );
    connection.execute_batch("ROLLBACK").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while store.pending_count() != 0 {
        store.poll();
        store.retry_unsaved();
        assert!(Instant::now() < deadline);
        thread::park_timeout(Duration::from_millis(5));
    }
    assert_eq!(
        fixture
            .reader()
            .stats(&Filter::default())
            .unwrap()
            .result_count,
        3
    );
    save(&mut store, record(10, 1_000_000, DATE));
    assert_eq!(
        fixture
            .reader()
            .stats(&Filter::default())
            .unwrap()
            .result_count,
        4
    );
}

#[test]
fn migration_acknowledgement_precedes_saved_result_samples_and_stats() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let result = record(25, 5_000_000, DATE);
    let id = result.id().to_owned();
    let profile = result.snapshot().profile_key.clone();
    assert!(fixture.reader().best(&profile).unwrap().is_none());
    assert_eq!(save(&mut store, result), BestOutcome::NewBest);
    assert_eq!(store.pending_count(), 0);
    let database = Connection::open(&fixture.options.database).unwrap();
    assert_eq!(
        database
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        storage::SQL_SCHEMA_VERSION
    );
    assert_eq!(
        database
            .query_row("SELECT COUNT(*) FROM results", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        database
            .query_row(
                "SELECT COUNT(*) FROM samples WHERE result_id=?",
                [&id],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        database
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let page = fixture
        .reader()
        .history(&Filter::current(&profile), Page::default())
        .unwrap();
    assert_eq!(page.results[0].snapshot.counts.credited_units, 25);
    assert!(page.results[0].snapshot.samples.is_empty());
    assert!(page.results[0].snapshot.words.is_empty());
    assert!(!page.results[0].sparkline.is_empty());
    let review = fixture.reader().review(&id).unwrap().unwrap();
    assert_eq!(review.record.snapshot().samples.len(), 1);
    assert_eq!(review.record.snapshot().words.len(), 1);
    assert!(review.record.snapshot().words[0].entered.is_empty());
    assert_eq!(
        fixture
            .reader()
            .stats(&Filter::current(profile))
            .unwrap()
            .result_count,
        1
    );
}

#[test]
fn replaying_a_save_id_is_idempotent_for_rows_stats_samples_and_bests() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let result = record(50, 10_000_000, DATE);
    let profile = result.snapshot().profile_key.clone();
    assert_eq!(save(&mut store, result.clone()), BestOutcome::NewBest);
    assert_eq!(save(&mut store, result.clone()), BestOutcome::AlreadySaved);
    assert_eq!(save(&mut store, result), BestOutcome::AlreadySaved);
    let stats = fixture.reader().stats(&Filter::current(profile)).unwrap();
    assert_eq!(stats.result_count, 1);
    assert_eq!(stats.attempts_total, 50);
    let connection = Connection::open(&fixture.options.database).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM samples", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn local_best_uses_exact_ratios_and_excludes_every_ineligible_outcome() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let first = record(9_007_199_254_740_992, 9_007_199_254_740_992, DATE);
    let profile = first.snapshot().profile_key.clone();
    assert_eq!(save(&mut store, first), BestOutcome::NewBest);
    assert_eq!(
        save(
            &mut store,
            record(9_007_199_254_740_993, 9_007_199_254_740_992, DATE + 1)
        ),
        BestOutcome::NewBest,
        "one unit beyond f64's exact integer range is a strict improvement"
    );
    assert_eq!(
        save(
            &mut store,
            record(9_007_199_254_740_993, 9_007_199_254_740_992, DATE + 2)
        ),
        BestOutcome::Tied
    );
    assert_eq!(
        save(&mut store, record(100, 1_000_000, DATE + 3)),
        BestOutcome::NotBest
    );
    for index in 0..13 {
        let mut s = snapshot(1_000_000, 1);
        match index {
            0 => s.outcome = Outcome::Aborted,
            1 => s.outcome = Outcome::Failed,
            2 => s.outcome = Outcome::Interrupted,
            3 => s.outcome = Outcome::Incomplete,
            4 => s.spec.explicit_seed = true,
            5 => s.spec.repeated = true,
            6 => s.spec.practice_reason = Some("practice".into()),
            7 => s.integrity.paste_attempted = true,
            8 => s.integrity.input_overload = true,
            9 => s.integrity.clock_interrupted = true,
            10 => s.spec.pace_wpm = Some(100.0),
            11 => {
                s.spec.mode = Mode::Code;
                s.spec.policy = Policy::Exact;
                s.spec.auto_indent = true;
            }
            12 => s.personal_best_eligible = false,
            _ => unreachable!(),
        }
        let result = Record::prepare_at(s, Persistence::default(), None, None, DATE + 10 + index)
            .unwrap()
            .unwrap();
        assert!(
            !result.snapshot().personal_best_eligible,
            "recovery exports must not advertise ineligible records before the writer runs"
        );
        assert_eq!(save(&mut store, result), BestOutcome::Ineligible);
    }
    let best = fixture.reader().best(&profile).unwrap().unwrap();
    assert_eq!(best.credited_units, 9_007_199_254_740_993);
}

#[test]
fn profile_and_filter_isolation_and_weighted_statistics_are_exact() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let first = record(10, 1_000_000, DATE);
    let profile = first.snapshot().profile_key.clone();
    save(&mut store, first);
    let mut second = snapshot(100, 20_000_000);
    second.counts.attempts_total = 200;
    second.counts.attempts_correct = 100;
    second.metrics = Metrics::calculate(second.counts, second.elapsed_us, false, false);
    save(
        &mut store,
        Record::prepare_at(second, Persistence::default(), None, None, DATE + 1000)
            .unwrap()
            .unwrap(),
    );
    let mut different = snapshot(50, 1_000_000);
    different.spec.source_id = "french_200".into();
    different.spec.content_hash = content::bundled_pack("french_200")
        .unwrap()
        .metadata
        .content_hash
        .clone();
    save(
        &mut store,
        Record::prepare_at(different, Persistence::default(), None, None, DATE + 2000)
            .unwrap()
            .unwrap(),
    );
    let stats = fixture.reader().stats(&Filter::current(&profile)).unwrap();
    assert_eq!(stats.result_count, 2);
    assert_eq!(stats.speed_sample_count, 2);
    assert!((stats.aggregate_wpm.unwrap() - 12_000_000.0 * 110.0 / 21_000_000.0).abs() < 1e-9);
    assert!((stats.aggregate_accuracy.unwrap() - 100.0 * 110.0 / 210.0).abs() < 1e-9);
    assert_ne!(
        stats.aggregate_wpm.unwrap(),
        90.0,
        "not an unweighted mean of run speeds"
    );
    let filter = Filter {
        profile_key: Some(profile),
        mode: Some(Mode::Time),
        language: Some("english_200".into()),
        outcome: Some(Outcome::Complete),
        classification: None,
        from_utc_ms: Some(DATE + 500),
        to_utc_ms: Some(DATE + 1500),
    };
    let reader = fixture.reader();
    assert_eq!(
        reader
            .history(&filter, Page::default())
            .unwrap()
            .results
            .len(),
        1
    );
    assert_eq!(reader.stats(&filter).unwrap().attempts_total, 200);
    let page = reader
        .history(
            &Filter::default(),
            Page {
                limit: 2,
                offset: 0,
            },
        )
        .unwrap();
    assert_eq!(page.results.len(), 2);
    assert_eq!(page.next_offset, Some(2));
    assert_eq!(
        reader
            .history(
                &Filter::default(),
                Page {
                    limit: 2,
                    offset: 2
                }
            )
            .unwrap()
            .results
            .len(),
        1
    );
}

#[test]
fn classification_filters_preserve_overlapping_assistance_and_independent_outcomes() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let mut cases = Vec::new();
    for (index, name) in [
        "standard",
        "explicit_seed",
        "repeated",
        "named",
        "paced",
        "paste",
        "assisted_code",
        "assisted_custom",
        "combined",
        "incomplete",
        "zero_duration",
        "plain_failed",
        "paste_failed",
        "zen",
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = snapshot(10, 1_000_000);
        match name {
            "explicit_seed" => value.spec.explicit_seed = true,
            "repeated" => value.spec.repeated = true,
            "named" => value.spec.practice_reason = Some("missed-word practice".into()),
            "paced" => value.spec.pace_wpm = Some(90.0),
            "paste" => value.integrity.paste_attempted = true,
            "assisted_code" | "assisted_custom" | "combined" => {
                value.spec.mode = if name == "assisted_custom" {
                    Mode::Custom
                } else {
                    Mode::Code
                };
                value.spec.policy = Policy::Exact;
                value.spec.auto_indent = true;
                value.integrity.paste_attempted = name == "combined";
            }
            "incomplete" => value.outcome = Outcome::Incomplete,
            "zero_duration" => value.elapsed_us = 0,
            "plain_failed" => value.outcome = Outcome::Failed,
            "paste_failed" => {
                value.outcome = Outcome::Failed;
                value.integrity.paste_attempted = true;
            }
            "zen" => value.spec.mode = Mode::Zen,
            "standard" => {}
            _ => unreachable!(),
        }
        value.metrics = Metrics::calculate(
            value.counts,
            value.elapsed_us,
            value.spec.mode == Mode::Zen,
            false,
        );
        let record = Record::prepare_at(
            value,
            Persistence::default(),
            None,
            None,
            DATE + index as i64,
        )
        .unwrap()
        .unwrap();
        let id = record.id().to_owned();
        save(&mut store, record);
        cases.push((name, id));
    }
    let reader = fixture.reader();
    let matching = |classification, outcome| {
        let filter = Filter {
            classification,
            outcome,
            ..Filter::default()
        };
        let page = reader.history(&filter, Page::default()).unwrap();
        let names: std::collections::BTreeSet<_> = page
            .results
            .iter()
            .map(|entry| cases.iter().find(|(_, id)| id == &entry.id).unwrap().0)
            .collect();
        assert_eq!(
            reader.stats(&filter).unwrap().result_count as usize,
            names.len()
        );
        let mut export = Vec::new();
        assert_eq!(
            reader
                .export_to(&mut export, ExportFormat::Json, &filter, false)
                .unwrap() as usize,
            names.len()
        );
        let exported: serde_json::Value = serde_json::from_slice(&export).unwrap();
        assert_eq!(exported["results"].as_array().unwrap().len(), names.len());
        names
    };
    let set = |values: &[&'static str]| {
        values
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(matching(None, None).len(), cases.len());
    assert_eq!(
        matching(Some(Classification::Standard), None),
        set(&["standard"])
    );
    assert_eq!(
        matching(Some(Classification::Practice), None),
        set(&[
            "explicit_seed",
            "repeated",
            "named",
            "paced",
            "paste",
            "assisted_code",
            "assisted_custom",
            "combined",
            "incomplete",
            "paste_failed",
            "zen",
        ])
    );
    assert_eq!(
        matching(Some(Classification::PasteAttempted), None),
        set(&["paste", "combined", "paste_failed"])
    );
    assert_eq!(
        matching(Some(Classification::AssistedCode), None),
        set(&["assisted_code", "assisted_custom", "combined"])
    );
    assert_eq!(
        matching(Some(Classification::Practice), Some(Outcome::Failed)),
        set(&["paste_failed"])
    );
    assert_eq!(
        matching(None, Some(Outcome::Failed)),
        set(&["plain_failed", "paste_failed"])
    );
    assert_eq!(
        matching(Some(Classification::Practice), Some(Outcome::Incomplete)),
        set(&["incomplete"])
    );
    assert!(matching(Some(Classification::Standard), Some(Outcome::Incomplete)).is_empty());
    assert!(matching(None, Some(Outcome::Complete)).contains("zero_duration"));

    let connection = Connection::open(&fixture.options.database).unwrap();
    let index_count: i64 = connection.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='index' AND name IN ('results_standard_history','results_practice_history','results_paste_history','results_assisted_history')", [], |row| row.get(0)).unwrap();
    assert_eq!(index_count, 4);
    let plan: String = connection.query_row("EXPLAIN QUERY PLAN SELECT id FROM results WHERE json_extract(header_json,'$.integrity.paste_attempted')=1 ORDER BY created_at_utc_ms DESC,id DESC LIMIT 20", [], |row| row.get(3)).unwrap();
    assert!(plan.contains("results_paste_history"), "{plan}");
}

#[test]
fn zen_and_zero_duration_do_not_invent_target_speed_or_accuracy() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let mut s = snapshot(10, 1_000_000);
    s.spec.mode = Mode::Zen;
    s.spec.approved_content = false;
    s.spec.content_hash = String::new();
    s.metrics = Metrics::calculate(s.counts, s.elapsed_us, true, false);
    s.personal_best_eligible = false;
    save(
        &mut store,
        Record::prepare_at(s, Persistence::default(), None, None, DATE)
            .unwrap()
            .unwrap(),
    );
    let stats = fixture
        .reader()
        .stats(&Filter {
            mode: Some(Mode::Zen),
            ..Filter::default()
        })
        .unwrap();
    assert_eq!(stats.aggregate_wpm, None);
    assert_eq!(stats.aggregate_accuracy, None);
    assert_eq!(stats.aggregate_raw_wpm, Some(120.0));
    let mut empty = snapshot(1, 0);
    empty.personal_best_eligible = false;
    save(
        &mut store,
        Record::prepare_at(empty, Persistence::default(), None, None, DATE + 1)
            .unwrap()
            .unwrap(),
    );
    let stats = fixture
        .reader()
        .stats(&Filter {
            mode: Some(Mode::Time),
            ..Filter::default()
        })
        .unwrap();
    assert_eq!(stats.result_count, 1);
    assert_eq!(stats.speed_sample_count, 0);
    assert_eq!(stats.aggregate_wpm, None);
}

#[test]
fn locked_database_retains_bounded_pending_records_without_blocking_submission() {
    let fixture = Fixture::new();
    let mut options = fixture.options.clone();
    options.pending_limit = 2;
    let mut store = Store::start_after_frame(options, thread::current()).unwrap();
    wait(&mut store, |event| matches!(event, Event::Ready { .. }));
    let lock = Connection::open(&fixture.options.database).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let first = record(10, 1_000_000, DATE);
    let id = first.id().to_owned();
    let start = Instant::now();
    let ticket = store.submit(first).unwrap();
    assert!(
        start.elapsed() < Duration::from_millis(100),
        "submitting must not wait for SQLite's250ms busy handler"
    );
    store.submit(record(20, 1_000_000, DATE + 1)).unwrap();
    let rejected = store.submit(record(30, 1_000_000, DATE + 2)).unwrap_err();
    assert_eq!(rejected.error.kind, ErrorKind::Capacity);
    assert_eq!(rejected.record.snapshot().counts.credited_units, 30);
    wait(
        &mut store,
        |event| matches!(event,Event::Unsaved{ticket:found,error} if *found==ticket && error.kind==ErrorKind::Busy),
    );
    assert_eq!(store.pending_count(), 2);
    assert!(
        store.pending().any(
            |(_, record, state)| record.id() == id && matches!(state, PendingState::Unsaved(_))
        )
    );
    let mut recovery = Vec::new();
    export_records(
        &mut recovery,
        ExportFormat::Jsonl,
        store.pending().map(|(_, record, _)| record),
        false,
    )
    .unwrap();
    assert_eq!(String::from_utf8(recovery).unwrap().lines().count(), 2);
    let start = Instant::now();
    let flush = store.flush_for(Duration::from_millis(100));
    assert!(start.elapsed() < Duration::from_millis(200));
    assert_eq!(flush.unsaved.len(), 2);
    lock.execute_batch("ROLLBACK").unwrap();
    store.retry_unsaved();
    let deadline = Instant::now() + Duration::from_secs(3);
    while store.pending_count() > 0 {
        store.poll();
        store.retry_unsaved();
        assert!(Instant::now() < deadline);
        thread::park_timeout(Duration::from_millis(10));
    }
    assert_eq!(
        fixture
            .reader()
            .stats(&Filter::default())
            .unwrap()
            .result_count,
        2
    );
}

#[test]
fn corrupt_and_unknown_schema_databases_are_preserved_and_report_unsaved() {
    for unknown in [false, true] {
        let fixture = Fixture::new();
        #[cfg(windows)]
        clack_private_fs::create_dir_all(fixture.options.database.parent().unwrap()).unwrap();
        #[cfg(not(windows))]
        fs::create_dir_all(fixture.options.database.parent().unwrap()).unwrap();
        if unknown {
            let connection = Connection::open(&fixture.options.database).unwrap();
            connection.execute_batch("CREATE TABLE unrelated(secret TEXT); INSERT INTO unrelated VALUES('private_database_sentinel'); PRAGMA user_version=999;").unwrap();
        } else {
            fs::write(
                &fixture.options.database,
                b"private_database_sentinel-not-a-sqlite-file",
            )
            .unwrap();
        }
        let before = fs::read(&fixture.options.database).unwrap();
        let mut store = fixture.start();
        let events = wait(&mut store, |event| matches!(event, Event::Unavailable(_)));
        let text = format!("{events:?}");
        assert!(!text.contains("private_database_sentinel"));
        let ticket = store.submit(record(10, 1_000_000, DATE)).unwrap();
        wait(
            &mut store,
            |event| matches!(event,Event::Unsaved{ticket:found,..} if *found==ticket),
        );
        assert_eq!(store.pending_count(), 1);
        assert_eq!(fs::read(&fixture.options.database).unwrap(), before);
        assert!(ReadStore::open(&fixture.options.database).is_err());
    }
}

#[test]
fn private_and_default_custom_records_have_no_text_path_or_diagnostics() {
    let fixture = Fixture::new();
    let secret = "private_custom_body_7927";
    let private = Persistence {
        save_results: false,
        store_custom_text: true,
        store_event_trace: true,
    };
    assert!(
        Record::prepare(private_snapshot(secret), private, Some(secret), None)
            .unwrap()
            .is_none()
    );
    assert!(!fixture.options.database.parent().unwrap().exists());
    let result = Record::prepare_at(
        private_snapshot(secret),
        Persistence::default(),
        Some(secret),
        None,
        DATE,
    )
    .unwrap()
    .unwrap();
    assert_eq!(result.snapshot().spec.source_id, "custom");
    assert_eq!(result.snapshot().spec.source_revision, "1");
    assert!(result.snapshot().words.is_empty());
    assert_eq!(result.full_text(), None);
    let hash = result.snapshot().spec.content_hash.clone();
    let id = result.id().to_owned();
    let mut store = fixture.ready();
    save(&mut store, result);
    for format in [ExportFormat::Json, ExportFormat::Jsonl, ExportFormat::Csv] {
        let mut bytes = Vec::new();
        fixture
            .reader()
            .export_to(&mut bytes, format, &Filter::default(), true)
            .unwrap();
        let output = String::from_utf8(bytes).unwrap();
        assert!(output.contains(&hash));
        for private in [
            secret,
            "private_wrong_input",
            "/private/account-source.txt",
            "private-source-path",
        ] {
            assert!(!output.contains(private));
        }
    }
    let review = fixture.reader().review(&id).unwrap().unwrap();
    assert!(matches!(
        review.content,
        ReviewContent::OriginalRequired { .. }
    ));
    let connection = Connection::open(&fixture.options.database).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM private_content", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM word_summaries", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn text_and_event_trace_opt_ins_are_independent_and_exports_require_text_opt_in() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let secret = "=private_formula_123";
    let trace = EventTrace {
        events: vec![TraceEvent {
            received_us: 1,
            sequence: 1,
            kind: TraceKind::Text,
            text: Some("private_trace_391".into()),
        }],
        events_total: 1,
        truncated: false,
    };
    let privacy = Persistence {
        save_results: true,
        store_custom_text: true,
        store_event_trace: true,
    };
    let result = Record::prepare_at(
        private_snapshot(secret),
        privacy,
        Some(secret),
        Some(trace.clone()),
        DATE,
    )
    .unwrap()
    .unwrap();
    let id = result.id().to_owned();
    save(&mut store, result);
    let record = fixture.reader().review(&id).unwrap().unwrap().record;
    assert_eq!(record.full_text(), Some(secret));
    assert!(record.event_trace().is_some());
    for include in [false, true] {
        let mut json = Vec::new();
        fixture
            .reader()
            .export_to(&mut json, ExportFormat::Json, &Filter::default(), include)
            .unwrap();
        let json = String::from_utf8(json).unwrap();
        assert_eq!(json.contains(secret), include);
        assert_eq!(json.contains("private_wrong_input"), include);
        assert_eq!(json.contains("private_trace_391"), include);
        serde_json::from_str::<serde_json::Value>(&json).unwrap();
    }
    let mut csv = Vec::new();
    fixture
        .reader()
        .export_to(&mut csv, ExportFormat::Csv, &Filter::default(), true)
        .unwrap();
    let mut reader = csv::Reader::from_reader(csv.as_slice());
    let row = reader.records().next().unwrap().unwrap();
    assert_eq!(&row[24], "'=private_formula_123");
    let no_trace = Record::prepare_at(
        private_snapshot(secret),
        Persistence {
            store_event_trace: false,
            ..privacy
        },
        Some(secret),
        Some(trace.clone()),
        DATE,
    )
    .unwrap()
    .unwrap();
    assert!(no_trace.event_trace().is_none());
    assert!(no_trace.snapshot().words[0].entered.is_empty());
    let no_text = Record::prepare_at(
        private_snapshot(secret),
        Persistence {
            store_custom_text: false,
            ..privacy
        },
        Some(secret),
        Some(trace),
        DATE,
    )
    .unwrap()
    .unwrap();
    assert!(no_text.full_text().is_none());
    assert!(no_text.snapshot().words.is_empty());
    assert!(no_text.event_trace().is_some());
}

#[test]
fn stored_custom_review_requires_the_same_normalized_source_and_honest_missing_input() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let secret = "café source";
    let result = Record::prepare_at(
        private_snapshot(secret),
        Persistence::default(),
        None,
        None,
        DATE,
    )
    .unwrap()
    .unwrap();
    let id = result.id().to_owned();
    save(&mut store, result);
    let mut review = fixture.reader().review(&id).unwrap().unwrap();
    assert!(review.attach_original(b"wrong private file").is_err());
    assert!(matches!(
        review.content,
        ReviewContent::OriginalRequired { .. }
    ));
    review
        .attach_original("  cafe\u{301}\r\n source ".as_bytes())
        .unwrap();
    assert!(matches!(review.content, ReviewContent::OriginalVerified));
    assert_eq!(review.verified_original.as_deref(), Some(secret));
    assert!(
        review.record.snapshot().words.is_empty(),
        "original target recovery cannot recreate discarded entered text"
    );
}

#[test]
fn hostile_trace_bounds_order_and_nonfinite_metrics_are_rejected() {
    let privacy = Persistence {
        save_results: true,
        store_custom_text: false,
        store_event_trace: true,
    };
    let base = TraceEvent {
        received_us: 2,
        sequence: 2,
        kind: TraceKind::Text,
        text: Some("x".into()),
    };
    for trace in [
        EventTrace {
            events: vec![base.clone(); storage::TRACE_EVENT_LIMIT + 1],
            events_total: (storage::TRACE_EVENT_LIMIT + 1) as u64,
            truncated: false,
        },
        EventTrace {
            events: vec![base.clone(), base.clone()],
            events_total: 2,
            truncated: false,
        },
        EventTrace {
            events: vec![TraceEvent {
                text: Some("x".repeat(storage::TRACE_TEXT_BYTES + 1)),
                ..base.clone()
            }],
            events_total: 1,
            truncated: false,
        },
        EventTrace {
            events: vec![TraceEvent {
                kind: TraceKind::PasteAttempt,
                text: Some("private_paste".into()),
                ..base.clone()
            }],
            events_total: 1,
            truncated: false,
        },
        EventTrace {
            events: vec![base],
            events_total: 100,
            truncated: false,
        },
    ] {
        assert!(
            Record::prepare_at(snapshot(10, 1_000_000), privacy, None, Some(trace), DATE).is_err()
        );
    }
    let mut s = snapshot(10, 1_000_000);
    s.metrics.wpm = Some(f64::NAN);
    assert!(Record::prepare_at(s, Persistence::default(), None, None, DATE).is_err());
}

#[test]
fn entered_orphan_continuations_round_trip_without_relaxing_control_or_grapheme_limits() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let privacy = Persistence {
        store_event_trace: true,
        ..Persistence::default()
    };
    for entered in ["\u{301}", "\u{200d}", "\u{fe0f}", "\u{301}\u{200d}", "\t\n"] {
        let mut snapshot = snapshot(10, 1_000_000);
        snapshot.words[0].entered = entered.into();
        let record = Record::prepare_at(snapshot, privacy, None, None, DATE)
            .unwrap()
            .unwrap();
        let id = record.id().to_owned();
        save(&mut store, record);
        assert_eq!(
            fixture
                .reader()
                .review(&id)
                .unwrap()
                .unwrap()
                .record
                .snapshot()
                .words[0]
                .entered,
            entered
        );
    }
    for entered in [
        "\u{1b}".into(),
        "\u{85}".into(),
        "\u{202e}".into(),
        "\u{0627}".into(),
        "\u{301}".repeat(33),
    ] {
        let mut snapshot = snapshot(10, 1_000_000);
        snapshot.words[0].entered = entered;
        assert!(Record::prepare_at(snapshot, privacy, None, None, DATE).is_err());
    }
}

#[test]
fn retained_zen_text_has_an_explicit_scope_and_accepts_visible_input_continuations() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    for (retained, expected_scope) in [
        (1, storage::TextScope::CompleteOutput),
        (4097, storage::TextScope::RetainedWindow),
    ] {
        let mut snapshot = snapshot(retained, 1_000_000);
        snapshot.spec.mode = Mode::Zen;
        snapshot.spec.approved_content = false;
        snapshot.personal_best_eligible = false;
        let record = Record::prepare_at(
            snapshot,
            Persistence {
                store_custom_text: true,
                ..Persistence::default()
            },
            Some("\u{200d}"),
            None,
            DATE,
        )
        .unwrap()
        .unwrap();
        assert_eq!(record.text_scope(), Some(expected_scope));
        assert!(record.export_view(false).text_scope().is_none());
        let id = record.id().to_owned();
        save(&mut store, record);
        let review = fixture.reader().review(&id).unwrap().unwrap();
        assert_eq!(review.record.text_scope(), Some(expected_scope));
        assert_eq!(review.record.full_text(), Some("\u{200d}"));
    }
}

#[test]
fn a_maximum_legal_custom_token_and_body_can_be_saved_and_read_with_matching_identity() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let text = "a".repeat(content::MAX_CUSTOM_BYTES);
    let privacy = Persistence {
        store_custom_text: true,
        ..Persistence::default()
    };
    assert!(
        Record::prepare_at(
            private_snapshot(&text),
            privacy,
            Some("different"),
            None,
            DATE
        )
        .is_err()
    );
    let record = Record::prepare_at(private_snapshot(&text), privacy, Some(&text), None, DATE)
        .unwrap()
        .unwrap();
    let id = record.id().to_owned();
    save(&mut store, record);
    let review = fixture.reader().review(&id).unwrap().unwrap();
    assert_eq!(
        review.record.text_scope(),
        Some(storage::TextScope::CompleteTarget)
    );
    assert_eq!(review.record.full_text(), Some(text.as_str()));
    assert_eq!(review.record.snapshot().words[0].token, text);
}

#[test]
fn nfc_expanded_legal_custom_body_and_diagnostics_round_trip_and_original_matches() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let source = format!("{}a", "a\u{344}".repeat(content::MAX_CUSTOM_BYTES / 3));
    assert_eq!(source.len(), content::MAX_CUSTOM_BYTES);
    let prepared =
        content::prepare_custom(source.as_bytes(), content::InputPolicy::Prose, false).unwrap();
    assert!(prepared.text.len() > content::MAX_CUSTOM_BYTES);
    let mut snapshot = private_snapshot(&prepared.text);
    snapshot.words[0].entered.clone_from(&prepared.text);
    let record = Record::prepare_at(
        snapshot,
        Persistence {
            store_custom_text: true,
            store_event_trace: true,
            ..Persistence::default()
        },
        Some(&prepared.text),
        None,
        DATE,
    )
    .unwrap()
    .unwrap();
    let id = record.id().to_owned();
    save(&mut store, record);
    let mut review = fixture.reader().review(&id).unwrap().unwrap();
    assert_eq!(review.record.full_text(), Some(prepared.text.as_str()));
    assert_eq!(review.record.snapshot().words[0].token, prepared.text);
    assert_eq!(review.record.snapshot().words[0].entered, prepared.text);
    assert_eq!(review.record.word_summaries_omitted(), 0);
    review.attach_original(source.as_bytes()).unwrap();
    assert_eq!(
        review.verified_original.as_deref(),
        Some(prepared.text.as_str())
    );
    let mut export = Vec::new();
    export_records(&mut export, ExportFormat::Json, [&review.record], true).unwrap();
    let exported: serde_json::Value = serde_json::from_slice(&export).unwrap();
    assert_eq!(exported["results"][0]["full_text"], prepared.text);
}

#[test]
fn an_oversized_opted_in_word_diagnostic_is_explicitly_omitted_without_losing_the_result() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let mut snapshot = snapshot(10, 1_000_000);
    snapshot.words[0].entered = "字".repeat(content::MAX_CANONICAL_CUSTOM_BYTES / 3 + 1);
    let expected_counts = snapshot.counts;
    let record = Record::prepare_at(
        snapshot,
        Persistence {
            store_event_trace: true,
            ..Persistence::default()
        },
        None,
        None,
        DATE,
    )
    .unwrap()
    .unwrap();
    assert!(record.snapshot().words.is_empty());
    assert_eq!(record.word_summaries_omitted(), 1);
    let id = record.id().to_owned();
    save(&mut store, record);
    let review = fixture.reader().review(&id).unwrap().unwrap();
    assert_eq!(review.record.snapshot().counts, expected_counts);
    assert!(review.record.snapshot().words.is_empty());
    assert_eq!(review.record.word_summaries_omitted(), 1);
}

#[test]
fn serialized_diagnostic_budget_is_bounded_and_omissions_are_explicit() {
    let mut snapshot = snapshot(10, 1_000_000);
    let word = clack::engine::WordSummary {
        token: "x".repeat(512),
        ..Default::default()
    };
    let total = 65_000;
    snapshot.words = vec![word; total];
    let record = Record::prepare_at(snapshot, Persistence::default(), None, None, DATE)
        .unwrap()
        .unwrap();
    assert!(record.snapshot().words.len() < total);
    assert_eq!(
        record.snapshot().words.len() as u64 + record.word_summaries_omitted(),
        total as u64
    );
    assert!(
        serde_json::to_vec(record.snapshot().words.as_slice())
            .unwrap()
            .len()
            <= storage::WORD_DIAGNOSTIC_JSON_BYTES + 65_536 + 2
    );
    assert!(serde_json::to_vec(&record).unwrap().len() < 32 * 1024 * 1024);
}

#[test]
fn async_queries_correlate_request_ids_and_do_not_announce_unloaded_bests() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let result = record(25, 5_000_000, DATE);
    let profile = result.snapshot().profile_key.clone();
    let request = store.request_best(&profile).unwrap();
    let events = wait(
        &mut store,
        |event| matches!(event,Event::Best{request_id,..} if *request_id==request),
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event,Event::Best{request_id,best:None} if *request_id==request))
    );
    save(&mut store, result);
    let history = store
        .request_history(Filter::current(&profile), Page::default())
        .unwrap();
    let stats = store.request_stats(Filter::current(profile)).unwrap();
    let mut history_seen = false;
    let mut stats_seen = false;
    let deadline = Instant::now() + Duration::from_secs(3);
    while !history_seen || !stats_seen {
        for event in store.poll() {
            match event {
                Event::History { request_id, page } if request_id == history => {
                    history_seen = true;
                    assert_eq!(page.results.len(), 1);
                }
                Event::Stats {
                    request_id,
                    statistics,
                } if request_id == stats => {
                    stats_seen = true;
                    assert_eq!(statistics.result_count, 1);
                }
                _ => {}
            }
        }
        assert!(Instant::now() < deadline);
        thread::park_timeout(Duration::from_millis(5));
    }
}

#[test]
fn malformed_review_requests_and_tampered_best_ids_are_rejected_without_echoing_text() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let record = record(25, 5_000_000, DATE);
    let profile = record.snapshot().profile_key.clone();
    save(&mut store, record);
    for id in [
        "x".repeat(1024 * 1024),
        format!("\u{1b}{}", "a".repeat(63)),
        "private_id".into(),
    ] {
        let error = store.request_review(&id).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Invalid);
        assert_eq!(error.to_string(), "invalid result ID");
        assert_eq!(
            fixture.reader().review(&id).unwrap_err().kind,
            ErrorKind::Invalid
        );
    }
    let connection = Connection::open(&fixture.options.database).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    connection
        .execute(
            "UPDATE profile_bests SET result_id=?",
            [format!("\u{1b}{}", "a".repeat(63))],
        )
        .unwrap();
    assert_eq!(
        fixture.reader().best(&profile).unwrap_err().kind,
        ErrorKind::Corrupt
    );
}

#[cfg(unix)]
#[test]
fn new_storage_files_are_private_and_final_symlinks_are_refused() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    save(&mut store, record(10, 1_000_000, DATE));
    assert_eq!(
        fs::metadata(&fixture.options.database)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(fixture.options.database.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let other = Fixture::new();
    let target = other._directory.path().join("target.txt");
    fs::write(&target, b"private_unrelated_file").unwrap();
    fs::create_dir_all(other.options.database.parent().unwrap()).unwrap();
    symlink(&target, &other.options.database).unwrap();
    let mut blocked = other.start();
    wait(&mut blocked, |event| matches!(event, Event::Unavailable(_)));
    assert_eq!(fs::read(target).unwrap(), b"private_unrelated_file");
}

#[cfg(windows)]
#[test]
fn windows_history_and_wal_sidecars_inherit_private_acls_and_pin_the_parent() {
    let mut fixture = Fixture::new();
    fixture.options.journal = JournalMode::Wal;
    let mut store = fixture.start();
    let events = wait(&mut store, |event| matches!(event, Event::Ready { .. }));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Ready { journal_mode } if journal_mode == "wal"))
    );
    save(&mut store, record(10, 1_000_000, DATE));
    let parent = fixture.options.database.parent().unwrap();
    drop(clack_private_fs::require_private_directory(parent).unwrap());
    for suffix in ["", "-wal", "-shm"] {
        let mut name = fixture.options.database.as_os_str().to_os_string();
        name.push(suffix);
        let path = std::path::PathBuf::from(name);
        assert!(path.exists());
        clack_private_fs::require_private_file(&path).unwrap();
    }
    let reader = fixture.reader();
    assert_eq!(reader.stats(&Filter::default()).unwrap().result_count, 1);
    let moved = fixture._directory.path().join("moved-private-data");
    assert!(fs::rename(parent, &moved).is_err());
    drop(store);
    assert!(fs::rename(parent, &moved).is_err());
    drop(reader);
    let deadline = Instant::now() + Duration::from_secs(3);
    while fs::rename(parent, &moved).is_err() {
        assert!(
            Instant::now() < deadline,
            "SQLite parent guard was not released"
        );
        thread::park_timeout(Duration::from_millis(5));
    }
}

#[cfg(windows)]
#[test]
fn windows_unsafe_existing_history_directory_is_preserved_and_reports_unsaved() {
    let fixture = Fixture::new();
    let parent = fixture.options.database.parent().unwrap();
    fs::create_dir_all(parent).unwrap();
    assert!(clack_private_fs::require_private_directory(parent).is_err());
    let sentinel = parent.join("unrelated.txt");
    fs::write(&sentinel, b"unrelated_private_fixture").unwrap();
    let mut store = fixture.start();
    let events = wait(&mut store, |event| matches!(event, Event::Unavailable(_)));
    assert!(events.iter().any(|event| matches!(event, Event::Unavailable(error) if error.kind == ErrorKind::Io && error.message.contains("private owned directory"))));
    let ticket = store.submit(record(10, 1_000_000, DATE)).unwrap();
    wait(
        &mut store,
        |event| matches!(event, Event::Unsaved { ticket: found, .. } if *found == ticket),
    );
    assert_eq!(store.pending_count(), 1);
    assert!(!fixture.options.database.exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"unrelated_private_fixture");
    assert!(clack_private_fs::require_private_directory(parent).is_err());

    // Even a read-only open can make SQLite sidecars, so present unsafe history
    // is refused before SQLite reads it. Missing history still creates nothing.
    assert_eq!(
        fixture
            .reader()
            .stats(&Filter::default())
            .unwrap()
            .result_count,
        0
    );
    fs::write(&fixture.options.database, b"unrelated_private_database").unwrap();
    let error = ReadStore::open(&fixture.options.database).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Io);
    assert!(error.message.contains("private owned directory"));
    assert_eq!(
        fs::read(&fixture.options.database).unwrap(),
        b"unrelated_private_database"
    );
}

#[test]
fn caller_and_worker_release_owned_pending_records_after_commit_or_shutdown() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    save(&mut store, record(10, 1_000_000, DATE));
    let flush = store.flush_for(Duration::from_millis(100));
    assert!(flush.unsaved.is_empty());
    let lock = Connection::open(&fixture.options.database).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    store.submit(record(20, 1_000_000, DATE + 1)).unwrap();
    let report = store.flush_for(Duration::from_millis(1));
    assert_eq!(report.unsaved.len(), 1);
    let retained = Arc::clone(&report.unsaved[0]);
    let weak = Arc::downgrade(&retained);
    drop(report);
    drop(retained);
    drop(store);
    lock.execute_batch("ROLLBACK").unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while weak.upgrade().is_some() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "run explicitly in release mode; records a 100,000-row history benchmark"]
fn history_100000_rows_reports_first_page_and_summary_p95() {
    let fixture = Fixture::new();
    let mut store = fixture.ready();
    let first = record(25, 5_000_000, DATE);
    let profile = first.snapshot().profile_key.clone();
    save(&mut store, first);
    drop(store);
    let connection = Connection::open(&fixture.options.database).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    connection.execute("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<99999) INSERT INTO results SELECT printf('%064x',i),created_at_utc_ms+i,profile_key,mode,source_id,outcome,eligible,elapsed_us,credited_units,retained_units,attempts_total,attempts_correct,deletion_count,final_correct,final_incorrect,final_extra,final_missed,header_json,sparkline_json,word_summaries_omitted FROM n CROSS JOIN (SELECT * FROM results LIMIT 1)",[]).unwrap();
    connection.execute("UPDATE profile_stats SET result_count=result_count*100000,speed_sample_count=speed_sample_count*100000,target_speed_sample_count=target_speed_sample_count*100000,elapsed_us=elapsed_us*100000,target_elapsed_us=target_elapsed_us*100000,credited_units=credited_units*100000,retained_units=retained_units*100000,attempts_total=attempts_total*100000,attempts_correct=attempts_correct*100000,target_attempts_total=target_attempts_total*100000,target_attempts_correct=target_attempts_correct*100000",[]).unwrap();
    connection.execute_batch("COMMIT").unwrap();
    let plan:String=connection.query_row("EXPLAIN QUERY PLAN SELECT id FROM results WHERE profile_key=? ORDER BY created_at_utc_ms DESC,id DESC LIMIT 20",[&profile],|row|row.get(3)).unwrap();
    assert!(plan.contains("results_profile_history"));
    let reader = fixture.reader();
    let filter = Filter::current(profile);
    let mut history = Vec::new();
    let mut stats = Vec::new();
    for index in 0..105 {
        let before = Instant::now();
        let page = reader.history(&filter, Page::default()).unwrap();
        let elapsed = before.elapsed().as_micros();
        assert_eq!(page.results.len(), 20);
        let before = Instant::now();
        let summary = reader.stats(&filter).unwrap();
        let elapsed_stats = before.elapsed().as_micros();
        assert_eq!(summary.result_count, 100000);
        if index >= 5 {
            history.push(elapsed);
            stats.push(elapsed_stats);
        }
    }
    history.sort_unstable();
    stats.sort_unstable();
    println!(
        "{{\"schema_version\":1,\"rows\":100000,\"samples\":100,\"warmups\":5,\"profile_page_p95_us\":{},\"profile_summary_p95_us\":{},\"sqlite_version\":\"{}\",\"debug_assertions\":{},\"controlled_reference\":false}}",
        history[94],
        stats[94],
        rusqlite::version(),
        cfg!(debug_assertions)
    );
}
