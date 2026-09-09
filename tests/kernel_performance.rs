use clack::engine::{Action, Backspace, Engine, Mode, Rules, TestSpec, ZEN_WINDOW};

#[test]
fn large_practice_words_share_target_metadata_and_allocate_correction_on_demand() {
    // A legal custom word can be repeated 25 times by practice. Eagerly cloning
    // target metadata and correction buffers would consume over 400 MiB here.
    let word = "a".repeat(65_536);
    let text = vec![word.as_str(); 25].join(" ");
    let allocations = allocation_counter::measure(|| {
        let mut engine = Engine::new(
            TestSpec {
                mode: Mode::Words,
                words: 25,
                ..TestSpec::default()
            },
            &text,
        )
        .unwrap();
        assert_eq!(engine.tokens().len(), 25);
        engine.apply(Action::Text(&word), 0);
        engine.apply(Action::Text(" "), 1);
        assert_eq!(engine.current_token(), 1);
        assert_eq!(engine.counts().retained_units, 65_537);
    });
    assert!(allocations.bytes_max < 40 * 1024 * 1024, "{allocations:?}");
    assert_eq!(allocations.bytes_current, 0, "{allocations:?}");
}

#[test]
fn warmed_ascii_reducer_has_no_heap_allocations() {
    let text = "cat dog ".repeat(1000);
    let mut engine = Engine::new(
        TestSpec {
            rules: Rules {
                backspace: Backspace::Full,
                ..Rules::default()
            },
            ..TestSpec::default()
        },
        &text,
    )
    .unwrap();
    engine.apply(Action::Text("c"), 0);
    let allocations = allocation_counter::measure(|| {
        engine.apply(Action::Text("ax"), 1);
        engine.apply(Action::Backspace, 2);
        engine.apply(Action::Text("t "), 3);
        engine.apply(Action::Text("dog "), 4);
        engine.apply(Action::Backspace, 5);
        engine.apply(Action::DeleteWord, 6);
        engine.apply(Action::Text("dog "), 7);
        engine.apply(Action::Tick, 1_000_000);
        std::hint::black_box(engine.counts());
    });
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
}

#[test]
fn zen_scroll_window_cycles_without_losing_cumulative_counts() {
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
    for i in 0..50_000 {
        engine.apply(Action::Text("a"), i * 10);
    }
    assert_eq!(engine.zen_window().len(), ZEN_WINDOW);
    assert_eq!(engine.counts().retained_units, 50_000);
    for _ in 0..ZEN_WINDOW + 20 {
        engine.apply(Action::Backspace, 600_000);
    }
    assert_eq!(engine.counts().retained_units, 50_000 - ZEN_WINDOW as u64);
    assert_eq!(engine.counts().attempts_total, 50_000);
    assert_eq!(engine.zen_window().len(), 0);
}

#[test]
fn successive_runs_release_all_engine_allocations() {
    let spec = TestSpec::default();
    let allocations = allocation_counter::measure(|| {
        for _ in 0..1000 {
            let mut engine = Engine::new(spec.clone(), "cat dog").unwrap();
            engine.apply(Action::Text("cat d"), 0);
            engine.apply(Action::Tick, 30_000_000);
            std::hint::black_box(engine.snapshot());
        }
    });
    assert_eq!(allocations.bytes_current, 0, "{allocations:?}");
    assert_eq!(allocations.count_current, 0, "{allocations:?}");
}

#[test]
fn zen_correction_respects_submitted_word_barriers_and_full_window() {
    for policy in [
        Backspace::Mistakes,
        Backspace::Current,
        Backspace::Full,
        Backspace::None,
    ] {
        let mut engine = Engine::new(
            TestSpec {
                mode: Mode::Zen,
                rules: Rules {
                    backspace: policy,
                    ..Rules::default()
                },
                ..TestSpec::default()
            },
            "",
        )
        .unwrap();
        engine.apply(Action::Text("word "), 0);
        engine.apply(Action::Backspace, 100);
        assert_eq!(
            engine.counts().retained_units,
            if policy == Backspace::Full { 4 } else { 5 }
        );
        engine.apply(Action::Text("a"), 200);
        engine.apply(Action::Backspace, 300);
        assert_eq!(
            engine.counts().retained_units,
            if policy == Backspace::Full {
                4
            } else if policy == Backspace::None {
                6
            } else {
                5
            }
        );
    }
}

#[test]
fn generator_parameters_roundtrip_and_only_effective_parameters_split_profiles() {
    let spec = TestSpec {
        punctuation: true,
        ..TestSpec::default()
    };
    let mut changed = spec.clone();
    changed.generator_parameters.comma_percent = 20;
    assert_ne!(spec.profile_key(), changed.profile_key());
    let json = serde_json::to_string(&changed).unwrap();
    let restored: TestSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(changed, restored);
    let mut inactive = TestSpec::default();
    inactive.generator_parameters.comma_percent = 30;
    assert_eq!(TestSpec::default().profile_key(), inactive.profile_key());
}
