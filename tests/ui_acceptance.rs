use clack::{
    engine::{Action, Engine, Mode, Policy, TestSpec},
    ui::{self, Appearance, ColorPolicy, Focus, Geometry, Status, Viewport},
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
};

fn draw(
    engine: &Engine,
    appearance: &Appearance,
    size: (u16, u16),
    viewport: &mut Viewport,
) -> (Buffer, Option<(u16, u16)>) {
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    let mut cursor = None;
    terminal
        .draw(|frame| {
            cursor = ui::render(
                frame,
                engine,
                appearance,
                &Status::default(),
                viewport,
                engine.elapsed_us(),
                None,
            )
        })
        .unwrap();
    (terminal.backend().buffer().clone(), cursor)
}
fn prose() -> Engine {
    Engine::new(TestSpec::default(),"the world can change when small things become part of the way we work and think about what comes next in a place we know so well").unwrap()
}
fn text(buffer: &Buffer) -> String {
    buffer
        .content()
        .chunks(usize::from(buffer.area.width).max(1))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_epoch_interruption_stays_visible_after_the_save_notice_is_cleared() {
    let mut engine = Engine::new(
        TestSpec {
            mode: Mode::Custom,
            ..TestSpec::default()
        },
        "cat",
    )
    .unwrap();
    engine.apply(Action::Text("ca"), 1_000_000);
    engine.apply(Action::Text("t"), 2_000_000);
    let mut snapshot = engine.snapshot();
    snapshot.outcome = clack::engine::Outcome::Interrupted;
    snapshot.reason = Some("input queue overload".into());
    snapshot.integrity.input_overload = true;
    snapshot.personal_best_eligible = false;
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    for notice in [Some("Saving…"), None] {
        terminal
            .draw(|frame| ui::render_snapshot(frame, &snapshot, &Appearance::default(), notice))
            .unwrap();
        let rendered = text(terminal.backend().buffer());
        assert!(rendered.contains("interrupted"));
        assert!(rendered.contains("input queue overload"));
        assert!(!rendered.contains("Personal best"));
    }
}

#[test]
fn current_review_can_reach_long_token_errors_without_disk_persistence() {
    let target = format!("{}tailGOOD", "x".repeat(2000));
    let input = format!("{}tailBADD", "x".repeat(2000));
    let mut engine = Engine::new(
        TestSpec {
            mode: Mode::Custom,
            policy: Policy::Exact,
            ..TestSpec::default()
        },
        &target,
    )
    .unwrap();
    engine.apply(Action::Text(&input), 1_000_000);
    engine.apply(Action::Finish, 2_000_000);
    let mut review = ui::review::Review::new(engine.snapshot());
    review.toggle();
    review.show_tail();
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    terminal
        .draw(|frame| ui::review::render(frame, &review, &Appearance::default()))
        .unwrap();
    let rendered = text(terminal.backend().buffer());
    assert!(rendered.contains("exp tailGOOD"));
    assert!(rendered.contains("got tailBADD"));
    review.pan(isize::MIN);
    assert_eq!(review.horizontal, 0);
}

#[test]
fn status_switches_are_independent_of_focus_and_zen_uses_output_cpm() {
    for mode in [Mode::Time, Mode::Zen] {
        let mut engine = Engine::new(
            TestSpec {
                mode,
                ..TestSpec::default()
            },
            if mode == Mode::Zen { "" } else { "abc def ghi" },
        )
        .unwrap();
        engine.apply(Action::Text("abc"), 1_000_000);
        engine.apply(Action::Tick, 3_000_000);
        for focus in [Focus::Auto, Focus::Off] {
            let appearance = Appearance {
                focus,
                ..Appearance::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut viewport = Viewport::default();
            let status = Status {
                progress: false,
                wpm: false,
                accuracy: false,
                ..Status::default()
            };
            terminal
                .draw(|frame| {
                    ui::render(
                        frame,
                        &engine,
                        &appearance,
                        &status,
                        &mut viewport,
                        2_000_000,
                        None,
                    );
                })
                .unwrap();
            let geometry = Geometry::new(Rect::new(0, 0, 120, 40), &appearance).unwrap();
            let row: String = (0..120)
                .map(|x| terminal.backend().buffer()[(x, geometry.status_y)].symbol())
                .collect();
            assert!(
                row.trim().is_empty(),
                "hidden status must stay hidden: {row:?}"
            );
            let status = Status {
                wpm: true,
                speed_unit: ui::SpeedUnit::Cpm,
                ..status
            };
            terminal
                .draw(|frame| {
                    ui::render(
                        frame,
                        &engine,
                        &appearance,
                        &status,
                        &mut viewport,
                        2_000_000,
                        None,
                    );
                })
                .unwrap();
            let rendered = text(terminal.backend().buffer());
            assert!(
                rendered.contains("90 cpm"),
                "{mode:?}/{focus:?}: {rendered}"
            );
            assert!(!rendered.contains("wpm"));
        }
    }
}

#[test]
fn first_input_keeps_target_coordinates_and_correct_typing_reuses_layout() {
    for size in [(40, 10), (80, 24), (120, 40)] {
        let appearance = Appearance::default();
        let mut viewport = Viewport::default();
        let mut engine = prose();
        let (_, ready) = draw(&engine, &appearance, size, &mut viewport);
        let rebuilds = viewport.layout_rebuilds;
        engine.apply(Action::Text("t"), 1_000_000);
        let (active, cursor) = draw(&engine, &appearance, size, &mut viewport);
        assert_eq!(cursor, ready.map(|(x, y)| (x + 1, y)));
        assert_eq!(viewport.layout_rebuilds, rebuilds);
        assert!(!text(&active).contains("start typing"));
        assert!(!text(&active).contains("english 200"));
        assert!(text(&active).contains("the world"));
    }
}
#[test]
fn incorrect_and_extra_input_have_visible_non_color_cues() {
    let appearance = Appearance {
        color: ColorPolicy::Never,
        ..Appearance::default()
    };
    let mut engine = prose();
    let mut viewport = Viewport::default();
    let (_, position) = draw(&engine, &appearance, (80, 24), &mut viewport);
    let (x, y) = position.unwrap();
    engine.apply(Action::Text("x"), 1_000_000);
    let (buffer, _) = draw(&engine, &appearance, (80, 24), &mut viewport);
    assert_eq!(buffer[(x, y)].symbol(), "x");
    assert!(buffer[(x, y)].modifier.contains(Modifier::UNDERLINED));
    assert_eq!(buffer[(x, y)].fg, Color::Reset);
    engine.apply(Action::Text("yzq"), 1_100_000);
    let (buffer, _) = draw(&engine, &appearance, (80, 24), &mut viewport);
    assert_eq!(buffer[(x + 3, y)].symbol(), "q");
    assert!(
        buffer[(x + 3, y)]
            .modifier
            .contains(Modifier::BOLD | Modifier::UNDERLINED)
    );
}
#[test]
fn wide_and_combining_units_have_correct_cells_and_logical_cursor() {
    let spec = TestSpec {
        mode: Mode::Custom,
        policy: Policy::Exact,
        ..TestSpec::default()
    };
    let mut engine = Engine::new(spec, "界éz").unwrap();
    let appearance = Appearance::default();
    let mut viewport = Viewport::default();
    let (buffer, ready) = draw(&engine, &appearance, (80, 24), &mut viewport);
    let (x, y) = ready.unwrap();
    assert_eq!(buffer[(x, y)].symbol(), "界");
    assert_eq!(buffer[(x + 2, y)].symbol(), "é");
    engine.apply(Action::Text("界"), 1_000_000);
    let (_, cursor) = draw(&engine, &appearance, (80, 24), &mut viewport);
    assert_eq!(cursor, Some((x + 2, y)));
    engine.apply(Action::Text("e\u{301}"), 1_100_000);
    let (buffer, cursor) = draw(&engine, &appearance, (80, 24), &mut viewport);
    assert_eq!(cursor, Some((x + 3, y)));
    assert_eq!(buffer[(x + 2, y)].symbol(), "e\u{301}");
    engine.apply(Action::Backspace, 1_200_000);
    let (_, cursor) = draw(&engine, &appearance, (80, 24), &mut viewport);
    assert_eq!(cursor, Some((x + 2, y)));
}
#[test]
fn exact_tabs_and_newlines_use_display_cells_without_changing_scoring() {
    let spec = TestSpec {
        mode: Mode::Code,
        policy: Policy::Exact,
        ..TestSpec::default()
    };
    let mut engine = Engine::new(spec, "\tcat\nx").unwrap();
    let appearance = Appearance::default();
    let mut viewport = Viewport::default();
    let (buffer, ready) = draw(&engine, &appearance, (80, 24), &mut viewport);
    let (x, y) = ready.unwrap();
    assert_eq!(buffer[(x, y)].symbol(), "→");
    assert_eq!(buffer[(x + 4, y)].symbol(), "c");
    assert_eq!(buffer[(x + 7, y)].symbol(), "↵");
    engine.apply(Action::Text("\tcat\n"), 1_000_000);
    let (_, cursor) = draw(&engine, &appearance, (80, 24), &mut viewport);
    assert_eq!(cursor, Some((x, y + 1)));
    assert_eq!(engine.counts().attempts_total, 5);
}
#[test]
fn geometry_and_all_states_are_safe_across_small_and_extreme_preferences() {
    for (width, height) in [
        (0, 0),
        (1, 1),
        (39, 9),
        (40, 10),
        (41, 11),
        (80, 24),
        (120, 40),
        (200, 60),
    ] {
        for lines in 1..=5 {
            for line_spacing in 0..=1 {
                let appearance = Appearance {
                    lines,
                    line_spacing,
                    ..Appearance::default()
                };
                let mut viewport = Viewport::default();
                let mut engine = prose();
                if let Some(geometry) = Geometry::new(Rect::new(0, 0, width, height), &appearance) {
                    assert!(geometry.text.bottom() <= height);
                    assert!(geometry.text.right() <= width);
                    assert_eq!(geometry.compact, width <= 40 || height <= 10);
                }
                for step in 0..3 {
                    if step == 1 {
                        engine.apply(Action::Text("x"), 1_000_000);
                    }
                    if step == 2 {
                        engine.apply(Action::Tick, 31_000_000);
                    }
                    let (_, cursor) = draw(&engine, &appearance, (width, height), &mut viewport);
                    if let Some((x, y)) = cursor {
                        assert!(x < width && y < height);
                    }
                }
            }
        }
    }
}
#[test]
fn full_focus_contains_only_text_and_caret_while_running() {
    let mut engine = prose();
    let appearance = Appearance {
        focus: Focus::Always,
        ..Appearance::default()
    };
    let mut viewport = Viewport::default();
    engine.apply(Action::Text("t"), 1_000_000);
    let (buffer, _) = draw(&engine, &appearance, (120, 40), &mut viewport);
    let geometry = Geometry::new(buffer.area, &appearance).unwrap();
    for y in 0..buffer.area.height {
        if y < geometry.text.y || y >= geometry.text.bottom() {
            for x in 0..buffer.area.width {
                assert_eq!(buffer[(x, y)].symbol(), " ");
            }
        }
    }
}

fn snapshot(buffer: &Buffer, cursor: Option<(u16, u16)>) -> String {
    let mut result = format!(
        "{}x{} cursor={cursor:?}\n",
        buffer.area.width, buffer.area.height
    );
    for y in 0..buffer.area.height {
        let mut row = String::new();
        let mut cues = String::new();
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            row.push_str(cell.symbol());
            cues.push(if cell.modifier.contains(Modifier::UNDERLINED) {
                'U'
            } else if cell.modifier.contains(Modifier::BOLD) {
                'B'
            } else if cell.modifier.contains(Modifier::DIM) {
                'd'
            } else {
                ' '
            });
        }
        result.push_str(row.trim_end());
        result.push('\n');
        if !cues.trim().is_empty() {
            result.push_str("style ");
            result.push_str(cues.trim_end());
            result.push('\n');
        }
    }
    result
}
#[test]
fn ready_running_results_snapshots_at_required_sizes_in_color_and_monochrome() {
    for (width, height) in [(40, 10), (80, 24), (120, 40)] {
        for color in [ColorPolicy::Always, ColorPolicy::Never] {
            for state in ["ready", "running", "results"] {
                let appearance = Appearance {
                    color,
                    ..Appearance::default()
                };
                let mut engine = prose();
                let mut viewport = Viewport::default();
                if state != "ready" {
                    engine.apply(Action::Text("thx"), 1_000_000);
                }
                if state == "results" {
                    engine.apply(Action::Tick, 31_000_000);
                }
                let (buffer, cursor) = draw(&engine, &appearance, (width, height), &mut viewport);
                let actual = snapshot(&buffer, cursor);
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                    "tests/snapshots/{state}-{width}x{height}-{}.txt",
                    if color == ColorPolicy::Never {
                        "mono"
                    } else {
                        "color"
                    }
                ));
                if std::env::var_os("CLACK_UPDATE_SNAPSHOTS").is_some() {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, &actual).unwrap();
                }
                assert_eq!(
                    std::fs::read_to_string(&path).expect(
                        "snapshot missing; inspect and regenerate with CLACK_UPDATE_SNAPSHOTS=1"
                    ),
                    actual,
                    "{}",
                    path.display()
                );
            }
        }
    }
}
