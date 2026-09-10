//! Actual executable contracts without a controlling terminal. Storage fixtures
//! use the real engine, privacy boundary, and SQLite worker.
use clack::{
    cli::Cli,
    config::{Config, Paths},
    content::{self, InputPolicy, PackMetadata},
    engine::{Action, Mode, Outcome},
    sample::Samples,
    storage::{self, Event, Options, Persistence, Record, Store},
};
use clap::Parser;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    directory: tempfile::TempDir,
    paths: Paths,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("data");
        let paths = Paths {
            config: directory.path().join("config.toml"),
            database: data.join("history.sqlite3"),
            languages: data.join("languages"),
            data,
        };
        fs::write(&paths.config, "schema_version=1\n").unwrap();
        Self { directory, paths }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_clack"));
        command
            .arg("--config")
            .arg(&self.paths.config)
            .arg("--data-dir")
            .arg(&self.paths.data)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1");
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    fn json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "unexpected diagnostic {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.contains(&27));
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn record(&self, args: &[&str], date: &str, outcome: Outcome, privacy: Persistence) -> Record {
        let cli =
            Cli::try_parse_from(std::iter::once("clack").chain(args.iter().copied())).unwrap();
        let mut config = Config::default();
        cli.apply(&mut config).unwrap();
        let mut samples = Samples::prepare(&cli, &config, &self.paths).unwrap();
        let mut sample = samples.next(&config).unwrap();
        if sample.engine.spec().mode == Mode::Time {
            let text = sample.text.split(' ').take(8).collect::<Vec<_>>().join(" ");
            sample.engine.apply(Action::Text(&text), 100);
            match outcome {
                Outcome::Complete => sample.engine.apply(
                    Action::Tick,
                    u64::from(config.test.seconds) * 1_000_000 + 100,
                ),
                Outcome::Interrupted => sample
                    .engine
                    .apply(Action::Interrupt("synthetic interruption"), 1_000_100),
                _ => panic!("unsupported fixture outcome"),
            };
        } else {
            let first = sample.text.chars().next().unwrap().len_utf8();
            sample
                .engine
                .apply(Action::Text(&sample.text[..first]), 100);
            sample
                .engine
                .apply(Action::Text(&sample.text[first..]), 500_100);
            if sample.engine.spec().policy == clack::engine::Policy::Exact {
                sample.engine.apply(Action::Finish, 600_100);
            }
        }
        Record::prepare_at(
            sample.engine.snapshot(),
            privacy,
            Some(&sample.text),
            None,
            storage::parse_utc_bound(date, false).unwrap(),
        )
        .unwrap()
        .unwrap()
    }
    fn save(&self, records: Vec<Record>) {
        let mut worker =
            Store::start_after_frame(Options::new(self.paths.database.clone()), thread::current())
                .unwrap();
        for record in records {
            worker.submit(record).unwrap();
        }
        // Fixture setup must await acknowledgements, not use the production
        // exit flush, which deliberately caps every caller's budget at 750 ms.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            for event in worker.poll() {
                match event {
                    Event::Unavailable(error) | Event::Unsaved { error, .. } => {
                        panic!("fixture failed to save: {error}");
                    }
                    _ => {}
                }
            }
            if worker.pending_count() == 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "fixture timed out awaiting save acknowledgements: {:?}",
                worker.pending().collect::<Vec<_>>()
            );
            thread::park_timeout(Duration::from_millis(5));
        }
    }
    fn pack(&self, id: &str, words: &str) -> PathBuf {
        let source = self.directory.path().join(format!("source-{id}"));
        fs::create_dir(&source).unwrap();
        let metadata = PackMetadata {
            schema_version: 1,
            id: id.into(),
            language_tag: "en".into(),
            revision: "1".into(),
            source: "Authored synthetic CLI fixture".into(),
            license: "CC0-1.0".into(),
            content_hash: content::content_hash(words.as_bytes()),
            text_direction: "ltr".into(),
            supported_input_policy: InputPolicy::Prose,
            token_count: words.lines().count(),
        };
        fs::write(source.join("words.txt"), words).unwrap();
        fs::write(
            source.join("metadata.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        source
    }
}

#[test]
fn missing_history_commands_are_empty_and_do_not_create_data() {
    let f = Fixture::new();
    let history = f.json(&["history", "--json"]);
    assert_eq!(history["export_version"], 1);
    assert_eq!(history["results"], serde_json::json!([]));
    assert!(history["next_offset"].is_null());
    let stats = f.json(&["stats", "--profile", "current", "--json"]);
    assert_eq!(stats["statistics"]["result_count"], 0);
    assert!(stats["statistics"]["aggregate_wpm"].is_null());
    assert!(stats["statistics"]["aggregate_accuracy"].is_null());
    let exported = f.json(&["export", "--format", "json"]);
    assert_eq!(exported["results"], serde_json::json!([]));
    let jsonl = f.run(&["export", "--format", "jsonl"]);
    assert!(jsonl.status.success() && jsonl.stdout.is_empty());
    let csv = f.run(&["export", "--format", "csv"]);
    assert!(csv.status.success());
    assert!(csv.stdout.starts_with(b"export_version,id,created_at_utc"));
    assert!(!f.paths.data.exists());
}

#[test]
fn configuration_independent_commands_reject_interactive_only_options() {
    let f = Fixture::new();
    fs::write(&f.paths.config, "deliberately broken configuration").unwrap();
    for command in [
        vec!["config", "path"],
        vec!["man"],
        vec!["completions", "bash"],
    ] {
        for flags in [
            vec!["--stdin"],
            vec!["--once"],
            vec!["--benchmark-output", "unused.json"],
        ] {
            let args = flags
                .into_iter()
                .chain(command.iter().copied())
                .collect::<Vec<_>>();
            let result = f.run(&args);
            assert_eq!(result.status.code(), Some(2), "args={args:?}");
            assert!(result.stdout.is_empty(), "args={args:?}");
            assert!(!result.stderr.contains(&27));
            assert!(!String::from_utf8_lossy(&result.stderr).contains("broken configuration"));
        }
    }
    assert!(!f.paths.data.exists());
}

#[test]
fn all_profile_queries_do_not_require_a_missing_configured_source() {
    let f = Fixture::new();
    let record = f.record(
        &["--time", "1"],
        "2026-09-04T12:00:00Z",
        Outcome::Complete,
        Persistence::default(),
    );
    f.save(vec![record]);
    let mut config = Config::default();
    config.test.mode = Mode::Custom;
    config.test.file = Some(f.directory.path().join("private-missing-source.txt"));
    let bytes = toml::to_string(&config).unwrap();
    fs::write(&f.paths.config, &bytes).unwrap();

    let history = f.json(&["history", "--profile", "all", "--json"]);
    assert_eq!(history["results"].as_array().unwrap().len(), 1);
    let stats = f.json(&["stats", "--profile", "all", "--json"]);
    assert_eq!(stats["statistics"]["result_count"], 1);
    let export = f.json(&["export", "--format", "json", "--profile", "all"]);
    assert_eq!(export["results"].as_array().unwrap().len(), 1);
    let current = f.run(&["history", "--profile", "current", "--json"]);
    assert_eq!(current.status.code(), Some(2));
    assert!(current.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&current.stderr).contains("private-missing-source"));
    assert_eq!(fs::read_to_string(&f.paths.config).unwrap(), bytes);
}

