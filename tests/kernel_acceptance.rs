//! Specification-derived fixtures. Expected counts come from the published
//! examples and observable typing semantics, not a second reducer.

use clack::engine::{
    Action, Backspace, Completion, Counts, Difficulty, Engine, Metrics, Mode, Outcome, Policy,
    Rules, State, StopOnError, TestSpec, ZEN_WINDOW,
};
use proptest::prelude::*;

const ORIGIN: u64 = 7_000_000;

fn timed(text: &str) -> Engine {
    Engine::new(TestSpec::default(), text).expect("valid fixture")
}

fn with_rules(text: &str, rules: Rules) -> Engine {
    Engine::new(
        TestSpec {
            rules,
            ..TestSpec::default()
        },
        text,
    )
    .expect("valid fixture")
}

fn exact(text: &str) -> Engine {
    Engine::new(
        TestSpec {
            mode: Mode::Code,
            policy: Policy::Exact,
            normalize: false,
            ..TestSpec::default()
        },
        text,
    )
    .expect("valid fixture")
}

fn enter(engine: &mut Engine, text: &str, start: u64) {
    for (i, character) in text.char_indices().enumerate() {
        let mut bytes = [0; 4];
        engine.apply(
            Action::Text(character.1.encode_utf8(&mut bytes)),
            start + i as u64 * 10_000,
        );
    }
}

#[test]
fn numeric_pace_assistance_cannot_enter_standard_personal_bests() {
    let mut plain = Engine::new(
        TestSpec {
            mode: Mode::Custom,
            ..TestSpec::default()
        },
        "cat",
    )
    .unwrap();
    let mut paced = Engine::new(
        TestSpec {
            mode: Mode::Custom,
            pace_wpm: Some(60.0),
            ..TestSpec::default()
        },
        "cat",
    )
    .unwrap();
    for engine in [&mut plain, &mut paced] {
        enter(engine, "cat", ORIGIN);
        assert_eq!(engine.outcome(), Outcome::Complete);
    }
    assert_eq!(
        plain.counts(),
        paced.counts(),
        "visual pace cannot change scoring"
    );
    assert!(plain.snapshot().personal_best_eligible);
    assert!(!paced.snapshot().personal_best_eligible);
    assert_ne!(plain.snapshot().profile_key, paced.snapshot().profile_key);
}

#[test]
fn final_associated_grapheme_is_whole_before_completion_and_difficulty_checks() {
    for (target, payload, attempts, correct) in
        [("ab", "ab\u{301}", 2, 1), ("👩", "👩\u{200d}💻", 1, 0)]
    {
        for difficulty in [Difficulty::Normal, Difficulty::Master] {
            let mut engine = Engine::new(
                TestSpec {
                    mode: Mode::Words,
                    words: 1,
                    rules: Rules {
                        difficulty,
                        ..Rules::default()
                    },
                    ..TestSpec::default()
                },
                target,
            )
            .unwrap();
            engine.apply(Action::Text(payload), ORIGIN);
            assert_eq!(output(&engine), payload);
            assert_eq!(engine.counts().attempts_total, attempts);
            assert_eq!(engine.counts().attempts_correct, correct);
            assert_eq!(engine.counts().retained_units, attempts);
            if difficulty == Difficulty::Master {
                assert_eq!(engine.state(), State::Results);
                assert_eq!(engine.outcome(), Outcome::Failed);
            } else {
                assert_eq!(engine.state(), State::Running);
            }
        }
    }
}

#[test]
fn exact_auto_completion_retains_the_entire_wrong_associated_cluster() {
    let mut engine = Engine::new(
        TestSpec {
            mode: Mode::Code,
            policy: Policy::Exact,
            completion: Completion::Auto,
            ..TestSpec::default()
        },
        "👩",
    )
    .unwrap();
    engine.apply(Action::Text("👩\u{200d}💻"), ORIGIN);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.outcome(), Outcome::Complete);
    assert_eq!(output(&engine), "👩\u{200d}💻");
    assert_eq!(engine.counts().attempts_total, 1);
    assert_eq!(engine.counts().attempts_correct, 0);
    assert_eq!(engine.counts().final_incorrect, 1);
}

#[test]
fn regional_indicator_boundary_uses_context_from_prior_associated_events() {
    let mut engine = Engine::new(
        TestSpec {
            mode: Mode::Words,
            words: 1,
            ..TestSpec::default()
        },
        "🇺🇸",
    )
    .unwrap();
    engine.apply(Action::Text("🇺"), ORIGIN);
    assert_eq!(engine.state(), State::Running);
    engine.apply(Action::Text("🇸🇨"), ORIGIN + 20_000);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.outcome(), Outcome::Complete);
    assert_eq!(output(&engine), "🇺🇸");
    assert_eq!(engine.counts().attempts_total, 1);
    assert_eq!(engine.counts().attempts_correct, 1);
}

fn output(engine: &Engine) -> String {
    let mut text = String::new();
    for token in engine.tokens() {
        for entry in &token.entered {
            text.push_str(entry.unit.as_str());
        }
        if token.separator {
            text.push(' ');
        }
    }
    text
}

fn near(actual: Option<f64>, expected: f64) {
    let actual = actual.expect("metric should be defined");
    assert!((actual - expected).abs() < 1.0e-9, "{actual} != {expected}");
}

#[test]
fn published_scoring_formula_fixture() {
    let counts = Counts {
        attempts_total: 5,
        attempts_correct: 4,
        deletion_count: 1,
        retained_units: 4,
        credited_units: 4,
        final_correct: 4,
        ..Counts::default()
    };
    let metrics = Metrics::calculate(counts, 2_000_000, false, false);
    near(metrics.wpm, 24.0);
    near(metrics.raw_wpm, 24.0);
    near(metrics.accuracy, 80.0);
    near(metrics.cpm, 120.0);
}

#[test]
fn whole_token_wpm_is_not_raw_speed_times_accuracy() {
    let metrics = Metrics::calculate(
        Counts {
            attempts_total: 4,
            attempts_correct: 3,
            retained_units: 4,
            credited_units: 0,
            ..Counts::default()
        },
        2_000_000,
        false,
        false,
    );
    near(metrics.wpm, 0.0);
    near(metrics.raw_wpm, 24.0);
    near(metrics.accuracy, 75.0);
}

