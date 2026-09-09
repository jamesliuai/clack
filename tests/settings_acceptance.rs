//! Settings acceptance tests exercise user-visible precedence and file outcomes.
use clack::{
    cli::Cli,
    config::Config,
    engine::{Backspace, Completion, Mode, Policy, StopOnError},
    settings::{self, BindingContext, Bindings, CommandAction, Edit, Kind},
    ui::{Focus, Width},
};
use clap::{CommandFactory, Parser};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::{collections::BTreeSet, fs};

fn edit(path: &str, value: &str) -> Edit {
    Edit::parse(path, value).unwrap()
}

#[cfg(windows)]
#[test]
fn windows_configuration_replacement_keeps_an_owner_only_dacl() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    // An existing document can inherit its parent's ACL. An explicit edit must
    // publish the new protected file, including on a second atomic replacement.
    fs::write(&path, "# keep this note\nschema_version=1\n").unwrap();
    let mut config = Config::read(&path).unwrap();
    for seconds in [60, 120] {
        config
            .persist_edits(&path, &[edit("test.seconds", &seconds.to_string())])
            .unwrap();
        clack_private_fs::require_private_file(&path).unwrap();
        assert_eq!(Config::read(&path).unwrap().test.seconds, seconds);
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("# keep this note")
        );
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn windows_configuration_rejects_alternate_streams_without_creating_a_temp_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml:private");
    let mut config = Config::default();
    assert!(
        config
            .persist_edits(&path, &[edit("test.seconds", "60")])
            .is_err()
    );
    assert_eq!(config.test.seconds, 30);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn registry_covers_every_serialized_setting_and_canonical_default() {
    let config = Config::default();
    config.validate().unwrap();
    let value = toml::Value::try_from(&config).unwrap();
    let mut paths = BTreeSet::new();
    for setting in settings::REGISTRY {
        assert!(paths.insert(setting.path), "duplicate {}", setting.path);
        assert!(!setting.label.is_empty() && !setting.help.is_empty() && !setting.group.is_empty());
        assert_eq!(setting.value(&config), setting.default_value());
        setting.validate(setting.default_value().as_ref()).unwrap();
    }
    fn visit(value: &toml::Value, prefix: &str) {
        if let Some(setting) = settings::find(prefix)
            && matches!(setting.kind, Kind::Bindings | Kind::Presets)
        {
            return;
        }
        if let Some(table) = value.as_table() {
            for (key, value) in table {
                visit(
                    value,
                    &if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    },
                );
            }
        } else {
            assert!(
                settings::find(prefix).is_some(),
                "unregistered leaf {prefix}"
            );
        }
    }
    visit(&value, "");
    for group in [
        "test", "display", "status", "rules", "practice", "workflow", "privacy",
    ] {
        assert!(
            settings::REGISTRY
                .iter()
                .any(|s| s.group == group && s.ordinary)
        );
    }
    assert_eq!(config.test.seconds, 30);
    assert_eq!(config.test.language, "english_200");
    assert!(!config.test.punctuation && !config.test.numbers);
    assert_eq!(config.rules.backspace, Backspace::Mistakes);
    assert!(config.status.progress && !config.status.wpm && !config.status.accuracy);
    assert_eq!(config.appearance.lines, 3);
    assert_eq!(config.appearance.width, Width::Auto);
}

#[test]
fn cli_enumerations_cannot_drift_from_the_shared_registry() {
    let cli = Cli::command();
    for (argument, path) in [
        ("completion", "test.completion"),
        ("length", "test.quote_length"),
        ("difficulty", "rules.difficulty"),
        ("backspace", "rules.backspace"),
        ("stop_on_error", "rules.stop_on_error"),
        ("focus", "appearance.focus"),
        ("alignment", "appearance.alignment"),
        ("caret", "appearance.caret"),
        ("color", "appearance.color"),
        ("speed_unit", "status.speed_unit"),
    ] {
        let argument = cli
            .get_arguments()
            .find(|a| a.get_id() == argument)
            .unwrap();
        let values: Vec<_> = argument
            .get_value_parser()
            .possible_values()
            .unwrap()
            .map(|v| v.get_name().to_owned())
            .collect();
        assert_eq!(
            values,
            settings::find(path).unwrap().choices(),
            "choice drift at {path}"
        );
    }
}

