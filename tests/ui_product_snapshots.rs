//! Observable Stage C fixtures. Golden files include cells and style roles so
//! monochrome affordances and selected rows are reviewed alongside the text.
use clack::{
    config::Config,
    engine::{Action, Engine, Mode, ResultSnapshot, TestSpec},
    settings,
    storage::{HistoryEntry, HistoryPage, Statistics},
    ui::{
        Appearance, ColorPolicy,
        history::{self, History},
        palette::{self, Palette},
        panel::{self, Panel},
        review::{self, Review, Tab},
    },
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, TestBackend},
    buffer::Buffer,
    style::{Color, Modifier},
};

const SIZES: [(u16, u16); 3] = [(40, 10), (80, 24), (120, 40)];

fn appearance(monochrome: bool) -> Appearance {
    Appearance {
        // These ANSI colors are identical at every detected terminal depth.
        theme: "high_contrast".into(),
        color: if monochrome {
            ColorPolicy::Never
        } else {
            ColorPolicy::Always
        },
        ..Appearance::default()
    }
}

fn draw(size: (u16, u16), render: impl FnOnce(&mut Frame)) -> (Buffer, (u16, u16)) {
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal.draw(render).unwrap();
    let cursor = terminal.backend_mut().get_cursor_position().unwrap();
    (terminal.backend().buffer().clone(), (cursor.x, cursor.y))
}