#[test]
fn history_current_uses_prepared_pack_identity_and_paginates_with_filters() {
    let f = Fixture::new();
    let first = f.record(&[], "2026-09-01", Outcome::Complete, Persistence::default());
    let profile = first.snapshot().profile_key.clone();
    f.save(vec![
        first,
        f.record(
            &[],
            "2026-09-02",
            Outcome::Interrupted,
            Persistence::default(),
        ),
        f.record(
            &["--time", "60"],
            "2026-09-03",
            Outcome::Complete,
            Persistence::default(),
        ),
    ]);
    let first = f.json(&["history", "--limit", "1", "--json"]);
    assert_eq!(first["filter"]["profile_key"], profile);
    assert_eq!(first["results"].as_array().unwrap().len(), 1);
    assert_eq!(first["results"][0]["snapshot"]["outcome"], "interrupted");
    assert_eq!(first["next_offset"], 1);
    let second = f.json(&["history", "--offset", "1", "--limit", "1", "--json"]);
    assert_eq!(second["results"][0]["snapshot"]["outcome"], "complete");
    assert!(second["next_offset"].is_null());
    let all = f.json(&[
        "history",
        "--profile",
        "all",
        "--from",
        "2026-09-01",
        "--to",
        "2026-09-02",
        "--mode",
        "time",
        "--language",
        "english_200",
        "--outcome",
        "complete",
        "--json",
    ]);
    assert_eq!(all["results"].as_array().unwrap().len(), 1);
    let other = f.json(&["--time", "60", "history", "--json"]);
    assert_eq!(other["results"].as_array().unwrap().len(), 1);
    assert_eq!(other["results"][0]["snapshot"]["spec"]["seconds"], 60);
}

