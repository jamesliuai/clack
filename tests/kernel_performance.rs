use clack::engine::{Action, Backspace, Engine, Mode, Rules, TestSpec, ZEN_WINDOW};

#[test]
fn distant_words_have_no_input_buffers() {
    let text = "a ".repeat(10_000);
    let allocations = allocation_counter::measure(|| {
        let engine = Engine::new(TestSpec::default(), &text).unwrap();
        assert!(
            engine.tokens()[4..]
                .iter()
                .all(|token| token.entered.capacity() == 0)
        );
        std::hint::black_box(engine);
    });
    assert!(allocations.bytes_max < 4 * 1024 * 1024, "{allocations:?}");
}

#[test]
fn combining_associated_text_reserves_graphemes_only_in_reachable_words() {
    let mut engine = Engine::new(TestSpec::default(), &"a ".repeat(10_000)).unwrap();
    let text = format!("e{}", "\u{301}".repeat(31)).repeat(128);
    let allocations = allocation_counter::measure(|| engine.prepare_text(&text));
    assert!(allocations.bytes_max < 64 * 1024, "{allocations:?}");
    assert!(
        engine.tokens()[4..]
            .iter()
            .all(|token| token.entered.capacity() == 0)
    );
    assert!(engine.tokens()[0].entered.capacity() >= 128);
}

#[test]
fn prepared_events_stay_allocation_free_across_long_and_short_words() {
    let text = format!("{} {} tail", "a ".repeat(500), "b".repeat(1024));
    let mut engine = Engine::new(TestSpec::default(), &text).unwrap();
    for (receipt, payload) in std::iter::repeat_n("a ", 500)
        .chain(std::iter::repeat_n("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", 32))
        .chain(["xxxx", " ", "tail"])
        .enumerate()
    {
        engine.prepare_input();
        engine.prepare_text(payload);
        let allocations = allocation_counter::measure(|| {
            engine.apply(Action::Text(payload), receipt as u64);
        });
        assert_eq!(allocations.count_total, 0, "{allocations:?}");
    }
}

#[test]
fn appending_lookahead_before_typing_prepares_the_entire_commit_stack() {
    let mut engine = Engine::new(TestSpec::default(), &"a ".repeat(8)).unwrap();
    engine.append_prepared(&"b ".repeat(100)).unwrap();
    for index in 0..105 {
        let payload = if index < 8 { "a " } else { "b " };
        engine.prepare_text(payload);
        let allocations = allocation_counter::measure(|| {
            engine.apply(Action::Text(payload), index);
        });
        assert_eq!(allocations.count_total, 0, "event {index}: {allocations:?}");
    }
}

#[test]
fn simulated_hour_keeps_typed_word_storage_small_and_reducer_allocation_free() {
    let text = "cat ".repeat(10_000);
    let allocations = allocation_counter::measure(|| {
        let mut engine = Engine::new(
            TestSpec {
                seconds: 3600,
                ..Default::default()
            },
            &text,
        )
        .unwrap();
        for index in 0..9000 {
            engine.prepare_input();
            engine.prepare_text("cat ");
            let reducer = allocation_counter::measure(|| {
                engine.apply(Action::Text("cat "), index * 400_000);
            });
            assert_eq!(reducer.count_total, 0, "{reducer:?}");
        }
        engine.apply(Action::Tick, 3_600_000_000);
        assert_eq!(engine.elapsed_us(), 3_600_000_000);
        assert_eq!(engine.counts().attempts_total, 36_000);
    });
    assert!(allocations.bytes_max < 16 * 1024 * 1024, "{allocations:?}");
    assert_eq!(allocations.bytes_current, 0, "{allocations:?}");
}

#[test]
fn associated_text_and_long_auto_indentation_are_prepared_outside_apply() {
    let spec = TestSpec {
        mode: Mode::Code,
        policy: clack::engine::Policy::Exact,
        auto_indent: true,
        ..Default::default()
    };
    let text = format!("x\n{}tail", " ".repeat(4096));
    let mut engine = Engine::new(spec, &text).unwrap();
    engine.prepare_text("x\n");
    let allocations = allocation_counter::measure(|| engine.apply(Action::Text("x\n"), 0));
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
    assert_eq!(engine.counts().retained_units, 2);
    assert_eq!(engine.tokens()[1].entered.len(), 4096);

    let mut spec = engine.spec().clone();
    spec.completion = clack::engine::Completion::Confirm;
    let target = format!("x\n{}", " ".repeat(4096));
    let mut trailing = Engine::new(spec, &target).unwrap();
    trailing.prepare_text("x\nz");
    let allocations = allocation_counter::measure(|| trailing.apply(Action::Text("x\nz"), 0));
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
    assert_eq!(trailing.tokens()[1].entered.len(), 4097);
}

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