#[test]
fn zero_time_and_early_live_speeds_are_unavailable() {
    let counts = Counts {
        attempts_total: 1,
        attempts_correct: 1,
        retained_units: 1,
        credited_units: 1,
        ..Counts::default()
    };
    let zero = Metrics::calculate(counts, 0, false, false);
    assert_eq!(zero.wpm, None);
    assert_eq!(zero.raw_wpm, None);
    assert_eq!(zero.cpm, None);
    let live = Metrics::calculate(counts, 999_999, false, true);
    assert_eq!(live.wpm, None);
    assert_eq!(live.raw_wpm, None);
    near(Metrics::calculate(counts, 500_000, false, false).wpm, 24.0);
    near(Metrics::calculate(counts, 1_000_000, false, true).wpm, 12.0);
    assert_eq!(
        Metrics::calculate(Counts::default(), 0, false, false).accuracy,
        None
    );
}

#[test]
fn zen_reports_only_output_speed() {
    let metrics = Metrics::calculate(
        Counts {
            attempts_total: 10,
            retained_units: 8,
            ..Counts::default()
        },
        4_000_000,
        true,
        false,
    );
    assert_eq!(metrics.wpm, None);
    assert_eq!(metrics.cpm, None);
    assert_eq!(metrics.accuracy, None);
    near(metrics.raw_wpm, 24.0);
}

#[test]
fn contradictory_correction_rules_are_rejected() {
    let impossible = Rules {
        backspace: Backspace::None,
        stop_on_error: StopOnError::Word,
        ..Rules::default()
    };
    assert!(impossible.validate().is_err());
    let conflicting = Rules {
        difficulty: Difficulty::Expert,
        stop_on_error: StopOnError::Word,
        ..Rules::default()
    };
    assert!(conflicting.validate().is_err());
    for difficulty in [Difficulty::Normal, Difficulty::Expert, Difficulty::Master] {
        for backspace in [
            Backspace::Mistakes,
            Backspace::Current,
            Backspace::Full,
            Backspace::None,
        ] {
            assert!(
                Rules {
                    difficulty,
                    backspace,
                    ..Rules::default()
                }
                .validate()
                .is_ok()
            );
        }
    }
}