#[test]
fn statistics_are_weighted_and_show_sample_counts() {
    let f = Fixture::new();
    let a = f.record(&[], "2026-09-01", Outcome::Complete, Persistence::default());
    let b = f.record(
        &["--time", "60"],
        "2026-09-02",
        Outcome::Complete,
        Persistence::default(),
    );
    let credited = a.snapshot().counts.credited_units + b.snapshot().counts.credited_units;
    let duration = a.snapshot().elapsed_us + b.snapshot().elapsed_us;
    f.save(vec![a, b]);
    let stats = f.json(&["stats", "--profile", "all", "--json"]);
    assert_eq!(stats["statistics"]["result_count"], 2);
    assert_eq!(stats["statistics"]["target_speed_sample_count"], 2);
    let actual = stats["statistics"]["aggregate_wpm"].as_f64().unwrap();
    assert!((actual - (12_000_000.0 * credited as f64 / duration as f64)).abs() < 1e-10);
    let filtered = f.json(&[
        "stats",
        "--profile",
        "all",
        "--from",
        "2026-09-02",
        "--to",
        "2026-09-02",
        "--json",
    ]);
    assert_eq!(filtered["statistics"]["result_count"], 1);
    let text = f.run(&["stats", "--profile", "all"]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("2 results"));
}

