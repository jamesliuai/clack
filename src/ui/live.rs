//! Presentation clocks are independent of scoring and text-driven frames.
use crate::engine::{Engine, Metrics, Mode, State};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Values {
    pub metrics: Metrics,
    pub pace: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct LiveCache {
    metrics: Option<(u64, Metrics)>,
    pace: Option<(u64, usize)>,
}

fn due(previous: Option<u64>, now: u64, interval: u64) -> bool {
    previous.is_none_or(|previous| now < previous || now - previous >= interval)
}

impl LiveCache {
    pub(crate) fn preview(&self, engine: &Engine, elapsed: u64) -> Values {
        if engine.state() != State::Running {
            return Values::default();
        }
        let metrics = if due(self.metrics.map(|(at, _)| at), elapsed, 250_000) {
            Metrics::calculate(
                engine.counts(),
                elapsed,
                engine.spec().mode == Mode::Zen,
                true,
            )
        } else {
            self.metrics.expect("not due means an existing sample").1
        };
        let pace = engine.spec().pace_wpm.map(|wpm| {
            if due(self.pace.map(|(at, _)| at), elapsed, 100_000) {
                (wpm * 5.0 * elapsed as f64 / 60_000_000.0).floor() as usize
            } else {
                self.pace.expect("not due means an existing sample").1
            }
        });
        Values { metrics, pace }
    }

    pub(crate) fn update(&mut self, engine: &Engine, elapsed: u64) -> Values {
        let values = self.preview(engine, elapsed);
        if engine.state() != State::Running {
            *self = Self::default();
            return values;
        }
        if due(self.metrics.map(|(at, _)| at), elapsed, 250_000) {
            self.metrics = Some((elapsed, values.metrics));
        }
        if due(self.pace.map(|(at, _)| at), elapsed, 100_000) {
            self.pace = values.pace.map(|position| (elapsed, position));
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Action, TestSpec};

    #[test]
    fn rapid_text_frames_preserve_four_hertz_statistics_and_ten_hertz_pace() {
        let mut engine = Engine::new(
            TestSpec {
                pace_wpm: Some(1000.0),
                ..TestSpec::default()
            },
            &"cat dog ".repeat(100),
        )
        .unwrap();
        let mut cache = LiveCache::default();
        engine.apply(Action::Text("c"), 0);
        let first = cache.update(&engine, 0);
        let mut previous = first;
        let mut metric_at = 0;
        let mut pace_at = 0;
        for now in (10_000..=2_000_000).step_by(10_000) {
            engine.apply(Action::Text("x"), now);
            let values = cache.update(&engine, now);
            if values.metrics.accuracy != previous.metrics.accuracy
                || values.metrics.raw_wpm != previous.metrics.raw_wpm
            {
                assert!(now - metric_at >= 250_000);
                metric_at = now;
            }
            if values.pace != previous.pace {
                assert!(now - pace_at >= 100_000);
                assert_eq!(
                    values.pace,
                    Some((1000.0 * 5.0 * now as f64 / 60_000_000.0).floor() as usize)
                );
                pace_at = now;
            }
            previous = values;
        }
        assert_eq!(engine.counts().attempts_total, 201);
        assert!(previous.metrics.accuracy.unwrap() < first.metrics.accuracy.unwrap());
        assert_eq!(pace_at, 2_000_000);
    }
}
