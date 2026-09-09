use clack::{
    cli::{Cli, Command, ConfigCommand, LanguageCommand},
    config::{Config, Paths},
    sample::Samples,
    storage::{self, ExportFormat, Filter, Page, ReadStore},
};
use clap::{CommandFactory, Parser};
use std::{
    fs,
    io::{self, Write},
    panic::{self, AssertUnwindSafe},
    path::Path,
    process::ExitCode,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug)]
struct Failure {
    code: u8,
    message: String,
}
impl Failure {
    fn argument(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }
    fn runtime(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: message.into(),
        }
    }
}
impl From<io::Error> for Failure {
    fn from(value: io::Error) -> Self {
        Self::runtime(value.to_string())
    }
}
impl From<storage::StorageError> for Failure {
    fn from(error: storage::StorageError) -> Self {
        Self {
            code: if error.kind == storage::ErrorKind::Invalid {
                2
            } else {
                1
            },
            message: error.to_string(),
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(failure) => {
            // A closed diagnostic stream must not turn a handled failure into
            // an unrelated panic after terminal restoration.
            let _ = writeln!(io::stderr().lock(), "clack: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}
fn execute(cli: &Cli) -> Result<u8, Failure> {
    cli.validate_command_contract().map_err(Failure::argument)?;
    // Help, man pages, and completions remain usable even with a broken config.
    match &cli.command {
        Some(Command::Completions { shell }) => {
            let mut bytes = Vec::new();
            clap_complete::generate(*shell, &mut Cli::command(), "clack", &mut bytes);
            io::stdout().lock().write_all(&bytes)?;
            return Ok(0);
        }
        Some(Command::Man) => {
            clap_mangen::Man::new(Cli::command()).render(&mut io::stdout())?;
            return Ok(0);
        }
        _ => {}
    }
    let paths = Paths::resolve(cli.config.as_deref(), cli.data_dir.as_deref())
        .map_err(Failure::argument)?;
    if matches!(
        cli.command,
        Some(Command::Config {
            command: ConfigCommand::Path
        })
    ) {
        if cli.json {
            write_json(&paths)?;
        } else {
            write_text(&format!(
                "config: {}\ndata: {}\ndatabase: {}\nlanguages: {}\n",
                paths.config.display(),
                paths.data.display(),
                paths.database.display(),
                paths.languages.display()
            ))?;
        }
        return Ok(0);
    }
    let mut notice = None;
    let mut config = if paths.config.exists() || cli.config.is_some() {
        match Config::read(&paths.config) {
            Ok(config) => config,
            Err(error) if cli.config.is_none() && cli.command.is_none() => {
                notice = Some(format!(
                    "Configuration not loaded: {error}. Using defaults."
                ));
                Config::default()
            }
            Err(error) => return Err(Failure::argument(error)),
        }
    } else {
        Config::default()
    };
    let stored_config = config.clone();
    cli.apply(&mut config).map_err(Failure::argument)?;
    if let Some(command) = &cli.command {
        return command_line(cli, command, &config, &stored_config, &paths);
    }
    // Establish an independent interactive channel before consuming source stdin.
    let _channel = clack::terminal::controlling_terminal().map_err(|_| {
        Failure::runtime("no suitable controlling terminal; interactive tests need a terminal")
    })?;
    let samples = Samples::prepare(cli, &config, &paths).map_err(Failure::argument)?;
    // Guards unwind first. Report a neutral panic diagnostic only after restoration.
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let run = panic::catch_unwind(AssertUnwindSafe(|| {
        clack::app::run(cli, config, paths, samples, notice)
    }));
    panic::set_hook(previous_hook);
    let output = match run {
        Ok(result) => result.map_err(Failure::runtime)?,
        Err(_) => {
            return Err(Failure::runtime(
                "an internal error interrupted the session; terminal restoration was attempted",
            ));
        }
    };
    if output.unsaved_count > 0 {
        let _ = writeln!(
            io::stderr().lock(),
            "clack: {} result(s) remain unsaved after the bounded storage flush",
            output.unsaved_count
        );
    }
    if let (Some(path), Some(bench)) = (&cli.benchmark_output, &output.benchmark) {
        let file = private_output(path)?;
        serde_json::to_writer_pretty(file, bench).map_err(|error| {
            Failure::runtime(format!("cannot write performance observations: {error}"))
        })?;
    }
    if cli.json {
        if let Some(result) = &output.result {
            write_json(result)?;
        } else {
            return Err(Failure {
                code: output.exit_code as u8,
                message: "session ended before any result was available".into(),
            });
        }
    }
    Ok(output.exit_code as u8)
}
fn command_line(
    cli: &Cli,
    command: &Command,
    config: &Config,
    stored: &Config,
    paths: &Paths,
) -> Result<u8, Failure> {
    match command {
        Command::Config { command } => match command {
            ConfigCommand::Path => unreachable!(),
            ConfigCommand::Validate => {
                if cli.json {
                    write_json(&serde_json::json!({"schema_version":1,"valid":true}))?;
                } else {
                    write_text("Configuration is valid (schema 1).\n")?;
                }
            }
            ConfigCommand::Show { resolved } => {
                let shown = if *resolved { config } else { stored };
                if cli.json {
                    write_json(shown)?;
                } else {
                    write_text(
                        &toml::to_string_pretty(shown)
                            .map_err(|error| Failure::runtime(error.to_string()))?,
                    )?;
                }
            }
        },
        Command::Languages {
            command: LanguageCommand::List,
        } => {
            let mut packs = Vec::new();
            for id in clack::content::BUNDLED_PACK_IDS {
                packs.push(
                    clack::content::bundled_pack(id)
                        .map_err(|error| Failure::runtime(error.to_string()))?
                        .metadata
                        .clone(),
                );
            }
            if paths.languages.is_dir() {
                let mut entries = fs::read_dir(&paths.languages)?.collect::<Result<Vec<_>, _>>()?;
                entries.sort_by_key(|entry| entry.file_name());
                for entry in entries {
                    if entry.file_type()?.is_dir()
                        && let Some(id) = entry.file_name().to_str()
                    {
                        if id.starts_with('.') || clack::content::BUNDLED_PACK_IDS.contains(&id) {
                            continue;
                        }
                        packs.push(
                            clack::sample::load_pack(id, paths)
                                .map_err(Failure::runtime)?
                                .metadata
                                .clone(),
                        );
                    }
                }
            }
            if cli.json {
                write_json(&packs)?;
            } else {
                for pack in packs {
                    write_text(&format!(
                        "{}\t{} words\t{}\t{}\n",
                        pack.id, pack.token_count, pack.revision, pack.license
                    ))?;
                }
            }
        }
        Command::Themes { .. } => {
            if cli.json {
                write_json(&clack::ui::THEMES)?;
            } else {
                for theme in clack::ui::THEMES {
                    write_text(&format!("{theme}\n"))?;
                }
            }
        }
        Command::Doctor { probe } => {
            let terminal = clack::terminal::controlling_terminal().is_ok();
            let size = if terminal {
                clack::terminal::terminal_size().ok()
            } else {
                None
            };
            let active_probe = if *probe {
                Some(clack::terminal::probe_capabilities().map_err(Failure::runtime)?)
            } else {
                None
            };
            let support = active_probe
                .as_ref()
                .and_then(|report| report.keyboard_enhancement_support);
            let input_mode = if !config.enhanced_keyboard {
                "baseline"
            } else {
                match support {
                    Some(true) => "enhanced requested; terminal support detected",
                    Some(false) => "baseline fallback; enhancements requested but unsupported",
                    None => "enhanced requested; support unverified",
                }
            };
            let interrupted = active_probe
                .as_ref()
                .is_some_and(|report| report.interrupted);
            let report = serde_json::json!({"app_version":env!("CARGO_PKG_VERSION"),"schema_version":1,"build":{"debug_assertions":cfg!(debug_assertions),"test_hooks_enabled":cfg!(feature="test-hooks"),"target_arch":std::env::consts::ARCH,"target_os":std::env::consts::OS,"rust_version_minimum":env!("CARGO_PKG_RUST_VERSION")},"paths":paths,"terminal_available":terminal,"terminal_size":size,"input_mode":input_mode,"color_policy":config.appearance.color,"color_depth":format!("{:?}",clack::ui::ColorDepth::detect(config.appearance.color)),"capabilities":{"source":"operating-system geometry, environment color policy, and compiled backend; active protocol queries require --probe","bracketed_paste_requested":true,"focus_reporting_requested":true,"mouse_capture":false,"keyboard_queries_on_startup":false,"keyboard_enhancement_support":support,"active_probe":active_probe,"associated_text_decoder":true,"native_console":cfg!(windows),"controlling_tty":cfg!(unix)},"journal_mode":config.storage.journal_mode,"history_exists":paths.database.exists(),"private_session":config.privacy.private_session});
            write_json(&report)?;
            if interrupted {
                return Ok(130);
            }
        }
        Command::History(args) => {
            let store = ReadStore::open(&paths.database)?;
            let filter = history_filter(
                cli,
                config,
                paths,
                &store,
                &args.profile,
                args.mode.as_deref(),
                args.language.as_deref(),
                args.outcome.as_deref(),
                args.classification.as_deref(),
                args.from.as_deref(),
                args.to.as_deref(),
            )?;
            let page = store.history(
                &filter,
                Page {
                    limit: args.limit,
                    offset: args.offset,
                },
            )?;
            if cli.json {
                write_json(
                    &serde_json::json!({"export_version":1,"filter":filter,"results":page.results,"next_offset":page.next_offset}),
                )?;
            } else {
                let mut output = io::stdout().lock();
                if page.results.is_empty() {
                    writeln!(output, "No matching results.")?;
                }
                for entry in page.results {
                    let snapshot = &entry.snapshot;
                    writeln!(
                        output,
                        "{}\t{}\t{} wpm\t{}%\t{}\t{}\t{}",
                        entry.id,
                        entry.created_at_utc,
                        clack::ui::metric(snapshot.metrics.wpm, 1),
                        clack::ui::metric(snapshot.metrics.accuracy, 1),
                        snapshot.spec.source_id.escape_default(),
                        enum_name(&snapshot.outcome),
                        if snapshot.personal_best_eligible {
                            "eligible"
                        } else {
                            "practice / non-record"
                        }
                    )?;
                }
                if let Some(offset) = page.next_offset {
                    writeln!(output, "Next page: --offset {offset}")?;
                }
            }
        }
        Command::Stats(args) => {
            let store = ReadStore::open(&paths.database)?;
            let filter = history_filter(
                cli,
                config,
                paths,
                &store,
                &args.profile,
                args.mode.as_deref(),
                args.language.as_deref(),
                args.outcome.as_deref(),
                args.classification.as_deref(),
                args.from.as_deref(),
                args.to.as_deref(),
            )?;
            let statistics = store.stats(&filter)?;
            if cli.json {
                write_json(
                    &serde_json::json!({"export_version":1,"filter":filter,"statistics":statistics}),
                )?;
            } else {
                write_text(&format!(
                    "{} results · {} target-speed samples · {} output-speed samples\nAggregate WPM: {}\nAggregate raw WPM: {}\nAggregate accuracy: {}%\n",
                    statistics.result_count,
                    statistics.target_speed_sample_count,
                    statistics.speed_sample_count,
                    clack::ui::metric(statistics.aggregate_wpm, 1),
                    clack::ui::metric(statistics.aggregate_raw_wpm, 1),
                    clack::ui::metric(statistics.aggregate_accuracy, 1)
                ))?;
            }
        }
        Command::Export(args) => {
            let store = ReadStore::open(&paths.database)?;
            let filter = history_filter(
                cli,
                config,
                paths,
                &store,
                &args.profile,
                args.mode.as_deref(),
                args.language.as_deref(),
                args.outcome.as_deref(),
                args.classification.as_deref(),
                args.from.as_deref(),
                args.to.as_deref(),
            )?;
            let format = ExportFormat::parse(&args.format)?;
            if let Some(path) = &args.output {
                let mut file = private_output(path).map_err(|_| {
                    Failure::runtime("cannot create export file; choose a new writable path")
                })?;
                let result = store
                    .export_to(&mut file, format, &filter, args.include_text)
                    .map_err(Failure::from)
                    .and_then(|count| file.sync_all().map(|()| count).map_err(Failure::from));
                drop(file);
                match result {
                    Ok(count) => {
                        writeln!(io::stderr().lock(), "clack: exported {count} result(s)")?
                    }
                    Err(error) => {
                        let _ = fs::remove_file(path);
                        return Err(error);
                    }
                }
            } else {
                store.export_to(io::stdout().lock(), format, &filter, args.include_text)?;
            }
        }
        Command::Languages {
            command: LanguageCommand::Import { path },
        } => {
            let (metadata, existing) = import_language(path, paths)?;
            if cli.json {
                write_json(
                    &serde_json::json!({"schema_version":1,"language":metadata,"already_installed":existing}),
                )?;
            } else {
                write_text(&format!(
                    "{} {} ({} words, revision {})\n",
                    if existing {
                        "Already installed"
                    } else {
                        "Installed"
                    },
                    metadata.id,
                    metadata.token_count,
                    metadata.revision
                ))?;
            }
        }
        Command::Completions { .. } | Command::Man => unreachable!(),
    }
    Ok(0)
}
fn enum_name(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}
#[allow(clippy::too_many_arguments)]
fn history_filter(
    cli: &Cli,
    config: &Config,
    paths: &Paths,
    store: &ReadStore,
    profile: &str,
    mode: Option<&str>,
    language: Option<&str>,
    outcome: Option<&str>,
    classification: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Filter, Failure> {
    let profile_key = match profile {
        "all" => None,
        "current" => Some(current_profile(cli, config, paths, store)?),
        key if key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some(key.to_ascii_lowercase())
        }
        _ => {
            return Err(Failure::argument(
                "--profile expects current, all, or a 64-digit profile key",
            ));
        }
    };
    fn choice<T: serde::de::DeserializeOwned>(value: &str, path: &str) -> Result<T, Failure> {
        serde_json::from_value(serde_json::Value::String(value.into()))
            .map_err(|_| Failure::argument(format!("{path}: unsupported value")))
    }
    if let Some(id) = language
        && (id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')))
    {
        return Err(Failure::argument(
            "--language filter expects a stable source ID",
        ));
    }
    let filter = Filter {
        profile_key,
        mode: mode.map(|value| choice(value, "--mode")).transpose()?,
        language: language.map(str::to_owned),
        outcome: outcome
            .map(|value| choice(value, "--outcome"))
            .transpose()?,
        classification: classification
            .map(|value| choice(value, "--classification"))
            .transpose()?,
        from_utc_ms: from
            .map(|value| storage::parse_utc_bound(value, false))
            .transpose()?,
        to_utc_ms: to
            .map(|value| storage::parse_utc_bound(value, true))
            .transpose()?,
    };
    if filter
        .from_utc_ms
        .zip(filter.to_utc_ms)
        .is_some_and(|(from, to)| from > to)
    {
        return Err(Failure::argument("--from must not be later than --to"));
    }
    Ok(filter)
}
fn current_profile(
    cli: &Cli,
    config: &Config,
    paths: &Paths,
    store: &ReadStore,
) -> Result<String, Failure> {
    if config.test.mode == clack::engine::Mode::Quote && config.test.quote_id.is_none() {
        return Err(Failure::argument(
            "quote current profile needs --quote-id; use --profile all or an explicit profile key otherwise",
        ));
    }
    let mut prepared = Samples::prepare(cli, config, paths).map_err(Failure::argument)?;
    let sample = prepared.next(config).map_err(Failure::argument)?;
    let mut spec = sample.engine.spec().clone();
    if config.practice.pace == "personal_best" {
        let best=store.best(&spec.profile_key())?.ok_or_else(||Failure::argument("personal_best pace has no matching saved best; choose a fixed --pace or an explicit --profile key"))?;
        spec.pace_wpm = Some(best.wpm());
        spec.validate().map_err(Failure::argument)?;
    }
    Ok(spec.profile_key())
}
static IMPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn import_language(
    source: &Path,
    paths: &Paths,
) -> Result<(clack::content::PackMetadata, bool), Failure> {
    fn read(path: &Path, limit: usize) -> Result<Vec<u8>, Failure> {
        let file = fs::File::open(path)
            .map_err(|_| Failure::argument("cannot read language pack words.txt/metadata.json"))?;
        clack::sample::read_bounded(file, limit)
            .map_err(|error| Failure::argument(error.to_string()))
    }
    let words = read(&source.join("words.txt"), clack::content::MAX_PACK_BYTES)?;
    let metadata = read(&source.join("metadata.json"), 64 * 1024)?;
    let pack = clack::content::LanguagePack::from_parts(&words, &metadata)
        .map_err(|error| Failure::argument(error.to_string()))?;
    if clack::content::BUNDLED_PACK_IDS.contains(&pack.metadata.id.as_str()) {
        return Err(Failure::argument(
            "bundled language IDs cannot be replaced; choose a new pack ID",
        ));
    }
    let target = paths.languages.join(&pack.metadata.id);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            let existing = clack::sample::load_pack(&pack.metadata.id, paths).map_err(|_| {
                Failure::argument(
                    "language ID already exists but is not a valid matching pack; choose a new ID",
                )
            })?;
            if existing.metadata == pack.metadata
                && existing.canonical_words() == pack.canonical_words()
            {
                return Ok((pack.metadata, true));
            }
            return Err(Failure::argument(
                "language ID is already installed with different metadata/content; choose a new ID",
            ));
        }
        Ok(_) => {
            return Err(Failure::argument(
                "language ID already exists as a nonregular or linked entry",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(Failure::runtime(
                "cannot inspect installed language directory",
            ));
        }
    }
    #[cfg(windows)]
    clack_private_fs::create_dir_all(&paths.languages)
        .map_err(|_| Failure::runtime("cannot create private language directory"))?;
    #[cfg(not(windows))]
    {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&paths.languages)
            .map_err(|_| Failure::runtime("cannot create private language directory"))?;
    }
    let temporary = (0..100)
        .find_map(|_| {
            let temporary = paths.languages.join(format!(
                ".clack-import-{}-{}",
                std::process::id(),
                IMPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            #[cfg(windows)]
            let created = clack_private_fs::create_dir(&temporary);
            #[cfg(not(windows))]
            let created = {
                let builder = fs::DirBuilder::new();
                #[cfg(unix)]
                let mut builder = builder;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&temporary)
            };
            match created {
                Ok(()) => Some(Ok(temporary)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(_) => Some(Err(Failure::runtime(
                    "cannot create temporary language directory",
                ))),
            }
        })
        .ok_or_else(|| Failure::runtime("cannot allocate a temporary language directory"))??;
    let result = (|| {
        let mut words = private_output(&temporary.join("words.txt"))?;
        words.write_all(pack.canonical_words().as_bytes())?;
        words.sync_all()?;
        drop(words);
        let mut metadata = private_output(&temporary.join("metadata.json"))?;
        serde_json::to_writer_pretty(&mut metadata, &pack.metadata)
            .map_err(|_| Failure::runtime("cannot serialize language metadata"))?;
        writeln!(metadata)?;
        metadata.sync_all()?;
        drop(metadata);
        if fs::symlink_metadata(&target).is_ok() {
            return Err(Failure::argument(
                "language ID appeared during import; it was not overwritten",
            ));
        }
        fs::rename(&temporary, &target).map_err(|_| {
            Failure::runtime("cannot publish language pack; existing packs are preserved")
        })?;
        #[cfg(unix)]
        if let Ok(directory) = fs::File::open(&paths.languages) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    Ok((pack.metadata, false))
}
fn write_text(text: &str) -> Result<(), Failure> {
    io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .map_err(Failure::from)
}
fn write_json(value: &impl serde::Serialize) -> Result<(), Failure> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, value)
        .map_err(|error| Failure::runtime(error.to_string()))?;
    writeln!(output)?;
    Ok(())
}
fn private_output(path: &std::path::Path) -> io::Result<fs::File> {
    #[cfg(windows)]
    {
        clack_private_fs::create_new_file(path)
    }
    #[cfg(not(windows))]
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path)
    }
}