fn rows(buffer: &Buffer) -> Vec<String> {
    buffer
        .content()
        .chunks(usize::from(buffer.area.width).max(1))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

fn text(buffer: &Buffer) -> String {
    rows(buffer).join("\n")
}

fn corrected_result() -> ResultSnapshot {
    let spec = TestSpec {
        mode: Mode::Custom,
        source_id: "custom".into(),
        content_hash: "c".repeat(64),
        approved_content: false,
        ..TestSpec::default()
    };
    let mut engine = Engine::new(spec, "cat dog fox owl yak eel ant bee").unwrap();
    engine.apply(Action::Text("cax"), 1_000_000);
    engine.apply(Action::Backspace, 1_100_000);
    engine.apply(Action::Text("t "), 1_200_000);
    for (input, when) in [
        ("do ", 2_000_000),
        ("fog ", 3_400_000),
        ("owl ", 4_400_000),
        ("yak ", 5_400_000),
        ("eel ", 6_400_000),
        ("ant ", 7_400_000),
        ("bee", 8_400_000),
    ] {
        engine.apply(Action::Text(input), when);
    }
    engine.snapshot()
}

fn populated_history() -> History {
    let result = corrected_result();
    let mut history = History::new(result.profile_key.clone(), false);
    history.loading = false;
    history.page = HistoryPage {
        results: (0..20)
            .map(|index| HistoryEntry {
                id: format!("fixture-{index}"),
                created_at_utc_ms: 1_783_036_800_000 + index * 86_400_000,
                created_at_utc: format!("2026-07-{:02}T12:00:00.000Z", index + 1),
                snapshot: result.clone(),
                sparkline: vec![Some(20.0), Some(27.0), None, Some(32.0)],
                word_summaries_omitted: 0,
            })
            .collect(),
        next_offset: Some(20),
    };
    history.statistics = Some(Statistics {
        result_count: 23,
        aggregate_wpm: Some(31.4),
        aggregate_accuracy: Some(92.3),
        ..Statistics::default()
    });
    history.selected = 19;
    history
}

fn config_panel() -> Panel {
    Panel {
        title: "Configuration".into(),
        lines: vec![
            "Effective values (CLI overrides remain session-local):".into(),
            "test.mode = time".into(),
            "test.seconds = 15".into(),
            "appearance.theme = high_contrast".into(),
            "privacy.private_session = true".into(),
            "privacy.save_results = false".into(),
            "workflow.bindings = { new_sample = 'ctrl+n' }".into(),
        ],
        scroll: 0,
        return_ready: true,
    }
}

#[test]
fn effective_result_verdict_and_unsaved_state_survive_compact_rendering() {
    let mut result = corrected_result();
    result.outcome = clack::engine::Outcome::Interrupted;
    result.reason = Some("input queue overload".into());
    result.integrity.input_overload = true;
    result.personal_best_eligible = false;
    let before = serde_json::to_value(&result).unwrap();
    for size in SIZES {
        for monochrome in [false, true] {
            let (buffer, _) = draw(size, |frame| {
                clack::ui::render_snapshot(
                    frame,
                    &result,
                    &appearance(monochrome),
                    Some("2 unsaved · commands: retry / export"),
                );
            });
            let rendered = text(&buffer).to_lowercase();
            assert!(rendered.contains("interrupted"), "{size:?}: {rendered}");
            assert!(rendered.contains("unsaved"), "{size:?}: {rendered}");
            assert!(!rendered.contains("personal best"), "{size:?}: {rendered}");
            assert_eq!(serde_json::to_value(&result).unwrap(), before);
        }
    }
}

#[test]
fn result_identity_distinguishes_effective_rules_and_limit_even_in_compact_mode() {
    let mut configurations = vec![TestSpec::default(); 3];
    configurations[1].seconds = 60;
    configurations[2].rules.difficulty = clack::engine::Difficulty::Expert;
    let mut identities = std::collections::HashSet::new();
    for spec in configurations {
        let seconds = spec.seconds;
        let mut engine = Engine::new(spec, "cat dog").unwrap();
        engine.apply(Action::Text("c"), 1_000_000);
        engine.apply(Action::Tick, 1_000_000 + u64::from(seconds) * 1_000_000);
        let result = engine.snapshot();
        let short_key = format!("#{}", &result.profile_key[..8]);
        assert!(
            identities.insert(short_key.clone()),
            "fixture identities must differ"
        );
        for size in SIZES {
            let mut previous = None;
            for monochrome in [false, true] {
                let (buffer, _) = draw(size, |frame| {
                    clack::ui::render_snapshot(frame, &result, &appearance(monochrome), None);
                });
                let rendered = text(&buffer);
                assert!(rendered.contains(&short_key), "{size:?}: {rendered}");
                assert!(
                    rendered.contains(&format!("time {seconds}s")),
                    "{size:?}: {rendered}"
                );
                if let Some(before) = &previous {
                    assert_eq!(&rendered, before, "color policy changed displayed identity");
                }
                previous = Some(rendered);
            }
        }
    }
}

#[test]
fn rapid_input_frames_keep_live_metrics_sampled_while_text_and_counts_advance() {
    let appearance = appearance(true);
    let geometry =
        clack::ui::Geometry::new(ratatui::layout::Rect::new(0, 0, 120, 40), &appearance).unwrap();
    let status = clack::ui::Status {
        progress: false,
        wpm: true,
        accuracy: true,
        ..clack::ui::Status::default()
    };
    let mut viewport = clack::ui::Viewport::default();
    let mut engine = Engine::new(TestSpec::default(), &"abcd ".repeat(100)).unwrap();
    engine.apply(Action::Text("abcd "), 1_000_000);
    engine.apply(Action::Tick, 2_000_000);
    let (buffer, _) = draw((120, 40), |frame| {
        clack::ui::render(
            frame,
            &engine,
            &appearance,
            &status,
            &mut viewport,
            1_000_000,
            None,
        );
    });
    let initial_status = rows(&buffer)[usize::from(geometry.status_y)].clone();
    assert!(initial_status.contains("60 wpm") && initial_status.contains("100.0%"));
    for index in 1..=24 {
        let elapsed = 1_000_000 + index * 10_000;
        engine.apply(Action::Text("x"), 1_000_000 + elapsed);
        let (buffer, _) = draw((120, 40), |frame| {
            clack::ui::render(
                frame,
                &engine,
                &appearance,
                &status,
                &mut viewport,
                elapsed,
                None,
            );
        });
        assert_eq!(engine.counts().attempts_total, 5 + index);
        assert_eq!(
            rows(&buffer)[usize::from(geometry.status_y)],
            initial_status,
            "a text frame refreshed metrics before the 250ms sample interval"
        );
        assert!(
            text(&buffer).contains('x'),
            "cached metrics must not freeze the typing widget"
        );
    }
    engine.apply(Action::Text("x"), 2_250_000);
    let (buffer, _) = draw((120, 40), |frame| {
        clack::ui::render(
            frame,
            &engine,
            &appearance,
            &status,
            &mut viewport,
            1_250_000,
            None,
        );
    });
    let updated = rows(&buffer)[usize::from(geometry.status_y)].clone();
    assert!(
        updated.contains("48 wpm") && updated.contains("16.7%"),
        "{updated}"
    );
    assert_eq!(engine.counts().attempts_total, 30);
}

#[test]
fn large_valid_live_speed_cannot_move_the_following_accuracy_column() {
    let source = "abcd ".repeat(2_100);
    let appearance = appearance(true);
    let geometry =
        clack::ui::Geometry::new(ratatui::layout::Rect::new(0, 0, 120, 40), &appearance).unwrap();
    let status = clack::ui::Status {
        progress: true,
        wpm: true,
        accuracy: true,
        ..clack::ui::Status::default()
    };
    let mut accuracy_columns = Vec::new();
    for entered_words in [1, 2_000] {
        let mut engine = Engine::new(TestSpec::default(), &source).unwrap();
        for (index, chunk) in source.as_bytes()[..entered_words * 5]
            .chunks(125)
            .enumerate()
        {
            // Each associated payload stays below the reader's128-grapheme cap.
            engine.apply(
                Action::Text(std::str::from_utf8(chunk).unwrap()),
                1_000_000 + index as u64 * 10_000,
            );
        }
        engine.apply(Action::Tick, 2_000_000);
        let mut viewport = clack::ui::Viewport::default();
        let (buffer, _) = draw((120, 40), |frame| {
            clack::ui::render(
                frame,
                &engine,
                &appearance,
                &status,
                &mut viewport,
                1_000_000,
                None,
            );
        });
        let row = rows(&buffer)[usize::from(geometry.status_y)].clone();
        accuracy_columns.push(row.find("100.0%").expect("accuracy must remain visible"));
        if entered_words == 2_000 {
            assert!(
                row.contains("9999+ wpm"),
                "large finite speed needs a fixed-width overflow cue: {row}"
            );
            assert_eq!(engine.counts().credited_units, 10_000);
            assert_eq!(engine.metrics().wpm, Some(120_000.0));
        }
    }
    assert_eq!(accuracy_columns[0], accuracy_columns[1]);
}

#[test]
fn every_theme_degrades_without_unsupported_color_sequences_and_keeps_error_cues() {
    use clack::ui::{ColorDepth, THEMES, Theme};
    for name in THEMES {
        let appearance = Appearance {
            theme: (*name).into(),
            ..Appearance::default()
        };
        for depth in [
            ColorDepth::Monochrome,
            ColorDepth::Ansi16,
            ColorDepth::Ansi256,
            ColorDepth::TrueColor,
        ] {
            let theme = Theme::from_preferences(&appearance, depth);
            let roles = [
                theme.background,
                theme.pending,
                theme.correct,
                theme.incorrect,
                theme.extra,
                theme.muted,
                theme.accent,
                theme.caret,
                theme.pace,
            ];
            for role in roles {
                for color in [role.fg, role.bg].into_iter().flatten() {
                    match depth {
                        ColorDepth::Monochrome => assert_eq!(color, Color::Reset),
                        ColorDepth::Ansi16 => {
                            assert!(!matches!(color, Color::Rgb(..) | Color::Indexed(_)))
                        }
                        ColorDepth::Ansi256 => assert!(!matches!(color, Color::Rgb(..))),
                        ColorDepth::TrueColor => {}
                    }
                }
            }
            assert!(
                theme
                    .incorrect
                    .add_modifier
                    .intersects(Modifier::UNDERLINED | Modifier::REVERSED)
            );
            assert!(
                theme
                    .extra
                    .add_modifier
                    .intersects(Modifier::UNDERLINED | Modifier::REVERSED)
            );
            assert!(
                theme
                    .caret
                    .add_modifier
                    .intersects(Modifier::UNDERLINED | Modifier::REVERSED)
            );
            if *name == "terminal" && depth != ColorDepth::Monochrome {
                assert_eq!(theme.background.bg, Some(Color::Reset));
                assert_eq!(theme.correct.fg, Some(Color::Reset));
            }
        }
    }
}

#[test]
fn every_overlay_survives_zero_or_unsafe_geometry() {
    let config = Config::default();
    let appearance = appearance(true);
    let mut palette = Palette::new(&config, true);
    let mut review = Review::new(corrected_result());
    let history = populated_history();
    let panel = config_panel();
    for size in [(0, 0), (1, 1), (39, 9), (40, 9), (39, 10)] {
        draw(size, |frame| palette::render(frame, &palette, &appearance));
        palette.begin_setting(settings::find("test.seconds").unwrap(), &config, Vec::new());
        draw(size, |frame| palette::render(frame, &palette, &appearance));
        palette.editor = None;
        for tab in [Tab::Summary, Tab::Text] {
            review.tab = tab;
            draw(size, |frame| review::render(frame, &review, &appearance));
        }
        draw(size, |frame| history::render(frame, &history, &appearance));
        draw(size, |frame| panel::render(frame, &panel, &appearance));
        draw(size, |frame| {
            clack::ui::render_snapshot(frame, &review.result, &appearance, Some("unsaved"));
        });
    }
}

#[test]
fn every_selected_palette_match_stays_visible_and_list_is_bounded() {
    for size in SIZES {
        for monochrome in [false, true] {
            let appearance = appearance(monochrome);
            let mut palette = Palette::new(&Config::default(), true);
            palette.query = "theme".into();
            let match_count = palette.matches().len();
            assert!(match_count > 2, "fixture must exercise compact scrolling");
            for selected in 0..match_count {
                palette.selected = selected;
                let label = palette.selected_entry().unwrap().label.clone();
                let (buffer, cursor) =
                    draw(size, |frame| palette::render(frame, &palette, &appearance));
                let visible_rows = rows(&buffer);
                let selected_row = visible_rows
                    .iter()
                    .find(|row| row.trim_start().starts_with("> ") && row.contains(&label));
                assert!(
                    selected_row.is_some(),
                    "{size:?}: selection {label:?} is invisible\n{}",
                    text(&buffer)
                );
                let entry_rows = visible_rows
                    .iter()
                    .filter(|row| {
                        palette
                            .entries
                            .iter()
                            .any(|entry| row.contains(&entry.label))
                    })
                    .count();
                assert!(entry_rows <= 7, "palette displays more than seven matches");
                assert!(cursor.0 < size.0 && cursor.1 < size.1);
                if monochrome {
                    assert!(
                        buffer
                            .content()
                            .iter()
                            .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
                    );
                }
            }
        }
    }
}

#[test]
fn palette_search_and_editor_handle_unicode_without_partial_deletion_or_controls() {
    let config = Config::default();
    let mut palette = Palette::new(&config, true);
    palette.insert("THEME high");
    assert_eq!(palette.matches().len(), 1);
    assert!(
        palette
            .selected_entry()
            .unwrap()
            .label
            .contains("high_contrast")
    );
    palette.query.clear();
    palette.insert("e\u{301}👩\u{200d}💻");
    palette.backspace();
    assert_eq!(palette.query, "e\u{301}");
    palette.backspace();
    assert!(palette.query.is_empty());
    palette.insert("\x1b[31m");
    assert!(palette.query.is_empty());
    assert!(palette.message.is_some());
    palette.insert(&"x".repeat(512));
    palette.insert("界");
    assert_eq!(palette.query.len(), 512);
    assert!(palette.message.is_some());

    palette.begin_setting(settings::find("test.file").unwrap(), &config, Vec::new());
    palette.insert("/tmp/界é.txt");
    assert_eq!(palette.editor.as_ref().unwrap().value, "/tmp/界é.txt");
    for size in SIZES {
        let (_, cursor) = draw(size, |frame| {
            palette::render(frame, &palette, &appearance(true))
        });
        assert!(cursor.0 < size.0 && cursor.1 < size.1);
    }
}

#[test]
fn review_distinguishes_repaired_attempts_from_final_missed_and_wrong_output() {
    let mut review = Review::new(corrected_result());
    review.tab = Tab::Text;
    let (buffer, _) = draw((120, 40), |frame| {
        review::render(frame, &review, &appearance(true))
    });
    let rendered = text(&buffer);
    for expected in [
        "corrected while typing",
        "exp cat",
        "got cat",
        "exp dog",
        "got do",
        "exp fox",
        "got fog",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?}\n{rendered}"
        );
    }
    // Scrolling must reveal the actual original last word rather than a typo-derived target.
    review.move_by(isize::MAX);
    for size in SIZES {
        let (buffer, _) = draw(size, |frame| {
            review::render(frame, &review, &appearance(true))
        });
        assert!(text(&buffer).contains("exp bee"));
    }
    review.toggle();
    assert_eq!(review.scroll, 0);
    let (buffer, _) = draw((120, 40), |frame| {
        review::render(frame, &review, &appearance(true))
    });
    let rendered = text(&buffer);
    for expected in [
        "mistaken attempts",
        "Final:",
        "missed",
        "deletions",
        "Mistaken tokens: cat, dog, fox",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?}\n{rendered}"
        );
    }
}