#[test]
fn challenge_thresholds_must_be_finite_and_meaningful() {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 100.1] {
        assert!(
            Rules {
                minimum_accuracy: Some(invalid),
                ..Rules::default()
            }
            .validate()
            .is_err()
        );
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
        assert!(
            Rules {
                minimum_wpm: Some(invalid),
                ..Rules::default()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        Rules {
            minimum_accuracy: Some(100.0),
            minimum_wpm: Some(50.0),
            ..Rules::default()
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn profile_ignores_random_seed_and_inactive_limits() {
    let original = TestSpec::default();
    let mut changed = original.clone();
    changed.seed = u64::MAX;
    changed.words = 9_999;
    assert_eq!(original.profile_key(), changed.profile_key());

    let words = TestSpec {
        mode: Mode::Words,
        ..original
    };
    let mut changed = words.clone();
    changed.seconds = 3_599;
    changed.seed = 876;
    assert_eq!(words.profile_key(), changed.profile_key());
}

#[test]
fn effective_rules_and_content_versions_split_record_categories() {
    let baseline = TestSpec::default();
    let mut variants = Vec::new();
    let mut variant = baseline.clone();
    variant.seconds += 1;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.source_id = "english_1000".into();
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.source_revision = "2".into();
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.generator_version += 1;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.scoring_version += 1;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.punctuation = true;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.numbers = true;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.rules.difficulty = Difficulty::Expert;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.rules.backspace = Backspace::Full;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.rules.stop_on_error = StopOnError::Letter;
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.rules.minimum_wpm = Some(30.0);
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.rules.minimum_accuracy = Some(95.0);
    variants.push(variant);
    let mut variant = baseline.clone();
    variant.pace_wpm = Some(50.0);
    variants.push(variant);
    for variant in variants {
        assert_ne!(baseline.profile_key(), variant.profile_key(), "{variant:?}");
    }
}

#[test]
fn fixed_passage_identity_and_exact_completion_are_effective_parameters() {
    let original = TestSpec {
        mode: Mode::Custom,
        policy: Policy::Exact,
        normalize: false,
        content_hash: "first passage hash".into(),
        ..TestSpec::default()
    };
    let mut changed = original.clone();
    changed.content_hash = "second passage hash".into();
    assert_ne!(original.profile_key(), changed.profile_key());
    changed = original.clone();
    changed.completion = Completion::Auto;
    assert_ne!(original.profile_key(), changed.profile_key());
    changed = original.clone();
    changed.auto_indent = true;
    assert_ne!(original.profile_key(), changed.profile_key());
    changed = original.clone();
    changed.normalize = true;
    assert_ne!(original.profile_key(), changed.profile_key());
}

#[test]
fn prose_normalization_and_blind_styling_do_not_split_effectively_equal_profiles() {
    let original = TestSpec::default();
    let mut changed = original.clone();
    changed.normalize = false;
    changed.rules.blind = true;
    assert_eq!(original.profile_key(), changed.profile_key());
}

#[test]
fn an_empty_run_has_no_valid_speed_even_with_positive_elapsed_time() {
    let metrics = Metrics::calculate(Counts::default(), 2_000_000, false, false);
    assert_eq!(metrics.wpm, None);
    assert_eq!(metrics.raw_wpm, None);
    assert_eq!(metrics.accuracy, None);
}

#[test]
fn first_wrong_character_starts_at_receipt_and_appears_once() {
    let mut engine = timed("cat dog");
    engine.apply(Action::Text(" "), ORIGIN - 3);
    engine.apply(Action::Backspace, ORIGIN - 2);
    engine.apply(Action::Tick, ORIGIN - 1);
    assert_eq!(engine.state(), State::Ready);
    assert_eq!(engine.started_at(), None);
    assert_eq!(engine.counts(), Counts::default());
    engine.apply(Action::Text("x"), ORIGIN);
    assert_eq!(engine.state(), State::Running);
    assert_eq!(engine.started_at(), Some(ORIGIN));
    assert_eq!(output(&engine), "x");
    assert_eq!(engine.counts().attempts_total, 1);
    assert_eq!(engine.counts().attempts_correct, 0);
    assert_eq!(engine.counts().retained_units, 1);
}

#[test]
fn corrected_cat_matches_the_complete_published_golden_vector() {
    let mut engine = timed("cat dog");
    enter(&mut engine, "cax", ORIGIN);
    engine.apply(Action::Backspace, ORIGIN + 30_000);
    enter(&mut engine, "t ", ORIGIN + 40_000);
    engine.apply(Action::Tick, ORIGIN + 2_000_000);
    assert_eq!(
        engine.counts(),
        Counts {
            attempts_total: 5,
            attempts_correct: 4,
            deletion_count: 1,
            retained_units: 4,
            credited_units: 4,
            final_correct: 4,
            ..Counts::default()
        }
    );
    near(engine.metrics().accuracy, 80.0);
    near(engine.metrics().wpm, 24.0);
    near(engine.metrics().raw_wpm, 24.0);
    assert_eq!(engine.missed_words(), vec!["cat"]);
}

#[test]
fn early_space_creates_missed_units_without_inventing_typed_output() {
    let mut engine = timed("cat dog eel");
    enter(&mut engine, "c ", ORIGIN);
    assert_eq!(engine.counts().retained_units, 2);
    assert_eq!(engine.counts().credited_units, 0);
    assert_eq!(engine.counts().attempts_total, 2);
    assert_eq!(engine.counts().attempts_correct, 1);
    assert_eq!(engine.counts().final_missed, 2);
    assert_eq!(engine.current_token(), 1);
    engine.apply(Action::Text("   "), ORIGIN + 50_000);
    assert_eq!(engine.current_token(), 1);
    assert_eq!(engine.counts().attempts_total, 2);
    assert_eq!(output(&engine), "c ");
}

#[test]
fn separator_does_not_penalize_an_existing_letter_error_twice() {
    let mut engine = timed("cat dog");
    enter(&mut engine, "cax ", ORIGIN);
    assert_eq!(engine.counts().attempts_total, 4);
    assert_eq!(engine.counts().attempts_correct, 3);
    assert_eq!(engine.counts().retained_units, 4);
    assert_eq!(engine.counts().credited_units, 0);
    assert_eq!(engine.counts().final_incorrect, 1);
    assert_eq!(engine.counts().final_missed, 0);
}

#[test]
fn extra_letters_remain_extra_and_invalidate_token_credit() {
    let mut engine = timed("cat dog");
    enter(&mut engine, "cats ", ORIGIN);
    assert_eq!(engine.counts().attempts_total, 5);
    assert_eq!(engine.counts().attempts_correct, 4);
    assert_eq!(engine.counts().retained_units, 5);
    assert_eq!(engine.counts().credited_units, 0);
    assert_eq!(engine.counts().final_extra, 1);
    assert_eq!(output(&engine), "cats ");
}

#[test]
fn active_prefix_credit_has_no_phantom_suffix_or_separator() {
    for (typed, expected_credit) in [("ca", 2), ("cx", 0)] {
        let mut engine = timed("cat dog");
        enter(&mut engine, typed, ORIGIN);
        engine.apply(Action::Tick, ORIGIN + 2_000_000);
        assert_eq!(engine.counts().retained_units, 2);
        assert_eq!(engine.counts().credited_units, expected_credit);
        near(engine.metrics().raw_wpm, 12.0);
        engine.apply(Action::Tick, ORIGIN + 30_000_000);
        assert_eq!(engine.counts().credited_units, expected_credit);
        assert_eq!(engine.counts().retained_units, 2);
    }
}

#[test]
fn deadline_is_exclusive_and_late_finalization_keeps_configured_duration() {
    let mut engine = timed("cat dog");
    engine.apply(Action::Text("c"), ORIGIN);
    engine.apply(Action::Text("a"), ORIGIN + 29_999_000);
    assert_eq!(engine.counts().retained_units, 2);
    engine.apply(Action::Text("t"), ORIGIN + 30_000_000);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.counts().retained_units, 2);
    assert_eq!(engine.elapsed_us(), 30_000_000);
    assert_eq!(output(&engine), "ca");
    near(engine.metrics().wpm, 0.8);

    let mut delayed = timed("cat dog");
    delayed.apply(Action::Text("c"), ORIGIN);
    delayed.apply(Action::Tick, ORIGIN + 90_000_000);
    assert_eq!(delayed.elapsed_us(), 30_000_000);
    assert_eq!(delayed.counts().retained_units, 1);
    assert_eq!(delayed.samples().len(), 30);
}

#[test]
fn correct_final_word_completes_without_trailing_space() {
    let mut engine = Engine::new(
        TestSpec {
            mode: Mode::Words,
            words: 1,
            ..TestSpec::default()
        },
        "cat",
    )
    .unwrap();
    enter(&mut engine, "cat", ORIGIN);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.outcome(), Outcome::Complete);
    assert_eq!(engine.counts().credited_units, 3);
    assert_eq!(engine.counts().retained_units, 3);
    assert_eq!(engine.counts().attempts_total, 3);
}

#[test]
fn wrong_final_word_is_repairable_and_normal_submission_is_allowed() {
    for repair in [false, true] {
        let mut engine = Engine::new(
            TestSpec {
                mode: Mode::Words,
                words: 1,
                ..TestSpec::default()
            },
            "cat",
        )
        .unwrap();
        enter(&mut engine, "cax", ORIGIN);
        assert_eq!(engine.state(), State::Running);
        if repair {
            engine.apply(Action::Backspace, ORIGIN + 40_000);
            engine.apply(Action::Text("t"), ORIGIN + 50_000);
            assert_eq!(engine.counts().credited_units, 3);
        } else {
            engine.apply(Action::Text(" "), ORIGIN + 50_000);
            assert_eq!(engine.counts().credited_units, 0);
        }
        assert_eq!(engine.state(), State::Results);
        assert_eq!(engine.outcome(), Outcome::Complete);
    }
}

#[test]
fn full_backtracking_reverses_each_contribution_once() {
    let mut engine = with_rules(
        "cat dog eel",
        Rules {
            backspace: Backspace::Full,
            ..Rules::default()
        },
    );
    enter(&mut engine, "cat dog ", ORIGIN);
    assert_eq!(engine.counts().credited_units, 8);
    engine.apply(Action::DeleteWord, ORIGIN + 200_000);
    assert_eq!(output(&engine), "cat ");
    assert_eq!(engine.counts().credited_units, 4);
    assert_eq!(engine.counts().retained_units, 4);
    engine.apply(Action::Backspace, ORIGIN + 300_000);
    assert_eq!(output(&engine), "cat");
    assert_eq!(engine.counts().credited_units, 3);
    engine.apply(Action::DeleteWord, ORIGIN + 400_000);
    assert_eq!(output(&engine), "");
    assert_eq!(engine.counts().credited_units, 0);
    assert_eq!(engine.counts().retained_units, 0);
    assert_eq!(engine.counts().attempts_total, 8);
    let before = engine.counts();
    engine.apply(Action::Backspace, ORIGIN + 500_000);
    assert_eq!(engine.counts(), before);
}

#[test]
fn mistakes_backtracking_reopens_errors_but_stops_at_correct_tokens() {
    let mut wrong = timed("cat dog");
    enter(&mut wrong, "cax ", ORIGIN);
    wrong.apply(Action::Backspace, ORIGIN + 50_000);
    assert_eq!(wrong.current_token(), 0);
    assert_eq!(output(&wrong), "cax");
    wrong.apply(Action::Backspace, ORIGIN + 60_000);
    assert_eq!(wrong.counts().credited_units, 2);
    enter(&mut wrong, "t ", ORIGIN + 70_000);
    assert_eq!(wrong.counts().credited_units, 4);
    assert_eq!(wrong.counts().attempts_total, 6);
    assert_eq!(wrong.counts().attempts_correct, 5);

    let mut correct = timed("cat dog");
    enter(&mut correct, "cat ", ORIGIN);
    let before = correct.counts();
    correct.apply(Action::Backspace, ORIGIN + 50_000);
    correct.apply(Action::DeleteWord, ORIGIN + 60_000);
    assert_eq!(correct.counts(), before);
    assert_eq!(correct.current_token(), 1);
}

#[test]
fn reopening_an_omitted_suffix_retracts_missed_count() {
    let mut engine = timed("cat dog");
    enter(&mut engine, "c ", ORIGIN);
    assert_eq!(engine.counts().final_missed, 2);
    engine.apply(Action::Backspace, ORIGIN + 20_000);
    assert_eq!(engine.counts().final_missed, 0);
    assert_eq!(engine.counts().credited_units, 1);
    enter(&mut engine, "at ", ORIGIN + 30_000);
    assert_eq!(engine.counts().final_missed, 0);
    assert_eq!(engine.counts().credited_units, 4);
    assert_eq!(engine.counts().attempts_total, 5);
    assert_eq!(engine.counts().attempts_correct, 4);
}

#[test]
fn current_and_none_correction_policies_obey_their_boundaries() {
    let mut current = with_rules(
        "cat dog",
        Rules {
            backspace: Backspace::Current,
            ..Rules::default()
        },
    );
    enter(&mut current, "cax ", ORIGIN);
    let before = current.counts();
    current.apply(Action::Backspace, ORIGIN + 100_000);
    current.apply(Action::DeleteWord, ORIGIN + 110_000);
    assert_eq!(current.counts(), before);
    enter(&mut current, "dx", ORIGIN + 120_000);
    current.apply(Action::DeleteWord, ORIGIN + 150_000);
    assert_eq!(output(&current), "cax ");

    let mut none = with_rules(
        "cat dog",
        Rules {
            backspace: Backspace::None,
            ..Rules::default()
        },
    );
    enter(&mut none, "cax", ORIGIN);
    let before = none.counts();
    none.apply(Action::Backspace, ORIGIN + 100_000);
    none.apply(Action::DeleteWord, ORIGIN + 110_000);
    assert_eq!(none.counts(), before);
}

#[test]
fn exact_whitespace_is_literal_and_final_confirmation_allows_repair() {
    let mut engine = exact("\tA\nB");
    engine.apply(Action::Text("\t"), ORIGIN);
    assert_eq!(engine.started_at(), Some(ORIGIN));
    enter(&mut engine, "A\nx", ORIGIN + 10_000);
    assert_eq!(engine.state(), State::Running);
    assert_eq!(engine.counts().retained_units, 4);
    engine.apply(Action::Backspace, ORIGIN + 50_000);
    engine.apply(Action::Text("B"), ORIGIN + 60_000);
    assert_eq!(engine.state(), State::Running);
    engine.apply(Action::Finish, ORIGIN + 1_000_000);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.outcome(), Outcome::Complete);
    assert_eq!(engine.counts().credited_units, 4);
    assert_eq!(engine.counts().attempts_total, 5);
    assert_eq!(engine.counts().attempts_correct, 4);
    assert_eq!(output(&engine), "\tA\nB");
}

#[test]
fn exact_wrong_space_occupies_one_position_and_early_finish_is_incomplete() {
    let mut engine = exact("cat dog");
    enter(&mut engine, "c ", ORIGIN);
    assert_eq!(engine.current_token(), 0);
    assert_eq!(engine.counts().retained_units, 2);
    assert_eq!(output(&engine), "c ");
    engine.apply(Action::Finish, ORIGIN + 1_000_000);
    assert_eq!(engine.outcome(), Outcome::Incomplete);
    assert!(!engine.snapshot().personal_best_eligible);
}

#[test]
fn one_unit_zero_duration_completion_has_no_speed_or_record() {
    let mut engine = Engine::new(
        TestSpec {
            mode: Mode::Words,
            words: 1,
            ..TestSpec::default()
        },
        "a",
    )
    .unwrap();
    engine.apply(Action::Text("a"), ORIGIN);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.elapsed_us(), 0);
    assert_eq!(engine.metrics().wpm, None);
    assert_eq!(engine.metrics().raw_wpm, None);
    assert!(!engine.snapshot().personal_best_eligible);
}

