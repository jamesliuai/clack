use allocation_counter::AllocationInfo;
use clack::engine::{Action, Backspace, Counts, Engine, Mode, Outcome, Rules, State, TestSpec};

// These are observable retention limits, independently checked against the
// complete reference transcript rather than imported from the implementation.
const EDITABLE_UNITS: usize = 4096;
const CHART_SAMPLES: usize = 3601;
const SECOND: u64 = 1_000_000;
const ORIGIN: u64 = 97_123_456;
const WARM_SECONDS: u64 = 4001;
const CYCLE_SECONDS: u64 = 3701;
const CYCLES: usize = 6;
const FULL_SECONDS: u64 = WARM_SECONDS + CYCLE_SECONDS * CYCLES as u64;
const TAIL_US: u64 = 375_000;
const BURST: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

struct Pattern {
    first: &'static str,
    second: &'static str,
    // Literal graphemes form the independent oracle. In particular, two
    // separate terminal events below deliver the one decomposed accent unit.
    units: &'static [&'static str],
}

fn pattern(second: u64) -> Option<Pattern> {
    if second % 13 == 8 {
        return None;
    }
    Some(match second % 5 {
        0 => Pattern {
            first: "abcd ",
            second: "",
            units: &["a", "b", "c", "d", " "],
        },
        1 => Pattern {
            first: "é界🙂 ",
            second: "",
            units: &["é", "界", "🙂", " "],
        },
        2 => Pattern {
            first: "e",
            second: "\u{301} z\t",
            units: &["e\u{301}", " ", "z", "\t"],
        },
        3 => Pattern {
            first: "quick fox\n",
            second: "",
            units: &["q", "u", "i", "c", "k", " ", "f", "o", "x", "\n"],
        },
        _ => Pattern {
            first: "one two ",
            second: "",
            units: &["o", "n", "e", " ", "t", "w", "o", " "],
        },
    })
}

fn drain_before(second: u64) -> bool {
    second >= WARM_SECONDS && (second - WARM_SECONDS).is_multiple_of(CYCLE_SECONDS)
}

#[derive(Clone, Copy)]
struct ExpectedSample {
    bucket: u64,
    duration_us: u64,
    elapsed_us: u64,
    attempts: u64,
    counts: Counts,
}

struct Checkpoint {
    readonly_units: usize,
    editable: Vec<&'static str>,
}

struct Oracle {
    samples: Vec<ExpectedSample>,
    checkpoints: Vec<Checkpoint>,
    final_editable: Vec<&'static str>,
    final_readonly: usize,
}

// The oracle deliberately keeps the complete surviving transcript in a Vec.
// Its immutable prefix is an absolute logical position, not a ring buffer.
// All oracle allocation happens before the measured engine lifetime starts.
fn reference() -> Oracle {
    let mut output = vec!["^"];
    let mut readonly = 0;
    let mut attempts = 1;
    let mut deletions = 0;
    let mut previous_attempts = 0;
    let mut samples = Vec::with_capacity(FULL_SECONDS as usize + 1);
    let mut checkpoints = Vec::with_capacity(CYCLES + 1);
    for second in 0..FULL_SECONDS {
        if drain_before(second) {
            deletions += (output.len() - readonly) as u64;
            output.truncate(readonly);
        }
        if let Some(pattern) = pattern(second) {
            for &unit in pattern.units {
                output.push(unit);
                attempts += 1;
                readonly = readonly.max(output.len().saturating_sub(EDITABLE_UNITS));
            }
            if second < 20 {
                for _ in 0..32 {
                    output.push("x");
                    attempts += 1;
                    readonly = readonly.max(output.len().saturating_sub(EDITABLE_UNITS));
                }
            }
            if second % 7 == 2 {
                for _ in 0..2 {
                    if output.len() > readonly {
                        output.pop();
                        deletions += 1;
                    }
                }
                output.push("r");
                attempts += 1;
                readonly = readonly.max(output.len().saturating_sub(EDITABLE_UNITS));
            }
        }
        samples.push(ExpectedSample {
            bucket: second,
            duration_us: SECOND,
            elapsed_us: (second + 1) * SECOND,
            attempts: attempts - previous_attempts,
            counts: Counts {
                attempts_total: attempts,
                deletion_count: deletions,
                retained_units: output.len() as u64,
                ..Counts::default()
            },
        });
        previous_attempts = attempts;
        let elapsed_seconds = second + 1;
        if elapsed_seconds >= WARM_SECONDS
            && (elapsed_seconds - WARM_SECONDS).is_multiple_of(CYCLE_SECONDS)
        {
            checkpoints.push(Checkpoint {
                readonly_units: readonly,
                editable: output[readonly..].to_vec(),
            });
        }
    }
    output.push("e\u{301}");
    attempts += 1;
    readonly = readonly.max(output.len().saturating_sub(EDITABLE_UNITS));
    samples.push(ExpectedSample {
        bucket: FULL_SECONDS,
        duration_us: TAIL_US,
        elapsed_us: FULL_SECONDS * SECOND + TAIL_US,
        attempts: 1,
        counts: Counts {
            attempts_total: attempts,
            deletion_count: deletions,
            retained_units: output.len() as u64,
            ..Counts::default()
        },
    });
    Oracle {
        samples,
        checkpoints,
        final_editable: output[readonly..].to_vec(),
        final_readonly: readonly,
    }
}