#[test]
fn review_summary_scroll_endpoint_never_leaves_an_empty_content_view() {
    let mut review = Review::new(corrected_result());
    review.move_by(isize::MAX);
    for size in SIZES {
        let (buffer, _) = draw(size, |frame| {
            review::render(frame, &review, &appearance(true))
        });
        assert!(
            rows(&buffer)[3..usize::from(size.1 - 2)]
                .iter()
                .any(|row| !row.trim().is_empty()),
            "summary scrolled past all meaningful content at {size:?}"
        );
    }
}

#[test]
fn historical_private_review_shows_hash_requirement_without_invented_text() {
    let mut snapshot = corrected_result();
    snapshot.words.clear();
    let mut review = Review::new(snapshot);
    review.tab = Tab::Text;
    assert!(review.original_required);
    for size in SIZES {
        let (buffer, _) = draw(size, |frame| {
            review::render(frame, &review, &appearance(true))
        });
        let rendered = text(&buffer);
        assert!(rendered.contains("Original text was not retained."));
        assert!(rendered.contains("cccccccccccc"));
        assert!(!rendered.contains("cat") && !rendered.contains("fog"));
    }
}

#[test]
fn zen_review_labels_output_and_does_not_invent_target_accuracy() {
    let spec = TestSpec {
        mode: Mode::Zen,
        source_id: "zen".into(),
        approved_content: false,
        ..TestSpec::default()
    };
    let mut engine = Engine::new(spec, "").unwrap();
    engine.apply(Action::Text("free writing"), 1_000_000);
    engine.apply(Action::Finish, 2_500_000);
    let mut review = Review::new(engine.snapshot());
    assert!(!review.original_required);
    let (buffer, _) = draw((120, 40), |frame| {
        review::render(frame, &review, &appearance(true))
    });
    let rendered = text(&buffer);
    assert!(rendered.contains("wpm output"));
    assert!(rendered.contains("Accuracy and target-error metrics are unavailable in zen."));
    assert!(!rendered.contains("Final:") && !rendered.contains("% accuracy"));
    review.toggle();
    let (buffer, _) = draw((120, 40), |frame| {
        review::render(frame, &review, &appearance(true))
    });
    assert!(text(&buffer).contains("Zen has no expected target or accuracy score."));
}