#[test]
fn master_and_expert_fail_at_different_observable_boundaries() {
    let mut master = with_rules(
        "cat dog",
        Rules {
            difficulty: Difficulty::Master,
            ..Rules::default()
        },
    );
    master.apply(Action::Text("x"), ORIGIN);
    assert_eq!(master.state(), State::Results);
    assert_eq!(master.outcome(), Outcome::Failed);
    assert_eq!(master.counts().attempts_total, 1);
    assert!(!master.snapshot().personal_best_eligible);

    let mut expert = with_rules(
        "cat dog",
        Rules {
            difficulty: Difficulty::Expert,
            ..Rules::default()
        },
    );
    enter(&mut expert, "cax", ORIGIN);
    assert_eq!(expert.state(), State::Running);
    expert.apply(Action::Text(" "), ORIGIN + 50_000);
    assert_eq!(expert.state(), State::Results);
    assert_eq!(expert.outcome(), Outcome::Failed);
    assert!(expert.reason().unwrap().contains("expert"));
}

#[test]
fn exact_expert_commits_at_literal_boundary_and_final_confirmation() {
    for (target, text, confirm) in [("cat dog", "cax ", false), ("cat", "cax", true)] {
        let mut engine = Engine::new(
            TestSpec {
                mode: Mode::Code,
                policy: Policy::Exact,
                rules: Rules {
                    difficulty: Difficulty::Expert,
                    ..Rules::default()
                },
                ..TestSpec::default()
            },
            target,
        )
        .unwrap();
        enter(&mut engine, text, ORIGIN);
        if confirm {
            assert_eq!(engine.state(), State::Running);
            engine.apply(Action::Finish, ORIGIN + 100_000);
        }
        assert_eq!(engine.state(), State::Results);
        assert_eq!(engine.outcome(), Outcome::Failed);
    }
}

