//! Deterministic full-frame CPU work through Ratatui's in-memory backend.
//! This does not measure a terminal, a keyboard, or application event scheduling.
use clack::{
    engine::{Action, Engine, TestSpec},
    ui::{self, Appearance, ColorPolicy, Status, Viewport},
};
use ratatui::{Terminal, backend::TestBackend};
use std::{hint::black_box, time::Instant};
use unicode_segmentation::UnicodeSegmentation;

fn main() {
    let runs = std::env::args()
        .nth(1)
        .map(|value| value.parse::<usize>().expect("integer runs"))
        .unwrap_or(30);
    assert!((1..=1000).contains(&runs));
    let mut reports = Vec::new();
    for (width, height) in [(80, 24), (120, 40), (200, 60)] {
        for (label, passage) in [
            (
                "ascii",
                "the world can change when small things become part of the way we work ",
            ),
            (
                "mixed_width",
                "café 界猫 résumé déjà élève Straße coöperate ",
            ),
        ] {
            let target = passage.repeat(64);
            let input: Vec<&str> = target.graphemes(true).take(375).collect();
            let appearance = Appearance {
                color: ColorPolicy::Never,
                ..Appearance::default()
            };
            let mut timings = Vec::with_capacity(runs * (input.len() + 2));
            let mut flushes = 0;
            let mut layouts = 0;
            for run in 0..=runs {
                let mut engine = Engine::new(TestSpec::default(), &target).unwrap();
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                let mut viewport = Viewport::default();
                for index in 0..input.len() + 2 {
                    let elapsed = if index == 0 {
                        0
                    } else if index == input.len() + 1 {
                        30_000_000
                    } else {
                        (index - 1) as u64 * 80_000
                    };
                    if index > 0 && index <= input.len() {
                        engine.apply(Action::Text(input[index - 1]), 1_000_000 + elapsed);
                    } else if index == input.len() + 1 {
                        engine.apply(Action::Tick, 31_000_000);
                    }
                    let before = Instant::now();
                    terminal
                        .draw(|frame| {
                            let _ = black_box(ui::render(
                                frame,
                                &engine,
                                &appearance,
                                &Status::default(),
                                &mut viewport,
                                elapsed,
                                None,
                            ));
                        })
                        .unwrap();
                    if run > 0 {
                        timings.push(before.elapsed().as_nanos() as u64);
                        flushes += 1;
                    }
                }
                black_box(terminal.backend().buffer());
                if run > 0 {
                    layouts += viewport.layout_rebuilds;
                }
            }
            timings.sort_unstable();
            reports.push(serde_json::json!({"geometry":[width,height],"input":label,"runs":runs,"warmup_runs_excluded":1,"frames":flushes,"layout_rebuilds":layouts,"frame_p50_ns":timings[timings.len()/2],"frame_p95_ns":timings[timings.len()*95/100],"frame_p99_ns":timings[timings.len()*99/100],"total_measured_render_ns":timings.iter().sum::<u64>()}));
        }
    }
    println!(
        "{}",
        serde_json::json!({"benchmark":"in_memory_full_frame_v1","profile":"release, explicit artifact/compiler metadata in enclosing report","trace":"375 grapheme events at simulated80ms receipt intervals, Ready and timed Results; no wall-clock replay delay","backend":"Ratatui TestBackend; complete frame+diff+in-memory flush; excludes engine apply and preparation from per-frame timings","measurements":reports})
    );
}