#[test]
fn verified_single_line_original_can_scroll_to_its_last_displayed_word() {
    for size in SIZES {
        let mut snapshot = corrected_result();
        snapshot.words.clear();
        let mut review = Review::new(snapshot);
        review.original_required = false;
        review.entered_available = false;
        review.set_original_text(Some(format!(
            "first {} LASTORIGINAL",
            "bridge ".repeat(200)
        )));
        review.tab = Tab::Text;
        let _ = draw(size, |frame| {
            review::render(frame, &review, &appearance(true))
        });
        review.move_by(isize::MAX);
        // A terminal-cell wrap may split the marker across the final two rows.
        review.move_by(-1);
        let (buffer, _) = draw(size, |frame| {
            review::render(frame, &review, &appearance(true))
        });
        let tail = rows(&buffer)[4..usize::from(size.1 - 2)]
            .iter()
            .map(|row| row.trim())
            .collect::<String>();
        assert!(
            tail.contains("LASTORIGINAL"),
            "verified source tail is unreachable at {size:?}"
        );
    }
}

#[test]
fn history_selection_empty_loading_and_failure_states_remain_usable() {
    for size in SIZES {
        let mut history = populated_history();
        let (buffer, _) = draw(size, |frame| {
            history::render(frame, &history, &appearance(true))
        });
        assert!(text(&buffer).contains("> 2026-07-20"));
        assert!(text(&buffer).contains("23 runs"));
        history.page.results.clear();
        let (buffer, _) = draw(size, |frame| {
            history::render(frame, &history, &appearance(true))
        });
        assert!(text(&buffer).contains("No results match this filter."));
        history.loading = true;
        let (buffer, _) = draw(size, |frame| {
            history::render(frame, &history, &appearance(true))
        });
        assert!(text(&buffer).contains("Loading local history"));
        history.message = Some("Storage busy; results remain unsaved".into());
        let (buffer, _) = draw(size, |frame| {
            history::render(frame, &history, &appearance(true))
        });
        assert!(text(&buffer).contains("Storage busy"));
    }
}