#[test]
fn canonical_spec_configuration_loads_and_unknown_paths_are_precise() {
    let config = Config::from_text(
        r#"
schema_version=1
[test]
mode="time"
seconds=30
words=50
language="english_200"
punctuation=false
numbers=false
[appearance]
theme="terminal"
focus="auto" # preserve me
width=72
alignment="center"
lines=3
line_spacing=0
caret="bar"
[status]
progress=true
wpm=false
accuracy=false
speed_unit="wpm"
[rules]
difficulty="normal"
backspace="mistakes"
stop_on_error="off"
blind=false
[practice]
pace="off"
auto_indent=false
[privacy]
save_results=true
store_custom_text=false
store_event_trace=false
"#,
    )
    .unwrap();
    assert_eq!(config.appearance.width, Width::Cells(72));
    let error = Config::from_text("[test]\nsecondz=31\n").unwrap_err();
    assert!(
        error.contains("test.secondz") && error.contains("31"),
        "{error}"
    );
    let error =
        Config::from_text("[test.generator_parameters]\nnumber_min_digits=9\n").unwrap_err();
    assert!(
        error.contains("test.generator_parameters.number_min_digits") && error.contains('9'),
        "{error}"
    );
}

#[test]
fn all_generator_and_advanced_fields_validate_even_when_inactive() {
    for (path, bad) in [
        ("test.generator_parameters.sentence_min_words", "0"),
        ("test.generator_parameters.sentence_max_words", "65"),
        ("test.generator_parameters.comma_percent", "101"),
        ("test.generator_parameters.number_percent", "-1"),
        ("test.generator_parameters.number_min_digits", "0"),
        ("test.generator_parameters.number_max_digits", "5"),
        ("storage.pending_limit", "0"),
        ("storage.journal_mode", "delete_everything"),
        ("practice.pace", "NaN"),
        ("appearance.tab_stop", "0"),
        ("rules.minimum_accuracy", "101"),
        ("rules.minimum_wpm", "inf"),
    ] {
        assert!(Edit::parse(path, bad).is_err(), "accepted {path}={bad}");
    }
    assert!(
        Config::default()
            .edited(&[
                edit("test.generator_parameters.sentence_min_words", "20"),
                edit("test.generator_parameters.sentence_max_words", "12"),
            ])
            .is_err()
    );
    let configured = Config::default()
        .edited(&[
            edit("test.generator_parameters.number_percent", "25"),
            edit("storage.journal_mode", "delete"),
            edit("storage.pending_limit", "8"),
            edit("enhanced_keyboard", "true"),
            edit("appearance.ascii_markers", "true"),
        ])
        .unwrap();
    assert_eq!(configured.test.generator_parameters.number_percent, 25);
    assert!(configured.enhanced_keyboard && configured.appearance.ascii_markers);
}

#[test]
fn combined_edits_are_transactional_and_allow_valid_cross_field_changes() {
    let config = Config::default();
    assert!(config.edited(&[edit("test.mode", "code")]).is_err());
    let code = config
        .edited(&[edit("test.mode", "code"), edit("test.policy", "exact")])
        .unwrap();
    assert_eq!(code.test.mode, Mode::Code);
    assert_eq!(code.test.policy, Policy::Exact);
    assert_eq!(config, Config::default());
    assert!(
        config
            .edited(&[
                edit("rules.stop_on_error", "word"),
                edit("rules.backspace", "none")
            ])
            .is_err()
    );
    let threshold = config.edited(&[edit("rules.minimum_wpm", "32.5")]).unwrap();
    assert_eq!(threshold.rules.minimum_wpm, Some(32.5));
    assert_eq!(
        threshold
            .edited(&[edit("rules.minimum_wpm", "unset")])
            .unwrap()
            .rules
            .minimum_wpm,
        None
    );
}