fn close(actual: Option<f64>, expected: f64) {
    let actual = actual.expect("a positive-duration Zen speed must be present");
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

fn check_sample(actual: &clack::engine::Sample, expected: &ExpectedSample) {
    assert_eq!(actual.bucket_index, expected.bucket);
    assert_eq!(actual.duration_us, expected.duration_us);
    assert_eq!(
        actual.bucket_index * SECOND + actual.duration_us,
        expected.elapsed_us
    );
    assert_eq!(actual.counts, expected.counts);
    assert_eq!(actual.attempts, expected.attempts);
    assert_eq!(actual.errors, 0);
    assert_eq!(actual.metrics.wpm, None);
    assert_eq!(actual.metrics.accuracy, None);
    assert_eq!(actual.metrics.cpm, None);
    let elapsed_seconds = expected.elapsed_us as f64 / SECOND as f64;
    close(
        actual.metrics.raw_wpm,
        60.0 * expected.counts.retained_units as f64 / (5.0 * elapsed_seconds),
    );
}

fn check_chart(engine: &Engine, expected: &[ExpectedSample]) {
    let first = expected.len().saturating_sub(CHART_SAMPLES);
    assert_eq!(engine.samples().len(), expected[first..].len());
    for (actual, expected) in engine.samples().iter().zip(&expected[first..]) {
        check_sample(actual, expected);
    }
}

fn check_text(engine: &Engine, expected: &[&str], readonly: usize) {
    assert_eq!(engine.zen_discarded(), readonly as u64);
    assert_eq!(engine.zen_window().len(), expected.len());
    for (actual, expected) in engine.zen_window().iter().zip(expected) {
        assert_eq!(actual.unit.as_str(), *expected);
        assert!(actual.correct && !actual.assisted);
    }
    assert_eq!(
        engine.counts().retained_units,
        (readonly + expected.len()) as u64
    );
}

fn drive_bucket(engine: &mut Engine, second: u64, oracle: &Oracle) {
    let at = ORIGIN + second * SECOND;
    if drain_before(second) {
        let checkpoint = &oracle.checkpoints[((second - WARM_SECONDS) / CYCLE_SECONDS) as usize];
        let mut expected = oracle.samples[second as usize - 1].counts;
        expected.retained_units -= checkpoint.editable.len() as u64;
        expected.deletion_count += checkpoint.editable.len() as u64;
        // Extra deletion requests must stop at the immutable prefix. This tests
        // edit permission after eviction, not merely a container's reported cap.
        for _ in 0..EDITABLE_UNITS + 17 {
            engine.apply(Action::Backspace, at + 10_000);
        }
        assert!(engine.zen_window().is_empty());
        assert_eq!(engine.counts(), expected);
        assert_eq!(engine.zen_discarded(), checkpoint.readonly_units as u64);
    }
    if let Some(pattern) = pattern(second) {
        engine.apply(Action::Text(pattern.first), at + 100_000);
        if !pattern.second.is_empty() {
            engine.apply(Action::Text(pattern.second), at + 300_000);
        }
        if second < 20 {
            engine.apply(Action::Text(BURST), at + 500_000);
        }
        if second % 7 == 2 {
            engine.apply(Action::Backspace, at + 700_000);
            engine.apply(Action::Backspace, at + 700_000);
            engine.apply(Action::Text("r"), at + 800_000);
        }
    }
    engine.apply(Action::Tick, at + SECOND);
    let expected = &oracle.samples[second as usize];
    assert_eq!(engine.state(), State::Running);
    assert_eq!(engine.elapsed_us(), expected.elapsed_us);
    assert_eq!(engine.counts(), expected.counts);
    assert!(engine.zen_window().len() <= EDITABLE_UNITS);
    check_sample(engine.samples().back().unwrap(), expected);
}

#[test]
fn zen_text_and_chart_cycle_repeatedly_with_flat_live_allocation_and_exact_history() {
    let oracle = reference();
    assert_eq!(oracle.checkpoints.len(), CYCLES + 1);
    assert!(oracle.final_readonly > 20 * EDITABLE_UNITS);
    assert_eq!(BURST.len(), 32);
    let mut warming = AllocationInfo::default();
    let mut cycles = [AllocationInfo::default(); CYCLES];
    let lifetime = allocation_counter::measure(|| {
        let mut warmed_engine = None;
        warming = allocation_counter::measure(|| {
            let mut engine = Engine::new(
                TestSpec {
                    mode: Mode::Zen,
                    rules: Rules {
                        backspace: Backspace::Full,
                        ..Rules::default()
                    },
                    ..TestSpec::default()
                },
                "",
            )
            .unwrap();
            engine.apply(Action::Text("^"), ORIGIN);
            for second in 0..WARM_SECONDS {
                drive_bucket(&mut engine, second, &oracle);
            }
            warmed_engine = Some(engine);
        });
        assert!(
            warming.bytes_current > 0,
            "the allocator must observe the live engine"
        );
        let mut engine = warmed_engine.unwrap();
        assert_eq!(engine.started_at(), Some(ORIGIN));
        assert_eq!(engine.deadline(), None);
        assert_eq!(engine.zen_window().len(), EDITABLE_UNITS);
        assert_eq!(engine.samples().len(), CHART_SAMPLES);
        for (cycle, allocations) in cycles.iter_mut().enumerate() {
            *allocations = allocation_counter::measure(|| {
                let start = WARM_SECONDS + cycle as u64 * CYCLE_SECONDS;
                let end = start + CYCLE_SECONDS;
                for second in start..end {
                    drive_bucket(&mut engine, second, &oracle);
                }
                let checkpoint = &oracle.checkpoints[cycle + 1];
                check_text(&engine, &checkpoint.editable, checkpoint.readonly_units);
                check_chart(&engine, &oracle.samples[..end as usize]);
            });
            assert_eq!(
                allocations.bytes_current, 0,
                "cycle {cycle}: {allocations:?}"
            );
            assert_eq!(
                allocations.count_current, 0,
                "cycle {cycle}: {allocations:?}"
            );
            assert_eq!(allocations.bytes_max, 0, "cycle {cycle}: {allocations:?}");
        }
        // Finishing after the chart has wrapped must retain its fractional
        // endpoint, replacing one old sample rather than resetting its clock.
        let last_second = ORIGIN + FULL_SECONDS * SECOND;
        engine.apply(Action::Text("e"), last_second + 125_000);
        engine.apply(Action::Text("\u{301}"), last_second + 250_000);
        engine.apply(Action::Finish, last_second + TAIL_US);
        assert_eq!(engine.state(), State::Results);
        assert_eq!(engine.outcome(), Outcome::Complete);
        assert_eq!(engine.counts(), oracle.samples.last().unwrap().counts);
        check_text(&engine, &oracle.final_editable, oracle.final_readonly);
        check_chart(&engine, &oracle.samples);
        // Consistency covers all complete buckets, including the initial burst
        // long since evicted from the chart, and excludes the fractional tail.
        let full = &oracle.samples[..FULL_SECONDS as usize];
        let sum: u64 = full.iter().map(|sample| sample.attempts).sum();
        let squares: u64 = full.iter().map(|sample| sample.attempts.pow(2)).sum();
        let mean = sum as f64 / FULL_SECONDS as f64;
        let variance = squares as f64 / FULL_SECONDS as f64 - mean * mean;
        close(
            engine.metrics().consistency,
            100.0 / (1.0 + variance.sqrt() / mean),
        );
        let result = engine.snapshot();
        assert_eq!(result.elapsed_us, FULL_SECONDS * SECOND + TAIL_US);
        assert_eq!(result.samples.len(), CHART_SAMPLES);
        assert_eq!(result.counts, engine.counts());
        assert!(!result.personal_best_eligible);
        assert!(result.words.is_empty());
        for (actual, expected) in result
            .samples
            .iter()
            .zip(&oracle.samples[oracle.samples.len() - CHART_SAMPLES..])
        {
            check_sample(actual, expected);
        }
    });
    assert_eq!(lifetime.bytes_current, 0, "{lifetime:?}");
    assert_eq!(lifetime.count_current, 0, "{lifetime:?}");
    println!(
        "Injected {} full seconds plus {} us; {} cycles after saturation; {} readonly output units; warm live bytes {}; full lifetime released",
        FULL_SECONDS, TAIL_US, CYCLES, oracle.final_readonly, warming.bytes_current
    );
    for (cycle, allocations) in cycles.iter().enumerate() {
        println!("cycle {cycle}: {allocations:?}");
    }
}
