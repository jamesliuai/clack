//! Constant-time comparison of the scored view, without formatting or allocation.
use crate::{
    config::Config,
    engine::{Engine, Mode, State, Unit},
    ui::{Focus, SpeedUnit},
};

#[derive(Debug, PartialEq)]
pub(super) struct Presentation {
    state: State,
    token: usize,
    length: usize,
    tail: Option<(Unit, bool, bool)>,
    discarded: u64,
    progress: Option<u64>,
    speed: Option<f64>,
    accuracy: Option<f64>,
    pace: Option<usize>,
}
impl Presentation {
    #[cfg(test)]
    pub(super) fn capture(engine: &Engine, config: &Config, elapsed: u64) -> Self {
        Self::capture_live(
            engine,
            config,
            elapsed,
            crate::ui::live::LiveCache::default().preview(engine, elapsed),
        )
    }
    pub(super) fn same_text(&self, other: &Self) -> bool {
        self.state == other.state
            && self.token == other.token
            && self.length == other.length
            && self.tail == other.tail
            && self.discarded == other.discarded
    }
    pub(super) fn capture_live(
        engine: &Engine,
        config: &Config,
        elapsed: u64,
        values: crate::ui::live::Values,
    ) -> Self {
        let zen = engine.spec().mode == Mode::Zen;
        let entries = engine.tokens().get(engine.current_token());
        let (length, tail) = if zen {
            (engine.zen_window().len(), engine.zen_window().back())
        } else {
            entries.map_or((0, None), |token| {
                (token.entered.len(), token.entered.last())
            })
        };
        let status = engine.state() == State::Running && config.appearance.focus != Focus::Always;
        let metrics = values.metrics;
        let speed = if zen { metrics.raw_wpm } else { metrics.wpm };
        Self {
            state: engine.state(),
            token: engine.current_token(),
            length,
            tail: tail.map(|entry| (entry.unit, entry.correct, entry.assisted)),
            discarded: engine.zen_discarded(),
            progress: (status && config.status.progress).then(|| match engine.spec().mode {
                Mode::Time => (u64::from(engine.spec().seconds) * 1_000_000)
                    .saturating_sub(elapsed)
                    .div_ceil(1_000_000),
                Mode::Words => engine.current_token() as u64,
                _ => elapsed / 1_000_000,
            }),
            speed: if status && config.status.wpm {
                speed.map(|value| {
                    (value
                        * if config.status.speed_unit == SpeedUnit::Cpm {
                            5.0
                        } else {
                            1.0
                        })
                    .round_ties_even()
                    .min(100_000.0)
                })
            } else {
                None
            },
            accuracy: if status && config.status.accuracy && !zen {
                metrics
                    .accuracy
                    .map(|value| (value * 10.0).round_ties_even())
            } else {
                None
            },
            pace: values.pace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Action, Backspace, Rules, StopOnError, TestSpec};

    #[test]
    fn ignored_input_and_rejected_hidden_errors_do_not_change_the_view() {
        let mut config = Config::default();
        config.appearance.focus = Focus::Always;
        let mut engine = Engine::new(
            TestSpec {
                rules: Rules {
                    backspace: Backspace::None,
                    stop_on_error: StopOnError::Letter,
                    ..Rules::default()
                },
                ..TestSpec::default()
            },
            "cat dog",
        )
        .unwrap();
        let ready = Presentation::capture(&engine, &config, 0);
        for action in [
            Action::Text("  "),
            Action::Backspace,
            Action::DeleteWord,
            Action::Finish,
        ] {
            engine.apply(action, 0);
            assert_eq!(ready, Presentation::capture(&engine, &config, 0));
        }
        engine.apply(Action::Text("c"), 0);
        let running = Presentation::capture(&engine, &config, 0);
        assert_ne!(ready, running);
        for action in [
            Action::Text("x"),
            Action::Backspace,
            Action::DeleteWord,
            Action::Tick,
        ] {
            engine.apply(action, 1);
            assert_eq!(running, Presentation::capture(&engine, &config, 1));
        }
        assert_eq!(engine.counts().attempts_total, 2);
    }

    #[test]
    fn provisional_unicode_and_visible_accuracy_changes_request_a_frame() {
        let mut config = Config::default();
        config.status.accuracy = true;
        let mut engine = Engine::new(TestSpec::default(), "éclair dog").unwrap();
        engine.apply(Action::Text("e"), 0);
        let before = Presentation::capture(&engine, &config, 0);
        engine.apply(Action::Text("\u{301}"), 0);
        assert_ne!(before, Presentation::capture(&engine, &config, 0));
        let mut engine = Engine::new(
            TestSpec {
                rules: Rules {
                    stop_on_error: StopOnError::Letter,
                    ..Rules::default()
                },
                ..TestSpec::default()
            },
            "cat dog",
        )
        .unwrap();
        engine.apply(Action::Text("c"), 0);
        let before = Presentation::capture(&engine, &config, 0);
        engine.apply(Action::Text("x"), 0);
        assert_ne!(before, Presentation::capture(&engine, &config, 0));
    }

    #[test]
    fn periodic_checks_follow_display_precision_and_pace_in_full_focus() {
        let mut config = Config::default();
        config.status.wpm = false;
        config.status.accuracy = false;
        let mut engine = Engine::new(TestSpec::default(), "cat dog").unwrap();
        engine.apply(Action::Text("c"), 0);
        assert_eq!(
            Presentation::capture(&engine, &config, 1),
            Presentation::capture(&engine, &config, 999_999)
        );
        assert_ne!(
            Presentation::capture(&engine, &config, 1),
            Presentation::capture(&engine, &config, 1_000_000)
        );
        config.appearance.focus = Focus::Always;
        let mut engine = Engine::new(
            TestSpec {
                pace_wpm: Some(120.0),
                ..TestSpec::default()
            },
            "cat dog",
        )
        .unwrap();
        engine.apply(Action::Text("c"), 0);
        assert_eq!(
            Presentation::capture(&engine, &config, 0),
            Presentation::capture(&engine, &config, 99_999)
        );
        assert_ne!(
            Presentation::capture(&engine, &config, 0),
            Presentation::capture(&engine, &config, 100_000)
        );
    }
}
