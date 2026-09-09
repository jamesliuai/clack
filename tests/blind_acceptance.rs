use clack::{
    engine::{Action, Engine, Rules, TestSpec},
    ui::{Appearance, ColorDepth, Geometry, Theme, TypingWidget, Viewport},
};
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

#[test]
fn blind_hides_only_running_error_cues_and_reveals_them_without_changing_scores() {
    for depth in [ColorDepth::TrueColor, ColorDepth::Monochrome] {
        let appearance = Appearance::default();
        let theme = Theme::from_preferences(&appearance, depth);
        let area = Rect::new(0, 0, 80, 24);
        let geometry = Geometry::new(area, &appearance).unwrap();
        let mut ordinary = Engine::new(TestSpec::default(), "cat dog").unwrap();
        let mut blind = Engine::new(
            TestSpec {
                rules: Rules {
                    blind: true,
                    ..Rules::default()
                },
                ..TestSpec::default()
            },
            "cat dog",
        )
        .unwrap();
        ordinary.apply(Action::Text("cax!"), 0);
        blind.apply(Action::Text("cax!"), 0);
        ordinary.apply(Action::Tick, 1_000_000);
        blind.apply(Action::Tick, 1_000_000);
        assert_eq!(ordinary.counts(), blind.counts());
        assert_eq!(ordinary.metrics(), blind.metrics());
        assert_eq!(ordinary.spec().profile_key(), blind.spec().profile_key());
        let mut viewport = Viewport::default();
        viewport.sync(&blind, geometry.text.width, appearance.tab_stop);
        let draw = |engine: &Engine| {
            let before = serde_json::to_value(engine.snapshot()).unwrap();
            let mut buffer = Buffer::empty(area);
            TypingWidget {
                engine,
                viewport: &viewport,
                geometry,
                theme,
                appearance: &appearance,
                pace: None,
            }
            .render(geometry.text, &mut buffer);
            assert_eq!(serde_json::to_value(engine.snapshot()).unwrap(), before);
            buffer
        };
        let running = draw(&blind);
        let correct = &running[(geometry.text.x, geometry.text.y)];
        for (offset, symbol) in [(2, "x"), (3, "!")] {
            let cell = &running[(geometry.text.x + offset, geometry.text.y)];
            assert_eq!(cell.symbol(), symbol, "blind retains the entered text");
            assert_eq!(
                (cell.fg, cell.bg, cell.modifier),
                (correct.fg, correct.bg, correct.modifier)
            );
        }
        ordinary.apply(Action::Finish, 2_000_000);
        blind.apply(Action::Finish, 2_000_000);
        assert_eq!(ordinary.counts(), blind.counts());
        assert_eq!(ordinary.metrics(), blind.metrics());
        let finished = draw(&blind);
        let ordinary_finished = draw(&ordinary);
        assert_eq!(
            finished, ordinary_finished,
            "Results reveal the ordinary error cues"
        );
        for offset in [2, 3] {
            let cell = &finished[(geometry.text.x + offset, geometry.text.y)];
            assert_ne!(
                (cell.fg, cell.bg, cell.modifier),
                (correct.fg, correct.bg, correct.modifier)
            );
        }
    }
}
