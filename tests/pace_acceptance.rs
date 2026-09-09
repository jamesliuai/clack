use clack::{
    engine::{Action, Engine, TestSpec},
    ui::{self, Appearance, ColorPolicy, Focus, Geometry, Status, Viewport},
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Modifier};

#[test]
fn pace_obeys_logical_position_viewport_and_user_caret_without_changing_scores() {
    let target = "cat dog ant bee eel fox ".repeat(100);
    let mut engine = Engine::new(
        TestSpec {
            pace_wpm: Some(120.0),
            ..TestSpec::default()
        },
        &target,
    )
    .unwrap();
    engine.apply(Action::Text("c"), 0);
    let appearance = Appearance {
        color: ColorPolicy::Always,
        focus: Focus::Always,
        ..Appearance::default()
    };
    let geometry = Geometry::new(Rect::new(0, 0, 80, 24), &appearance).unwrap();
    let mut viewport = Viewport::default();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    for (elapsed, position) in [(0, 0), (100_000, 1), (2_000_000, 20)] {
        let before = serde_json::to_value(engine.snapshot()).unwrap();
        terminal
            .draw(|frame| {
                ui::render(
                    frame,
                    &engine,
                    &appearance,
                    &Status::default(),
                    &mut viewport,
                    elapsed,
                    None,
                );
            })
            .unwrap();
        let cell = &terminal.backend().buffer()[(geometry.text.x + position, geometry.text.y)];
        assert_eq!(
            cell.modifier.contains(Modifier::BOLD),
            position == 1,
            "the user caret wins overlap at logical unit1"
        );
        assert_eq!(viewport.anchor_row, 0);
        assert_eq!(serde_json::to_value(engine.snapshot()).unwrap(), before);
    }
    engine.apply(Action::Text(&target[1..500]), 1_000_000);
    let before = serde_json::to_value(engine.snapshot()).unwrap();
    for elapsed in [2_100_000, 25_000_000] {
        terminal
            .draw(|frame| {
                ui::render(
                    frame,
                    &engine,
                    &appearance,
                    &Status::default(),
                    &mut viewport,
                    elapsed,
                    None,
                );
            })
            .unwrap();
        assert!(viewport.anchor_row > 0);
        let anchor = viewport.anchor_row;
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .filter(|cell| cell.modifier.contains(Modifier::UNDERLINED))
                .all(|cell| cell.modifier.contains(Modifier::BOLD)),
            "pace outside the user's viewport stays hidden"
        );
        assert_eq!(serde_json::to_value(engine.snapshot()).unwrap(), before);
        // A far-ahead pace must not move the viewport away from the user either.
        terminal
            .draw(|frame| {
                ui::render(
                    frame,
                    &engine,
                    &appearance,
                    &Status::default(),
                    &mut viewport,
                    300_000_000,
                    None,
                );
            })
            .unwrap();
        assert_eq!(viewport.anchor_row, anchor);
    }
}