#[test]
fn sparklines_obey_requested_width_ascii_and_finite_fallback() {
    for width in [0, 1, 4, 80] {
        for values in [
            vec![],
            vec![0.0],
            vec![f64::NAN, f64::INFINITY, -10.0, 2.0, 5.0],
        ] {
            let actual = review::sparkline(values.into_iter(), width, true);
            assert!(actual.is_ascii(), "ASCII chart used {actual:?}");
            assert!(
                actual.len() <= width,
                "chart exceeded width {width}: {actual:?}"
            );
        }
    }
}

fn snapshot(label: &str, buffer: &Buffer, cursor: Option<(u16, u16)>) -> String {
    use std::fmt::Write;
    let mut output = format!(
        "[{label}] {}x{} cursor={cursor:?}\n",
        buffer.area.width, buffer.area.height
    );
    for row in buffer.content().chunks(usize::from(buffer.area.width)) {
        let content: String = row.iter().map(|cell| cell.symbol()).collect();
        writeln!(&mut output, "{}", content.trim_end()).unwrap();
        let styles: String = row
            .iter()
            .map(|cell| {
                if cell.symbol().trim().is_empty() {
                    ' '
                } else if cell.modifier.contains(Modifier::UNDERLINED) {
                    'U'
                } else if cell.modifier.contains(Modifier::DIM) {
                    'd'
                } else if cell.fg == Color::Yellow {
                    'A'
                } else if cell.fg == Color::Gray {
                    'm'
                } else if cell.fg == Color::Reset {
                    '0'
                } else {
                    'F'
                }
            })
            .collect();
        if !styles.trim().is_empty() {
            writeln!(&mut output, "style {}", styles.trim_end()).unwrap();
        }
    }
    output
}