#[test]
fn parsing_rejects_unknown_fields_and_toml_value_injection() {
    assert!(Edit::parse("test.run_shell", "rm").is_err());
    assert!(Edit::parse("status.wpm", "true\n[privacy]\nsave_results=false").is_err());
    assert!(Edit::parse("test.file", "\"\u{1b}[31m\"").is_err());
    assert!(Config::from_text("[test]\nlanguage='../escape'\n").is_err());
    let error = Config::from_text("[\"bad\\u001bname\"]\nx=1").unwrap_err();
    assert!(!error.contains('\u{1b}'));
}

#[test]
fn precedence_is_defaults_then_file_then_preset_then_cli_then_ui() {
    let mut config = Config::from_text(
        "[test]\nseconds=45\n[appearance]\ntheme='dark'\n[presets.training.test]\nseconds=90\n",
    )
    .unwrap();
    let cli = Cli::try_parse_from([
        "clack", "--preset", "training", "--time", "120", "--theme", "warm",
    ])
    .unwrap();
    cli.apply(&mut config).unwrap();
    assert_eq!(config.test.seconds, 120);
    assert_eq!(config.appearance.theme, "warm");
    let next = config.edited(&[edit("test.seconds", "60")]).unwrap();
    assert_eq!(next.test.seconds, 60);
    assert_eq!(next.appearance.theme, "warm");
}

#[test]
fn edited_fields_persist_without_cli_overrides_and_preserve_comments() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let original = "# overall explanation\nschema_version = 1\n\n[test] # test note\nseconds = 30 # timer note\n\n[appearance]\ntheme = 'dark' # color note\n\n[status]\nwpm = false # speed note\n";
    fs::write(&path, original).unwrap();
    let mut session = Config::read(&path).unwrap();
    session.test.seconds = 120;
    session.appearance.theme = "warm".into();
    session
        .persist_edits(&path, &[edit("status.wpm", "true")])
        .unwrap();
    let disk = Config::read(&path).unwrap();
    assert_eq!(disk.test.seconds, 30);
    assert_eq!(disk.appearance.theme, "dark");
    assert!(disk.status.wpm && session.status.wpm);
    assert_eq!(session.test.seconds, 120);
    let text = fs::read_to_string(&path).unwrap();
    for comment in [
        "# overall explanation",
        "# test note",
        "# timer note",
        "# color note",
        "# speed note",
    ] {
        assert!(text.contains(comment), "lost {comment}: {text}");
    }
    assert!(text.contains("seconds = 30 # timer note"));
    assert!(text.contains("theme = 'dark' # color note"));
    assert!(text.contains("wpm = true # speed note"));
}

#[test]
fn inline_table_edits_preserve_neighbor_values_and_comments() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    fs::write(
        &path,
        "# header\nstatus = { progress = true, wpm = false } # inline note\n",
    )
    .unwrap();
    let mut config = Config::read(&path).unwrap();
    config
        .persist_edits(&path, &[edit("status.wpm", "true")])
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# header") && text.contains("# inline note"));
    assert!(Config::read(&path).unwrap().status.progress);
    assert!(Config::read(&path).unwrap().status.wpm);
}

#[test]
fn invalid_effective_or_disk_combination_never_partially_commits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let original = "[rules]\nstop_on_error='word'\n";
    fs::write(&path, original).unwrap();
    let mut config = Config::read(&path).unwrap();
    let before = config.clone();
    assert!(
        config
            .persist_edits(&path, &[edit("rules.backspace", "none")])
            .is_err()
    );
    assert_eq!(config, before);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    // CLI override makes the effective edit valid, but it remains invalid for
    // saved defaults. The transaction must fail rather than silently save CLI.
    config.rules.stop_on_error = StopOnError::Off;
    let before = config.clone();
    assert!(
        config
            .persist_edits(&path, &[edit("rules.backspace", "none")])
            .is_err()
    );
    assert_eq!(config, before);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    config
        .persist_edits(
            &path,
            &[
                edit("rules.stop_on_error", "off"),
                edit("rules.backspace", "none"),
            ],
        )
        .unwrap();
    assert_eq!(
        Config::read(&path).unwrap().rules.backspace,
        Backspace::None
    );
}