#[test]
fn classification_filters_are_independent_of_outcome_across_history_stats_and_exports() {
    let f = Fixture::new();
    let standard = f.record(
        &["--time", "1"],
        "2026-09-01",
        Outcome::Complete,
        Persistence::default(),
    );
    let standard_id = standard.id().to_owned();
    let practice = f.record(
        &["--time", "1", "--seed", "42"],
        "2026-09-02",
        Outcome::Complete,
        Persistence::default(),
    );
    let assisted = f.record(
        &["--code", "--text", "a b", "--auto-indent"],
        "2026-09-03",
        Outcome::Complete,
        Persistence::default(),
    );
    let interrupted = f.record(
        &["--time", "30"],
        "2026-09-04",
        Outcome::Interrupted,
        Persistence::default(),
    );
    assert_eq!(interrupted.snapshot().outcome, Outcome::Interrupted);
    let paste = {
        let cli = Cli::try_parse_from(["clack", "--time", "1"]).unwrap();
        let mut config = Config::default();
        cli.apply(&mut config).unwrap();
        let mut sources = Samples::prepare(&cli, &config, &f.paths).unwrap();
        let mut sample = sources.next(&config).unwrap();
        let text = sample.text.split(' ').take(8).collect::<Vec<_>>().join(" ");
        let first = text.chars().next().unwrap().len_utf8();
        sample.engine.apply(Action::Text(&text[..first]), 100);
        sample.engine.apply(Action::Paste, 200);
        sample.engine.apply(Action::Text(&text[first..]), 500_100);
        sample.engine.apply(Action::Tick, 1_000_100);
        Record::prepare_at(
            sample.engine.snapshot(),
            Persistence::default(),
            None,
            None,
            storage::parse_utc_bound("2026-09-05", false).unwrap(),
        )
        .unwrap()
        .unwrap()
    };
    assert!(paste.snapshot().integrity.paste_attempted);
    f.save(vec![standard, practice, assisted, interrupted, paste]);
    for (classification, count) in [
        ("standard", 1),
        ("practice", 3),
        ("paste_attempted", 1),
        ("assisted_code", 1),
    ] {
        let history = f.json(&[
            "history",
            "--profile",
            "all",
            "--classification",
            classification,
            "--json",
        ]);
        assert_eq!(history["filter"]["classification"], classification);
        assert_eq!(
            history["results"].as_array().unwrap().len(),
            count,
            "classification={classification}"
        );
        if classification == "standard" {
            assert_eq!(history["results"][0]["id"], standard_id);
        }
        let stats = f.json(&[
            "stats",
            "--profile",
            "all",
            "--classification",
            classification,
            "--json",
        ]);
        assert_eq!(stats["statistics"]["result_count"], count);
        let exported = f.json(&[
            "export",
            "--format",
            "json",
            "--classification",
            classification,
        ]);
        assert_eq!(exported["results"].as_array().unwrap().len(), count);
    }
    let complete = f.json(&[
        "history",
        "--profile",
        "all",
        "--classification",
        "practice",
        "--outcome",
        "complete",
        "--json",
    ]);
    assert_eq!(complete["results"].as_array().unwrap().len(), 3);
    let interrupted = f.json(&[
        "history",
        "--profile",
        "all",
        "--outcome",
        "interrupted",
        "--json",
    ]);
    assert_eq!(interrupted["results"].as_array().unwrap().len(), 1);
    let intersection = f.json(&[
        "history",
        "--profile",
        "all",
        "--classification",
        "practice",
        "--outcome",
        "interrupted",
        "--json",
    ]);
    assert!(intersection["results"].as_array().unwrap().is_empty());
    let lines = f.run(&[
        "export",
        "--format",
        "jsonl",
        "--classification",
        "practice",
    ]);
    assert!(lines.status.success());
    assert_eq!(String::from_utf8(lines.stdout).unwrap().lines().count(), 3);
    let csv = f.run(&["export", "--format", "csv", "--classification", "practice"]);
    assert!(csv.status.success());
    assert_eq!(
        csv::Reader::from_reader(&csv.stdout[..]).records().count(),
        3
    );
}

#[test]
fn personal_best_pace_current_profile_uses_only_the_matching_indexed_best() {
    let f = Fixture::new();
    let absent = f.run(&["--pace", "personal_best", "stats", "--json"]);
    assert_eq!(absent.status.code(), Some(2));
    let record = f.record(&[], "2026-09-01", Outcome::Complete, Persistence::default());
    let pace = record.snapshot().metrics.wpm.unwrap().to_string();
    let paced = f.record(
        &["--pace", &pace],
        "2026-09-02",
        Outcome::Complete,
        Persistence::default(),
    );
    let expected = paced.snapshot().profile_key.clone();
    f.save(vec![record, paced]);
    let actual = f.json(&["--pace", "personal_best", "stats", "--json"]);
    assert_eq!(actual["filter"]["profile_key"], expected);
    assert_eq!(actual["statistics"]["result_count"], 1);
}