#[test]
fn product_overlay_snapshot_matrix() {
    for size in SIZES {
        for monochrome in [false, true] {
            let appearance = appearance(monochrome);
            let mut output = String::from(
                "Style legend: A=accent m=muted F=foreground 0=monochrome d=dim U=underlined\n",
            );
            let mut capture = |label: &str, cursor: bool, render: &dyn Fn(&mut Frame)| {
                let (buffer, position) = draw(size, render);
                output.push_str(&snapshot(label, &buffer, cursor.then_some(position)));
            };
            let config = Config::default();
            let mut palette = Palette::new(&config, true);
            palette.query = "theme".into();
            palette.selected = palette.matches().len() - 1;
            capture("palette-theme-last", true, &|frame| {
                palette::render(frame, &palette, &appearance)
            });
            palette.query = "does not match a command".into();
            palette.selected = 0;
            capture("palette-no-matches", true, &|frame| {
                palette::render(frame, &palette, &appearance)
            });
            palette.begin_setting(settings::find("test.seconds").unwrap(), &config, Vec::new());
            palette.insert("0");
            palette.message = Some("test.seconds=0: expected an integer in 1..=3600".into());
            capture("settings-invalid-value", true, &|frame| {
                palette::render(frame, &palette, &appearance)
            });
            palette.begin_setting(
                settings::find("appearance.theme").unwrap(),
                &config,
                clack::ui::THEMES
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
            );
            palette.editor.as_mut().unwrap().value = "warm".into();
            capture("settings-theme-choices", true, &|frame| {
                palette::render(frame, &palette, &appearance)
            });
            let mut review = Review::new(corrected_result());
            capture("review-summary", false, &|frame| {
                review::render(frame, &review, &appearance)
            });
            review.tab = Tab::Text;
            capture("review-text", false, &|frame| {
                review::render(frame, &review, &appearance)
            });
            review.move_by(isize::MAX);
            capture("review-text-last", false, &|frame| {
                review::render(frame, &review, &appearance)
            });
            review.result.words.clear();
            review.original_required = true;
            capture("review-private-original-required", false, &|frame| {
                review::render(frame, &review, &appearance)
            });
            review.original_required = false;
            review.entered_available = false;
            review.set_original_text(Some("cat dog fox owl yak eel ant bee".into()));
            review.scroll = 0;
            capture("review-verified-original", false, &|frame| {
                review::render(frame, &review, &appearance)
            });
            let mut zen = Engine::new(
                TestSpec {
                    mode: Mode::Zen,
                    source_id: "zen".into(),
                    approved_content: false,
                    ..TestSpec::default()
                },
                "",
            )
            .unwrap();
            zen.apply(Action::Text("free writing"), 1_000_000);
            zen.apply(Action::Finish, 2_500_000);
            let mut zen_review = Review::new(zen.snapshot());
            capture("review-zen-summary", false, &|frame| {
                review::render(frame, &zen_review, &appearance)
            });
            zen_review.toggle();
            capture("review-zen-text", false, &|frame| {
                review::render(frame, &zen_review, &appearance)
            });
            let mut history = populated_history();
            capture("history-selected-last", false, &|frame| {
                history::render(frame, &history, &appearance)
            });
            history.page.results.clear();
            history.message = Some("Storage busy; results remain unsaved".into());
            capture("history-empty-storage-failure", false, &|frame| {
                history::render(frame, &history, &appearance)
            });
            history.loading = true;
            history.message = None;
            history.statistics = None;
            capture("history-loading", false, &|frame| {
                history::render(frame, &history, &appearance)
            });
            let mut panel = config_panel();
            capture("config-panel", false, &|frame| {
                panel::render(frame, &panel, &appearance)
            });
            panel.move_by(isize::MAX);
            capture("config-panel-last", false, &|frame| {
                panel::render(frame, &panel, &appearance)
            });
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "tests/snapshots/product-{}x{}-{}.txt",
                size.0,
                size.1,
                if monochrome { "mono" } else { "color" }
            ));
            if std::env::var_os("CLACK_UPDATE_SNAPSHOTS").is_some() {
                std::fs::write(&path, &output).unwrap();
            }
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                output,
                "{}",
                path.display()
            );
        }
    }
}