#[test]
fn stop_letter_records_rejected_attempts_and_stop_word_prevents_commit() {
    let mut letter = with_rules(
        "cat dog",
        Rules {
            stop_on_error: StopOnError::Letter,
            ..Rules::default()
        },
    );
    enter(&mut letter, "xcat ", ORIGIN);
    assert_eq!(output(&letter), "cat ");
    assert_eq!(letter.counts().attempts_total, 5);
    assert_eq!(letter.counts().attempts_correct, 4);
    assert_eq!(letter.counts().credited_units, 4);

    let mut word = with_rules(
        "cat dog",
        Rules {
            stop_on_error: StopOnError::Word,
            ..Rules::default()
        },
    );
    enter(&mut word, "cax ", ORIGIN);
    assert_eq!(word.current_token(), 0);
    assert_eq!(output(&word), "cax");
    assert_eq!(word.counts().retained_units, 3);
    word.apply(Action::Backspace, ORIGIN + 50_000);
    enter(&mut word, "t ", ORIGIN + 60_000);
    assert_eq!(word.current_token(), 1);
    assert_eq!(word.counts().credited_units, 4);
}

#[test]
fn minimum_accuracy_starts_with_twentieth_scored_attempt() {
    let mut engine = with_rules(
        "aaaaaaaaaaaaaaaaaaaaaaaaa next",
        Rules {
            minimum_accuracy: Some(95.0),
            ..Rules::default()
        },
    );
    enter(&mut engine, "xxxxxxxxxxxxxxxxxxx", ORIGIN);
    assert_eq!(engine.counts().attempts_total, 19);
    assert_eq!(engine.state(), State::Running);
    engine.apply(Action::Text("x"), ORIGIN + 200_000);
    assert_eq!(engine.counts().attempts_total, 20);
    assert_eq!(engine.outcome(), Outcome::Failed);
    assert!(engine.reason().unwrap().contains("accuracy"));
}

#[test]
fn minimum_wpm_respects_grace_and_unrounded_boundary_value() {
    let mut engine = with_rules(
        "cat dog",
        Rules {
            minimum_wpm: Some(2.01),
            ..Rules::default()
        },
    );
    engine.apply(Action::Text("c"), ORIGIN);
    engine.apply(Action::Tick, ORIGIN + 5_999_999);
    assert_eq!(engine.state(), State::Running);
    engine.apply(Action::Tick, ORIGIN + 6_000_000);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.outcome(), Outcome::Failed);
    assert_eq!(engine.elapsed_us(), 6_000_000);
    near(engine.metrics().wpm, 2.0);
    assert!(engine.reason().unwrap().contains("WPM"));
}

#[test]
fn decomposed_accent_is_one_revised_prose_attempt_without_false_master_failure() {
    let mut engine = with_rules(
        "é next",
        Rules {
            difficulty: Difficulty::Master,
            ..Rules::default()
        },
    );
    engine.apply(Action::Text("e"), ORIGIN);
    assert_eq!(engine.state(), State::Running);
    engine.apply(Action::Text("\u{301}"), ORIGIN + 10_000);
    assert_eq!(engine.state(), State::Running);
    engine.apply(Action::Text(" "), ORIGIN + 20_000);
    assert_eq!(engine.counts().attempts_total, 2);
    assert_eq!(engine.counts().attempts_correct, 2);
    assert_eq!(engine.counts().retained_units, 2);
    assert_eq!(engine.counts().credited_units, 2);
}

#[test]
fn finalized_incomplete_canonical_prefix_is_wrong_even_when_deleted() {
    let mut engine = with_rules(
        "é next",
        Rules {
            difficulty: Difficulty::Master,
            ..Rules::default()
        },
    );
    engine.apply(Action::Text("e"), ORIGIN);
    assert_eq!(engine.state(), State::Running);
    engine.apply(Action::Backspace, ORIGIN + 10_000);
    assert_eq!(engine.outcome(), Outcome::Failed);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.counts().attempts_total, 1);
    assert_eq!(engine.counts().attempts_correct, 0);
}

#[test]
fn backspace_removes_whole_combining_and_zwj_graphemes() {
    for (target, scalars) in [("é next", "e\u{301}"), ("👩‍💻 next", "👩\u{200d}💻")] {
        let mut engine = timed(target);
        enter(&mut engine, scalars, ORIGIN);
        assert_eq!(engine.counts().attempts_total, 1);
        assert_eq!(engine.counts().attempts_correct, 1);
        assert_eq!(engine.counts().retained_units, 1);
        engine.apply(Action::Backspace, ORIGIN + 100_000);
        assert_eq!(engine.counts().retained_units, 0);
        assert_eq!(engine.counts().attempts_total, 1);
        assert_eq!(engine.counts().deletion_count, 1);
        assert_eq!(output(&engine), "");
    }
}

#[test]
fn exact_policy_preserves_code_points_and_wide_units_are_not_two_attempts() {
    let mut exact = exact("é");
    enter(&mut exact, "e\u{301}", ORIGIN);
    exact.apply(Action::Finish, ORIGIN + 100_000);
    assert_eq!(exact.counts().attempts_total, 1);
    assert_eq!(exact.counts().attempts_correct, 0);
    assert_eq!(exact.counts().credited_units, 0);

    let mut wide = timed("界 next");
    wide.apply(Action::Text("界"), ORIGIN);
    assert_eq!(wide.counts().retained_units, 1);
    assert_eq!(wide.counts().credited_units, 1);
    assert_eq!(wide.tokens()[0].target[0].width(), 2);
}

#[test]
fn paste_is_atomic_and_active_attempt_disqualifies_a_record() {
    let mut engine = timed("cat dog");
    engine.apply(Action::Paste, ORIGIN - 1);
    assert_eq!(engine.state(), State::Ready);
    assert_eq!(engine.counts(), Counts::default());
    engine.apply(Action::Text("c"), ORIGIN);
    let before = engine.counts();
    engine.apply(Action::Paste, ORIGIN + 10_000);
    assert_eq!(engine.counts(), before);
    engine.apply(Action::Tick, ORIGIN + 30_000_000);
    assert!(engine.snapshot().integrity.paste_attempted);
    assert!(!engine.snapshot().personal_best_eligible);
}