#[test]
fn failed_io_and_broken_config_are_not_repaired_or_applied() {
    let directory = tempfile::tempdir().unwrap();
    let blocked = directory.path().join("file-as-directory");
    fs::write(&blocked, "keep me").unwrap();
    let mut config = Config::default();
    assert!(
        config
            .persist_edits(&blocked.join("config.toml"), &[edit("status.wpm", "true")])
            .is_err()
    );
    assert_eq!(config, Config::default());
    assert_eq!(fs::read_to_string(&blocked).unwrap(), "keep me");
    let path = directory.path().join("broken.toml");
    let broken = "# preserve broken evidence\n[status\nwpm='maybe'\n";
    fs::write(&path, broken).unwrap();
    assert!(
        config
            .persist_edits(&path, &[edit("status.wpm", "true")])
            .is_err()
    );
    assert!(config.save_defaults(&path).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    assert_eq!(config, Config::default());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn builtin_presets_are_data_and_failed_custom_presets_leave_session_unchanged() {
    let mut config = Config::default();
    config.preset("focused").unwrap();
    assert_eq!(config.appearance.focus, Focus::Always);
    assert!(!config.status.progress && !config.status.wpm && !config.status.accuracy);
    config.preset("code").unwrap();
    assert_eq!(config.test.policy, Policy::Exact);
    assert_eq!(config.test.completion, Completion::Confirm);
    config.presets.insert(
        "invalid".into(),
        toml::from_str("[test]\nmode='time'").unwrap(),
    );
    let before = config.clone();
    assert!(config.preset("invalid").is_err());
    assert_eq!(config, before);
    assert!(Config::from_text("[presets.default.test]\nseconds=12").is_err());
    assert!(Config::from_text("[presets.evil]\ncommand='run a shell'").is_err());
    assert!(Config::from_text("[presets.evil.presets.nested.test]\nseconds=10").is_err());
}

#[test]
fn applying_preset_persists_explicit_fields_even_if_cli_already_matches() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    fs::write(
        &path,
        "[appearance]\nfocus='auto'\n[status]\nprogress=true\n",
    )
    .unwrap();
    let mut config = Config::read(&path).unwrap();
    config.appearance.focus = Focus::Always;
    config.status.progress = false;
    config.test.seconds = 120;
    config.persist_preset(&path, "focused").unwrap();
    let saved = Config::read(&path).unwrap();
    assert_eq!(saved.appearance.focus, Focus::Always);
    assert!(!saved.status.progress);
    assert_eq!(saved.test.seconds, 30);
    assert_eq!(config.test.seconds, 120);
}

#[test]
fn saving_named_preset_and_explicit_defaults_preserves_existing_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    fs::write(&path,"# file note\n[test]\nseconds=30 # duration note\n[presets.older.test]\nseconds=15 # old preset note\n").unwrap();
    let mut config = Config::read(&path).unwrap();
    config.test.seconds = 120;
    config.appearance.theme = "warm".into();
    config.save_preset(&path, "training").unwrap();
    let mut saved = Config::read(&path).unwrap();
    assert_eq!(saved.test.seconds, 30);
    assert!(saved.presets.contains_key("older"));
    saved.preset("training").unwrap();
    assert_eq!(saved.test.seconds, 120);
    assert_eq!(saved.appearance.theme, "warm");
    config.save_defaults(&path).unwrap();
    assert_eq!(Config::read(&path).unwrap().test.seconds, 120);
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("# file note")
            && text.contains("# duration note")
            && text.contains("# old preset note")
    );
    assert!(!config.presets["training"].contains_key("presets"));
}

