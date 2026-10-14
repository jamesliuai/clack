//! Deterministic reducer benchmark; this measures no terminal behavior.
use clack::engine::{Action, Backspace, Engine, Rules, TestSpec};
use std::{hint::black_box, time::Instant};
fn main() {
    let text = "cat dog ".repeat(10_000);
    let spec = TestSpec {
        rules: Rules {
            backspace: Backspace::Full,
            ..Rules::default()
        },
        ..TestSpec::default()
    };
    let mut engine = Engine::new(spec, &text).expect("target");
    engine.apply(Action::Text("c"), 0);
    let mut timings = Vec::with_capacity(50_000);
    let actions = [
        Action::Text("a"),
        Action::Text("x"),
        Action::Backspace,
        Action::Text("t"),
        Action::Text(" "),
        Action::Text("d"),
        Action::Text("o"),
        Action::Text("g"),
        Action::Text(" "),
        Action::Text("c"),
    ];
    let mut allocation_count = 0;
    let mut allocated_bytes = 0;
    for index in 0..50_000 {
        if let Action::Text(text) = actions[index % actions.len()] {
            engine.prepare_input();
            engine.prepare_text(text);
        }
        let allocations = allocation_counter::measure(|| {
            let before = Instant::now();
            engine.apply(
                black_box(actions[index % actions.len()]),
                index as u64 * 100,
            );
            timings.push(before.elapsed().as_nanos() as u64);
        });
        allocation_count += allocations.count_total;
        allocated_bytes += allocations.bytes_total;
    }
    timings.sort_unstable();
    println!(
        "{}",
        serde_json::json!({
            "benchmark":"warm_ascii_reducer_v1", "events":timings.len(), "p50_ns":timings[timings.len()/2],
            "p95_ns":timings[timings.len()*95/100], "p99_ns":timings[timings.len()*99/100],
            "allocations":allocation_count, "allocated_bytes":allocated_bytes,
            "preparation":"input buffers warmed outside reducer timing and allocation measurement",
            "final_counts":engine.counts(), "timer_instrumentation":"Instant around each apply; no terminal I/O; no established controlled-machine baseline"
        })
    );
}