#[test]
fn idle_samples_and_fractional_tail_do_not_depend_on_render_frequency() {
    let mut engine = timed("cat dog");
    engine.apply(Action::Text("c"), ORIGIN);
    engine.apply(Action::Finish, ORIGIN + 5_500_000);
    assert_eq!(engine.samples().len(), 6);
    let samples: Vec<_> = engine.samples().iter().collect();
    assert_eq!(samples[0].attempts, 1);
    for sample in &samples[1..5] {
        assert_eq!(sample.attempts, 0);
        assert_eq!(sample.duration_us, 1_000_000);
    }
    assert_eq!(samples[5].duration_us, 500_000);
    near(engine.metrics().consistency, 100.0 / 3.0);
}

#[test]
fn consistency_requires_five_full_buckets_and_a_nonzero_attempt_mean() {
    let mut short = timed("cat dog");
    short.apply(Action::Text("c"), ORIGIN);
    short.apply(Action::Finish, ORIGIN + 4_900_000);
    assert_eq!(short.samples().len(), 5);
    assert_eq!(short.metrics().consistency, None);

    // The incomplete Unicode prefix has not committed any text-unit attempt.
    let mut zero_mean = timed("é next");
    zero_mean.apply(Action::Text("e"), ORIGIN);
    zero_mean.apply(Action::Tick, ORIGIN + 5_000_000);
    assert_eq!(zero_mean.samples().len(), 5);
    assert_eq!(zero_mean.counts().attempts_total, 0);
    assert_eq!(zero_mean.metrics().consistency, None);
}

#[test]
fn consistency_uses_population_rates_and_excludes_a_busy_fractional_tail() {
    let mut engine = timed("cccccccccccccccccccc next");
    for second in 0..5 {
        engine.apply(Action::Text("c"), ORIGIN + second * 1_000_000);
    }
    engine.apply(Action::Tick, ORIGIN + 5_000_000);
    near(engine.metrics().consistency, 100.0);
    engine.apply(Action::Text("cccccc"), ORIGIN + 5_100_000);
    engine.apply(Action::Finish, ORIGIN + 5_500_000);
    assert_eq!(engine.samples().back().unwrap().duration_us, 500_000);
    assert_eq!(engine.samples().back().unwrap().attempts, 6);
    near(engine.metrics().consistency, 100.0);
}

#[test]
fn interruption_flags_and_repeat_prevent_standard_records() {
    for action in [
        Action::Interrupt("suspend detected"),
        Action::Overload,
        Action::Abort,
    ] {
        let mut engine = timed("cat dog");
        engine.apply(Action::Text("c"), ORIGIN);
        engine.apply(action, ORIGIN + 1_000_000);
        assert_eq!(engine.state(), State::Results);
        assert!(!engine.snapshot().personal_best_eligible);
        assert!(engine.reason().is_some());
    }
    for (repeated, explicit_seed) in [(true, false), (false, true)] {
        let mut engine = Engine::new(
            TestSpec {
                repeated,
                explicit_seed,
                ..TestSpec::default()
            },
            "cat dog",
        )
        .unwrap();
        engine.apply(Action::Text("c"), ORIGIN);
        engine.apply(Action::Tick, ORIGIN + 30_000_000);
        assert!(!engine.snapshot().personal_best_eligible);
    }
}

#[test]
fn zen_cycles_bounded_window_and_keeps_cumulative_output() {
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
    let total = ZEN_WINDOW * 3 + 17;
    for i in 0..total {
        engine.apply(Action::Text("a"), ORIGIN + i as u64 * 1_000);
    }
    assert_eq!(engine.zen_window().len(), ZEN_WINDOW);
    assert_eq!(engine.zen_discarded(), (total - ZEN_WINDOW) as u64);
    assert_eq!(engine.counts().retained_units, total as u64);
    for i in 0..ZEN_WINDOW + 1 {
        engine.apply(Action::Backspace, ORIGIN + total as u64 * 1_000 + i as u64);
    }
    assert!(engine.zen_window().is_empty());
    assert_eq!(engine.counts().retained_units, (total - ZEN_WINDOW) as u64);
    assert_eq!(engine.counts().attempts_total, total as u64);
    engine.apply(
        Action::Finish,
        ORIGIN + total as u64 * 1_000 + ZEN_WINDOW as u64 + 2,
    );
    assert_eq!(engine.metrics().wpm, None);
    assert_eq!(engine.metrics().accuracy, None);
    assert!(engine.metrics().raw_wpm.is_some());
}

#[test]
fn a_maximum_size_grapheme_can_be_followed_by_a_distinct_non_ascii_unit() {
    let cluster = format!("e{}", "\u{301}".repeat(31));
    let target = format!("{cluster}界 next");
    let mut engine = timed(&target);
    enter(&mut engine, &cluster, ORIGIN);
    engine.apply(Action::Text("界"), ORIGIN + 500_000);
    assert_eq!(engine.state(), State::Running);
    assert_eq!(engine.counts().attempts_total, 2);
    assert_eq!(engine.counts().attempts_correct, 2);
    assert_eq!(engine.counts().retained_units, 2);
}

#[test]
fn canonical_mark_reordering_stays_provisional_until_the_cluster_is_complete() {
    let mut prose = with_rules(
        "é\u{327} next",
        Rules {
            difficulty: Difficulty::Master,
            ..Rules::default()
        },
    );
    enter(&mut prose, "e\u{301}", ORIGIN);
    assert_eq!(prose.state(), State::Running);
    prose.apply(Action::Text("\u{327}"), ORIGIN + 20_000);
    assert_eq!(prose.state(), State::Running);
    prose.apply(Action::Text(" "), ORIGIN + 30_000);
    assert_eq!(prose.counts().attempts_total, 2);
    assert_eq!(prose.counts().attempts_correct, 2);

    let mut preserved = exact("e\u{327}\u{301}");
    enter(&mut preserved, "e\u{301}\u{327}", ORIGIN);
    preserved.apply(Action::Finish, ORIGIN + 30_000);
    assert_eq!(preserved.counts().attempts_total, 1);
    assert_eq!(preserved.counts().attempts_correct, 0);
}