#[test]
fn baseline_bindings_work_by_context_and_releases_never_execute() {
    let bindings = Bindings::default();
    for (letter, action) in [
        ('C', CommandAction::Quit),
        ('R', CommandAction::NewSample),
        ('P', CommandAction::Palette),
        ('W', CommandAction::DeleteWord),
    ] {
        assert_eq!(
            bindings.resolve(
                &KeyEvent::new(
                    KeyCode::Char(letter),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT
                ),
                BindingContext::Running,
            ),
            Some(action),
        );
    }
    for (code, modifiers, action) in [
        (KeyCode::Esc, KeyModifiers::NONE, CommandAction::Palette),
        (
            KeyCode::Char('r'),
            KeyModifiers::CONTROL,
            CommandAction::NewSample,
        ),
        (
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            CommandAction::Quit,
        ),
        (
            KeyCode::F(2),
            KeyModifiers::NONE,
            CommandAction::RepeatSample,
        ),
        (KeyCode::F(5), KeyModifiers::NONE, CommandAction::Finish),
    ] {
        let mut key = KeyEvent::new(code, modifiers);
        assert_eq!(
            bindings.resolve(&key, BindingContext::Running),
            Some(action)
        );
        key.kind = KeyEventKind::Release;
        assert_eq!(bindings.resolve(&key, BindingContext::Running), None);
    }
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(bindings.resolve(&enter, BindingContext::Running), None);
    assert_eq!(
        bindings.resolve(&enter, BindingContext::Results),
        Some(CommandAction::NextSample)
    );
    assert_eq!(
        bindings.resolve(
            &KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            ),
            BindingContext::Running
        ),
        None,
        "AltGr text is not plain Ctrl-C"
    );
}

#[test]
fn custom_bindings_add_safe_aliases_and_reject_ambiguity_and_collisions() {
    let config =
        Config::from_text("[workflow.bindings]\nnew_sample='ctrl+n'\nhelp='f1'\n").unwrap();
    let bindings = Bindings::from_config(&config).unwrap();
    assert_eq!(
        bindings.resolve(
            &KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            BindingContext::Running
        ),
        Some(CommandAction::NewSample)
    );
    assert_eq!(
        bindings.resolve(
            &KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
            BindingContext::Running
        ),
        Some(CommandAction::NewSample)
    );
    for chord in [
        "a",
        "A",
        "enter",
        "tab",
        "ctrl+i",
        "ctrl+h",
        "ctrl+j",
        "ctrl+m",
        "ctrl+alt+a",
        "ctrl+ctrl+n",
        "f25",
        "f2",
        "ctrl+c",
    ] {
        let source = format!("[workflow.bindings]\nhelp={chord:?}\n");
        assert!(
            Config::from_text(&source).is_err(),
            "accepted ambiguous/colliding {chord}"
        );
    }
    assert!(Config::from_text("[workflow.bindings]\nrun_shell='f1'").is_err());
    assert!(Config::from_text("[workflow.bindings]\nhelp='ctrl+n'\nhistory='ctrl+n'").is_err());
}

#[test]
fn configured_private_mode_and_cli_private_are_explicit_session_flags() {
    let config = Config::from_text("[privacy]\nprivate_session=true").unwrap();
    assert!(config.privacy.private_session);
    let mut config = Config::default();
    config.privacy.store_custom_text = true;
    config.privacy.store_event_trace = true;
    Cli::try_parse_from(["clack", "--private"])
        .unwrap()
        .apply(&mut config)
        .unwrap();
    assert!(config.privacy.private_session);
    assert!(
        !config.privacy.save_results
            && !config.privacy.store_custom_text
            && !config.privacy.store_event_trace
    );
}

#[test]
fn oversized_configuration_is_rejected_without_modifying_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let content = "#".repeat(262_145);
    fs::write(&path, &content).unwrap();
    assert!(Config::read(&path).unwrap_err().contains("256"));
    assert_eq!(fs::metadata(&path).unwrap().len(), 262_145);
}

#[cfg(unix)]
#[test]
fn atomic_configuration_files_and_new_directories_are_user_only() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("new-private");
    let path = parent.join("config.toml");
    let mut config = Config::default();
    config
        .persist_edits(&path, &[edit("status.wpm", "true")])
        .unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn symbolic_link_configuration_edits_are_refused_without_touching_target() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("original.toml");
    let path = directory.path().join("link.toml");
    fs::write(&target, "# original\n").unwrap();
    symlink(&target, &path).unwrap();
    let mut config = Config::read(&path).unwrap();
    assert!(
        config
            .persist_edits(&path, &[edit("status.wpm", "true")])
            .is_err()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "# original\n");
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