#[test]
fn date_profile_and_query_validation_return_argument_errors() {
    let f = Fixture::new();
    for args in [
        vec!["history", "--profile", "garbage"],
        vec!["history", "--from", "2026-02-30"],
        vec!["stats", "--to", "yesterday"],
        vec!["history", "--from", "2026-09-03", "--to", "2026-09-01"],
        vec!["history", "--limit", "0"],
        vec!["history", "--offset", "18446744073709551615"],
        vec!["history", "--mode", "duck"],
        vec!["export", "--outcome", "success"],
        vec!["history", "--classification", "record-ish"],
        vec!["stats", "--classification", "failed"],
        vec!["export", "--classification", "all"],
        vec!["history", "--language", "../../private"],
    ] {
        let output = f.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    assert!(!f.paths.data.exists());
}

#[test]
fn fixed_source_current_profile_matches_without_recording_source_paths() {
    let f = Fixture::new();
    let source = f.directory.path().join("PRIVATE_FILE_NAME.txt");
    fs::write(&source, "private fixture passage").unwrap();
    let path = source.to_str().unwrap();
    let record = f.record(
        &["--file", path],
        "2026-09-01",
        Outcome::Complete,
        Persistence::default(),
    );
    let key = record.snapshot().profile_key.clone();
    f.save(vec![record]);
    let history = f.json(&["--file", path, "history", "--json"]);
    assert_eq!(history["filter"]["profile_key"], key);
    assert_eq!(history["results"].as_array().unwrap().len(), 1);
    let text = serde_json::to_string(&history).unwrap();
    assert!(!text.contains("PRIVATE_FILE_NAME") && !text.contains("private fixture passage"));
}

#[test]
fn unknown_quote_or_unresolved_current_quote_is_neutral_and_all_scope_works() {
    let f = Fixture::new();
    let output = f.run(&["--quote", "history", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("quote-id"));
    f.json(&["--quote", "history", "--profile", "all", "--json"]);
    let output = f.run(&[
        "--quote-id",
        "PRIVATE_INVALID_QUOTE_SENTINEL",
        "history",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_INVALID_QUOTE_SENTINEL"));
}

#[test]
fn command_stdin_is_never_consumed_as_source() {
    let f = Fixture::new();
    // Keep the writer open without supplying EOF. A command must reject --stdin
    // immediately, rather than reading until EOF as an interactive source.
    let mut command = f.command(&["--stdin", "history", "--json"]);
    command.stdin(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            panic!("noninteractive command consumed/waited for source stdin");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let _ = input.write_all(b"private source remains unused");
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn export_privacy_defaults_and_explicit_available_text_are_consistent() {
    let f = Fixture::new();
    let text = "PRIVATE_CLI_SOURCE_SENTINEL";
    let record = f.record(
        &["--text", text],
        "2026-09-01",
        Outcome::Complete,
        Persistence {
            store_custom_text: true,
            ..Persistence::default()
        },
    );
    f.save(vec![record]);
    for format in ["json", "jsonl", "csv"] {
        let output = f.run(&["export", "--format", format]);
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains(text));
        let output = f.run(&["export", "--format", format, "--include-text"]);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(text));
        assert!(!output.stdout.contains(&27));
    }
}

#[test]
fn export_never_invents_text_that_was_not_saved() {
    let f = Fixture::new();
    let text = "PRIVATE_NEVER_SAVED_SENTINEL";
    f.save(vec![f.record(
        &["--text", text],
        "2026-09-01",
        Outcome::Complete,
        Persistence::default(),
    )]);
    let result = f.json(&["export", "--format", "json", "--include-text"]);
    assert!(!serde_json::to_string(&result).unwrap().contains(text));
}

#[test]
fn export_files_are_new_only_private_and_reserved_stdout_stays_empty() {
    let f = Fixture::new();
    let path = f.directory.path().join("results.json");
    let output = f.run(&[
        "export",
        "--format",
        "json",
        "--output",
        path.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let content = fs::read(&path).unwrap();
    let _: Value = serde_json::from_slice(&content).unwrap();
    #[cfg(windows)]
    clack_private_fs::require_private_file(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let again = f.run(&[
        "export",
        "--format",
        "json",
        "--output",
        path.to_str().unwrap(),
    ]);
    assert_eq!(again.status.code(), Some(1));
    assert_eq!(fs::read(&path).unwrap(), content);
}

#[cfg(windows)]
#[test]
fn windows_export_rejects_stream_and_device_destinations_without_payloads() {
    let f = Fixture::new();
    for name in [
        "results.json:private",
        "results.json::$DATA",
        "NUL",
        "COM¹.txt",
        "aliased.",
    ] {
        let destination = f.directory.path().join(name);
        let output = f.run(&[
            "export",
            "--format",
            "json",
            "--output",
            destination.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(1), "{name}");
        assert!(output.stdout.is_empty());
    }
    assert_eq!(fs::read_dir(f.directory.path()).unwrap().count(), 1);
    assert!(!f.paths.data.exists());
}

#[test]
fn corrupt_history_is_preserved_with_neutral_runtime_failure() {
    let f = Fixture::new();
    #[cfg(windows)]
    clack_private_fs::create_dir(&f.paths.data).unwrap();
    #[cfg(not(windows))]
    fs::create_dir(&f.paths.data).unwrap();
    let content = b"PRIVATE_CORRUPT_HISTORY_SENTINEL";
    #[cfg(windows)]
    clack_private_fs::create_new_file(&f.paths.database)
        .unwrap()
        .write_all(content)
        .unwrap();
    #[cfg(not(windows))]
    fs::write(&f.paths.database, content).unwrap();
    for command in ["history", "stats", "export"] {
        let output = f.run(&[command, "--json"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("PRIVATE_CORRUPT_HISTORY_SENTINEL")
        );
        assert_eq!(fs::read(&f.paths.database).unwrap(), content);
    }
}

#[test]
fn language_import_is_validated_atomic_idempotent_and_user_only() {
    let f = Fixture::new();
    let source = f.pack("tiny_cli", "alpha\nbeta\ngamma\n");
    let path = source.to_str().unwrap();
    let imported = f.json(&["languages", "import", path, "--json"]);
    assert_eq!(imported["language"]["id"], "tiny_cli");
    assert_eq!(imported["already_installed"], false);
    let target = f.paths.languages.join("tiny_cli");
    assert_eq!(
        fs::read_to_string(target.join("words.txt")).unwrap(),
        "alpha\nbeta\ngamma\n"
    );
    let again = f.json(&["languages", "import", path, "--json"]);
    assert_eq!(again["already_installed"], true);
    let list = f.json(&["languages", "list", "--json"]);
    assert_eq!(
        list.as_array()
            .unwrap()
            .iter()
            .filter(|p| p["id"] == "tiny_cli")
            .count(),
        1
    );
    assert_eq!(fs::read_dir(&f.paths.languages).unwrap().count(), 1);
    #[cfg(windows)]
    {
        clack_private_fs::require_private_directory(&target).unwrap();
        for file in ["words.txt", "metadata.json"] {
            clack_private_fs::require_private_file(&target.join(file)).unwrap();
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for file in ["words.txt", "metadata.json"] {
            assert_eq!(
                fs::metadata(target.join(file))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn invalid_language_import_never_publishes_or_executes_pack_files() {
    let f = Fixture::new();
    let source = f.pack("bad_cli", "alpha\nbeta\n");
    fs::write(
        source.join("words.txt"),
        b"PRIVATE_BAD_PACK_SENTINEL\x1b[31m\n",
    )
    .unwrap();
    fs::write(source.join("install.sh"), "exit 99\n").unwrap();
    let output = f.run(&["languages", "import", source.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_BAD_PACK_SENTINEL"));
    assert!(!f.paths.languages.exists());
}

#[test]
fn conflicting_language_id_and_bundled_shadow_do_not_overwrite() {
    let f = Fixture::new();
    let source = f.pack("same_cli", "alpha\nbeta\n");
    f.json(&["languages", "import", source.to_str().unwrap(), "--json"]);
    let target = f.paths.languages.join("same_cli");
    let original = fs::read(target.join("metadata.json")).unwrap();
    let mut metadata: Value =
        serde_json::from_slice(&fs::read(source.join("metadata.json")).unwrap()).unwrap();
    metadata["revision"] = "2".into();
    fs::write(
        source.join("metadata.json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    let output = f.run(&["languages", "import", source.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(target.join("metadata.json")).unwrap(), original);
    let bundled = f.pack("english_200", "alpha\nbeta\n");
    let output = f.run(&["languages", "import", bundled.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!f.paths.languages.join("english_200").exists());
}

#[test]
fn config_commands_preserve_cli_override_precedence_and_files() {
    let f = Fixture::new();
    let original = "[test]\nseconds=45\n";
    fs::write(&f.paths.config, original).unwrap();
    let stored = f.json(&["--time", "90", "config", "show", "--json"]);
    assert_eq!(stored["test"]["seconds"], 45);
    let resolved = f.json(&[
        "--time",
        "90",
        "--tab-stop",
        "8",
        "--ascii-markers",
        "config",
        "show",
        "--resolved",
        "--json",
    ]);
    assert_eq!(resolved["test"]["seconds"], 90);
    assert_eq!(resolved["appearance"]["tab_stop"], 8);
    assert_eq!(resolved["appearance"]["ascii_markers"], true);
    f.json(&["config", "validate", "--json"]);
    let paths = f.json(&["config", "path", "--json"]);
    assert_eq!(paths["config"], f.paths.config.to_str().unwrap());
    assert_eq!(fs::read_to_string(&f.paths.config).unwrap(), original);
    assert!(!f.paths.data.exists());
}

#[test]
fn broken_explicit_config_fails_without_rewrite_but_help_stays_available() {
    let f = Fixture::new();
    let original = "[appearance]\nwidth=999\n";
    fs::write(&f.paths.config, original).unwrap();
    let output = f.run(&["config", "validate", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("appearance.width"));
    for args in [
        vec!["--help"],
        vec!["--version"],
        vec!["man"],
        vec!["completions", "bash"],
        vec!["config", "path", "--json"],
    ] {
        let output = f.run(&args);
        assert!(
            output.status.success(),
            "{args:?}: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.is_empty());
    }
    assert_eq!(fs::read_to_string(&f.paths.config).unwrap(), original);
    assert!(!f.paths.data.exists());
}

#[test]
fn doctor_reports_observed_limits_without_terminal_or_protocol_claims() {
    let f = Fixture::new();
    let report = f.json(&["doctor", "--json"]);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["input_mode"], "baseline");
    assert!(report["capabilities"]["keyboard_enhancement_support"].is_null());
    assert_eq!(report["capabilities"]["keyboard_queries_on_startup"], false);
    assert!(!f.paths.data.exists());
}

#[test]
#[cfg(unix)]
fn closed_output_streams_return_handled_exit_codes_without_panicking() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};

    fn closed_stream() -> Stdio {
        let (reader, writer) = UnixStream::pair().unwrap();
        drop(reader);
        Stdio::from(OwnedFd::from(writer))
    }

    let f = Fixture::new();
    for args in [vec!["completions", "zsh"], vec!["man"], vec!["doctor"]] {
        let output = f.command(&args).stdout(closed_stream()).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
    let output = f
        .command(&["--minimum-accuracy", "101"])
        .stderr(closed_stream())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!f.paths.data.exists());
}

#[test]
fn source_flags_and_literal_policy_are_validated_before_interactive_setup() {
    let f = Fixture::new();
    for args in [
        vec!["--time", "0"],
        vec!["--words", "10001"],
        vec!["--time", "30", "--words", "20"],
        vec!["--text", "x", "--stdin"],
        vec!["--quote", "--exact"],
        vec!["--time", "30", "--normalize-exact"],
        vec!["--json"],
        vec!["--tab-stop", "0"],
    ] {
        let output = f.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
    }
    let source = f.directory.path().join("code.txt");
    fs::write(&source, "fn main() {}\n").unwrap();
    let code = f.json(&[
        "--code",
        "--file",
        source.to_str().unwrap(),
        "config",
        "show",
        "--resolved",
        "--json",
    ]);
    assert_eq!(code["test"]["mode"], "code");
    assert_eq!(code["test"]["policy"], "exact");
}