#[test]
fn master_rejects_prefixes_that_cannot_be_repaired_by_appending_scalars() {
    for (target, typed) in [
        ("e\u{301}\u{300} next", "e\u{300}"),
        ("👩\u{200d}❤\u{fe0f}\u{200d}👩 next", "👩\u{200d}👩"),
    ] {
        let mut engine = with_rules(
            target,
            Rules {
                difficulty: Difficulty::Master,
                ..Rules::default()
            },
        );
        enter(&mut engine, typed, ORIGIN);
        assert_eq!(
            engine.state(),
            State::Results,
            "unrepairable prefix {typed:?}"
        );
        assert_eq!(engine.outcome(), Outcome::Failed);
        assert_eq!(engine.counts().attempts_total, 1);
        assert_eq!(engine.counts().attempts_correct, 0);
    }
}

#[test]
fn exact_word_stop_also_blocks_wrong_final_confirmation_and_auto_completion() {
    for completion in [Completion::Confirm, Completion::Auto] {
        let mut engine = Engine::new(
            TestSpec {
                mode: Mode::Code,
                policy: Policy::Exact,
                completion,
                rules: Rules {
                    stop_on_error: StopOnError::Word,
                    ..Rules::default()
                },
                ..TestSpec::default()
            },
            "cat",
        )
        .unwrap();
        enter(&mut engine, "cax", ORIGIN);
        assert_eq!(engine.state(), State::Running);
        engine.apply(Action::Finish, ORIGIN + 50_000);
        assert_eq!(engine.state(), State::Running);
    }
}

