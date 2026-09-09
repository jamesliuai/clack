use clack::{
    config::Config,
    engine::{Mode, Policy},
    settings::{BindingContext, Bindings, CommandAction},
    ui::{self, Appearance, ColorPolicy, setup::Setup},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

fn applied(config: &Config, setup: &Setup) -> Config {
    config.edited(&setup.edits().unwrap()).unwrap()
}

#[test]
fn common_switches_are_direct_and_keep_each_modes_length() {
    let mut config = Config::default();
    config.test.seconds = 15;
    let mut setup = Setup::new(&config);
    setup.key(KeyCode::Right);
    assert_eq!(applied(&config, &setup).test.seconds, 30);
    setup.insert("w");
    assert_eq!(applied(&config, &setup).test.mode, Mode::Words);
    setup.key(KeyCode::Left);
    assert_eq!(applied(&config, &setup).test.words, 25);
    setup.insert("t");
    assert_eq!(applied(&config, &setup).test.seconds, 30);
    setup.insert("w");
    assert_eq!(applied(&config, &setup).test.words, 25);
    assert_eq!(config.test.seconds, 15, "draft must not mutate original");
}

#[test]
fn numeric_values_validate_recover_and_step_to_nearby_presets() {
    let config = Config::default();
    let mut setup = Setup::new(&config);
    for value in ["0", "3601", ""] {
        setup = Setup::new(&config);
        setup.key(KeyCode::Backspace);
        setup.insert(value);
        assert!(setup.edits().unwrap_err().contains("1-3600"));
    }
    setup.insert("45");
    assert_eq!(applied(&config, &setup).test.seconds, 45);
    setup.key(KeyCode::Right);
    assert_eq!(applied(&config, &setup).test.seconds, 60);
    setup.insert("45");
    setup.key(KeyCode::Left);
    assert_eq!(applied(&config, &setup).test.seconds, 30);
    setup.insert("w");
    setup.insert("10000");
    assert_eq!(applied(&config, &setup).test.words, 10000);
    setup.key(KeyCode::Backspace);
    setup.insert("1");
    assert!(setup.edits().is_err());
}

#[test]
fn code_transitions_and_quote_categories_are_atomic() {
    let mut config = Config::default();
    config.test.mode = Mode::Code;
    config.test.policy = Policy::Exact;
    config.test.normalize_exact = true;
    config.practice.auto_indent = true;
    let mut setup = Setup::new(&config);
    setup.insert("w");
    let next = applied(&config, &setup);
    assert_eq!(next.test.policy, Policy::Prose);
    assert!(!next.test.normalize_exact && !next.practice.auto_indent);
    let mut setup = Setup::new(&next);
    setup.insert("d");
    assert_eq!(applied(&next, &setup).test.policy, Policy::Exact);
    config = Config::default();
    config.test.mode = Mode::Quote;
    config.test.quote_id = Some("pinned-quote".into());
    let mut setup = Setup::new(&config);
    setup.key(KeyCode::Right);
    let next = applied(&config, &setup);
    assert_eq!(next.test.quote_length, "medium");
    assert!(next.test.quote_id.is_none());
}

#[test]
fn only_visible_changed_settings_are_committed() {
    let config = Config::default();
    let mut setup = Setup::new(&config);
    assert!(setup.edits().unwrap().is_empty());
    setup.key(KeyCode::Down);
    setup.key(KeyCode::Right); // punctuation
    setup.key(KeyCode::Tab);
    setup.insert(" "); // enhanced-keyboard Space toggles numbers
    let next = applied(&config, &setup);
    assert!(next.test.punctuation && next.test.numbers);
    setup.insert("z");
    let edits = setup.edits().unwrap();
    assert!(
        !edits
            .iter()
            .any(|e| e.path == "test.punctuation" || e.path == "test.numbers")
    );
}

#[test]
fn file_paths_accept_spaces_and_shortcut_letters_as_literal_text() {
    let config = Config::default();
    let mut setup = Setup::new(&config);
    setup.insert("c");
    setup.insert("/tmp/my");
    setup.key(KeyCode::Char(' '));
    setup.insert("words.txt");
    assert_eq!(
        applied(&config, &setup).test.file.unwrap().to_str(),
        Some("/tmp/my words.txt")
    );
    assert_eq!(setup.mode, Mode::Custom);
}

#[test]
fn setup_commands_are_recognized_before_scoring_in_every_context() {
    let bindings = Bindings::default();
    for context in [
        BindingContext::Ready,
        BindingContext::Running,
        BindingContext::Results,
        BindingContext::Overlay,
    ] {
        for key in [
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE),
        ] {
            assert_eq!(
                bindings.resolve(&key, context),
                Some(CommandAction::TestSetup)
            );
        }
    }
}

#[test]
fn setup_layout_keeps_selection_and_controls_visible_at_all_supported_sizes() {
    for (width, height) in [(40, 10), (60, 18), (80, 24), (120, 40)] {
        for color in [ColorPolicy::Never, ColorPolicy::Always] {
            for mode in [
                Mode::Time,
                Mode::Words,
                Mode::Quote,
                Mode::Custom,
                Mode::Code,
                Mode::Zen,
            ] {
                let mut config = Config::default();
                config.test.mode = mode;
                let mut setup = Setup::new(&config);
                setup.key(KeyCode::Up); // select mode row
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|f| {
                        ui::setup::render(
                            f,
                            &setup,
                            &Appearance {
                                color,
                                ..Appearance::default()
                            },
                            None,
                        )
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                let lines: Vec<String> = buffer
                    .content()
                    .chunks(width as usize)
                    .map(|row| row.iter().map(|c| c.symbol()).collect())
                    .collect();
                let text = lines.join("\n");
                let name = format!("[{mode:?}]").to_lowercase();
                assert!(text.contains(&name), "{width}x{height} {name}:\n{text}");
                assert!(text.contains("enter apply") && text.contains("esc cancel"));
                assert!(text.contains("←→ change"));
                assert!(text.contains("Test setup"));
            }
        }
    }
}