#[test]
fn final_correct_character_cannot_bypass_minimum_accuracy_failure() {
    let mut engine = Engine::new(
        TestSpec {
            mode: Mode::Words,
            words: 1,
            rules: Rules {
                minimum_accuracy: Some(95.1),
                ..Rules::default()
            },
            ..TestSpec::default()
        },
        "aaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();
    engine.apply(Action::Text("x"), ORIGIN);
    engine.apply(Action::Backspace, ORIGIN + 1);
    enter(&mut engine, "aaaaaaaaaaaaaaaaaaa", ORIGIN + 2);
    assert_eq!(engine.counts().attempts_total, 20);
    assert_eq!(engine.counts().attempts_correct, 19);
    assert_eq!(engine.state(), State::Results);
    assert_eq!(engine.outcome(), Outcome::Failed);
}

#[test]
fn finalizing_pending_cluster_on_deletion_enforces_accuracy() {
    let mut engine = with_rules(
        "aaaaaaaaaaaaaaaaaaaé next",
        Rules {
            minimum_accuracy: Some(99.0),
            ..Rules::default()
        },
    );
    enter(&mut engine, "aaaaaaaaaaaaaaaaaaae", ORIGIN);
    assert_eq!(engine.state(), State::Running);
    engine.apply(Action::Backspace, ORIGIN + 300_000);
    assert_eq!(engine.counts().attempts_total, 20);
    assert_eq!(engine.counts().attempts_correct, 19);
    assert_eq!(engine.outcome(), Outcome::Failed);
    assert_eq!(engine.state(), State::Results);
}

#[test]
fn timed_pending_attempt_is_included_in_final_bucket_and_never_extends_duration() {
    for difficulty in [Difficulty::Normal, Difficulty::Master] {
        let mut engine = with_rules(
            "é next",
            Rules {
                difficulty,
                ..Rules::default()
            },
        );
        engine.apply(Action::Text("e"), ORIGIN);
        engine.apply(Action::Tick, ORIGIN + 31_000_000);
        assert_eq!(engine.elapsed_us(), 30_000_000);
        assert_eq!(engine.counts().attempts_total, 1);
        assert_eq!(engine.counts().attempts_correct, 0);
        assert_eq!(
            engine
                .samples()
                .iter()
                .map(|sample| sample.attempts)
                .sum::<u64>(),
            1
        );
        assert_eq!(
            engine
                .samples()
                .iter()
                .map(|sample| sample.errors)
                .sum::<u64>(),
            1
        );
        if difficulty == Difficulty::Master {
            assert_eq!(engine.outcome(), Outcome::Failed);
        }
    }
}

#[test]
fn assisted_indentation_is_visible_but_never_credited_as_manual_input() {
    let mut engine = Engine::new(
        TestSpec {
            mode: Mode::Code,
            policy: Policy::Exact,
            auto_indent: true,
            ..TestSpec::default()
        },
        "a\n  b",
    )
    .unwrap();
    enter(&mut engine, "a\n", ORIGIN);
    assert_eq!(output(&engine), "a\n  ");
    assert_eq!(engine.counts().retained_units, 2);
    assert_eq!(engine.counts().attempts_total, 2);
    engine.apply(Action::Text("b"), ORIGIN + 100_000);
    engine.apply(Action::Finish, ORIGIN + 200_000);
    assert_eq!(engine.counts().credited_units, 3);
    assert_eq!(engine.counts().retained_units, 3);
    assert_eq!(engine.counts().attempts_total, 3);
    assert!(!engine.snapshot().personal_best_eligible);
}

#[test]
fn slow_practice_requires_eight_eligible_tokens_and_selects_slowest_quartile() {
    let mut engine = timed("a b c d e f g h i next");
    let mut at = ORIGIN;
    for (index, word) in ["a ", "b ", "c ", "d ", "e ", "f ", "g ", "h ", "i "]
        .iter()
        .enumerate()
    {
        at += index as u64 * 100_000;
        enter(&mut engine, word, at);
        if index < 8 {
            assert!(engine.slow_words().is_empty());
        }
    }
    assert_eq!(engine.slow_words(), vec!["i", "h"]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_exact_edits_preserve_positional_counts_and_clock_independence(
        actions in prop::collection::vec(0u8..13, 0..250),
        correction in 0u8..4,
        stop in 0u8..2,
    ) {
        let spec = TestSpec {
            mode: Mode::Code, policy: Policy::Exact, normalize: false,
            rules: Rules {
                backspace: [Backspace::Mistakes, Backspace::Current, Backspace::Full, Backspace::None][correction as usize],
                stop_on_error: [StopOnError::Off, StopOnError::Letter][stop as usize],
                ..Rules::default()
            },
            ..TestSpec::default()
        };
        let mut engine = Engine::new(spec.clone(), "\ta b\né 界\n👩‍💻").unwrap();
        let mut shifted = Engine::new(spec, "\ta b\né 界\n👩‍💻").unwrap();
        for (index, code) in actions.into_iter().enumerate() {
            let action = match code {
                0 => Action::Text("a"), 1 => Action::Text("b"), 2 => Action::Text("x"),
                3 => Action::Text(" "), 4 => Action::Text("\t"), 5 => Action::Text("\n"),
                6 => Action::Text("e"), 7 => Action::Text("\u{301}"), 8 => Action::Text("界"),
                9 => Action::Backspace, 10 => Action::DeleteWord, 11 => Action::Tick, _ => Action::Finish,
            };
            let at = index as u64 * 110_000;
            engine.apply(action, at);
            shifted.apply(action, ORIGIN + at);
            let counts = engine.counts();
            prop_assert!(counts.attempts_correct <= counts.attempts_total);
            prop_assert!(counts.credited_units <= counts.retained_units);
            prop_assert_eq!(counts.final_correct + counts.final_incorrect + counts.final_extra, counts.retained_units);
            prop_assert!(engine.current_token() <= engine.tokens().len());
            prop_assert_eq!(counts, shifted.counts());
            prop_assert_eq!(engine.elapsed_us(), shifted.elapsed_us());
            prop_assert_eq!(engine.state(), shifted.state());
            prop_assert_eq!(engine.outcome(), shifted.outcome());
        }
    }

    #[test]
    fn exact_full_correction_retracts_inserted_output_at_any_boundary(prefix in "[ab \n\t]{0,35}") {
        let mut engine = Engine::new(TestSpec {
            mode: Mode::Code, policy: Policy::Exact,
            rules: Rules { backspace: Backspace::Full, ..Rules::default() },
            ..TestSpec::default()
        }, "a b\na\tb").unwrap();
        enter(&mut engine, &prefix, ORIGIN);
        let before = engine.counts();
        let before_text = output(&engine);
        engine.apply(Action::Text("x"), ORIGIN + 1_000_000);
        engine.apply(Action::Backspace, ORIGIN + 1_000_001);
        let after = engine.counts();
        prop_assert_eq!(after.retained_units, before.retained_units);
        prop_assert_eq!(after.credited_units, before.credited_units);
        prop_assert_eq!(after.final_correct, before.final_correct);
        prop_assert_eq!(after.final_incorrect, before.final_incorrect);
        prop_assert_eq!(after.final_extra, before.final_extra);
        prop_assert_eq!(after.attempts_total, before.attempts_total + 1);
        prop_assert_eq!(output(&engine), before_text);
    }

    #[test]
    fn arbitrary_edits_preserve_bounds_and_replay_is_clock_origin_independent(
        actions in prop::collection::vec(0u8..12, 0..300),
        correction in 0u8..4,
    ) {
        let backspace = [Backspace::Mistakes, Backspace::Current, Backspace::Full, Backspace::None][correction as usize];
        let rules = Rules { backspace, ..Rules::default() };
        let mut first = with_rules("cat dog eel fox yak ant bee cow", rules.clone());
        let mut shifted = with_rules("cat dog eel fox yak ant bee cow", rules);
        for (index, code) in actions.into_iter().enumerate() {
            let action = match code {
                0 => Action::Text("c"), 1 => Action::Text("a"), 2 => Action::Text("t"),
                3 => Action::Text("x"), 4 => Action::Text(" "), 5 => Action::Backspace,
                6 => Action::DeleteWord, 7 => Action::Text("é"), 8 => Action::Text("\u{301}"),
                9 => Action::Text("界"), 10 => Action::Tick, _ => Action::Paste,
            };
            let at = index as u64 * 110_000;
            first.apply(action, at);
            shifted.apply(action, ORIGIN + at);
            let counts = first.counts();
            prop_assert!(counts.attempts_correct <= counts.attempts_total);
            prop_assert!(counts.credited_units <= counts.retained_units);
            prop_assert!(first.current_token() <= first.tokens().len());
            prop_assert_eq!(first.counts(), shifted.counts());
            prop_assert_eq!(first.elapsed_us(), shifted.elapsed_us());
            prop_assert_eq!(first.state(), shifted.state());
            prop_assert_eq!(first.outcome(), shifted.outcome());
            for metric in [first.metrics().wpm, first.metrics().raw_wpm, first.metrics().accuracy].into_iter().flatten() {
                prop_assert!(metric.is_finite());
                prop_assert!(metric >= 0.0);
            }
        }
    }

    #[test]
    fn inserting_and_deleting_ascii_restores_output_but_keeps_the_attempt(character in b'a'..=b'z') {
        let mut engine = with_rules("cat dog", Rules { backspace: Backspace::Full, ..Rules::default() });
        let text = (character as char).to_string();
        engine.apply(Action::Text(&text), ORIGIN);
        engine.apply(Action::Backspace, ORIGIN + 1);
        prop_assert_eq!(engine.counts().retained_units, 0);
        prop_assert_eq!(engine.counts().credited_units, 0);
        prop_assert_eq!(engine.counts().attempts_total, 1);
        prop_assert_eq!(engine.counts().deletion_count, 1);
        prop_assert_eq!(output(&engine), "");
    }

    #[test]
    fn finalized_results_are_immutable_under_residual_input(text in "[ a-z]{0,150}") {
        let mut engine = timed("cat dog");
        engine.apply(Action::Text("c"), ORIGIN);
        engine.apply(Action::Tick, ORIGIN + 30_000_000);
        let snapshot = serde_json::to_value(engine.snapshot()).unwrap();
        engine.apply(Action::Text(&text), ORIGIN + 30_100_000);
        engine.apply(Action::Backspace, ORIGIN + 30_200_000);
        engine.apply(Action::DeleteWord, ORIGIN + 30_300_000);
        engine.apply(Action::Finish, ORIGIN + 30_400_000);
        prop_assert_eq!(serde_json::to_value(engine.snapshot()).unwrap(), snapshot);
    }

    #[test]
    fn metric_domain_is_finite_and_output_is_bounded(
        attempts in 1u64..1_000_000,
        correct_fraction in 0u64..1_000_000,
        retained_fraction in 0u64..1_000_000,
        credit_fraction in 0u64..1_000_000,
        elapsed_us in 0u64..3_600_000_001,
    ) {
        let attempts_correct = correct_fraction % (attempts + 1);
        let retained_units = retained_fraction % (attempts + 1);
        let credited_units = credit_fraction % (retained_units + 1);
        let metrics = Metrics::calculate(Counts {
            attempts_total: attempts, attempts_correct, retained_units, credited_units,
            ..Counts::default()
        }, elapsed_us, false, false);
        for metric in [metrics.wpm, metrics.raw_wpm, metrics.accuracy, metrics.cpm, metrics.consistency].into_iter().flatten() {
            prop_assert!(metric.is_finite());
            prop_assert!(metric >= 0.0);
        }
        prop_assert!(metrics.accuracy.unwrap() <= 100.0);
        if let (Some(wpm), Some(raw)) = (metrics.wpm, metrics.raw_wpm) {
            prop_assert!(wpm <= raw);
        }
    }
}
