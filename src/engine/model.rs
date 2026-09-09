use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCORING_VERSION: u32 = 1;
pub const SAMPLE_CAPACITY: usize = 3601;
pub const ZEN_WINDOW: usize = 4096;
pub const TOKEN_EXTRA_LIMIT: usize = 1024;

macro_rules! choices {
    ($name:ident, $default:ident, $($variant:ident),+) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
        impl Default for $name { fn default() -> Self { Self::$default } }
    };
}
choices!(Mode, Time, Time, Words, Quote, Custom, Code, Zen);
choices!(Policy, Prose, Prose, Exact);
choices!(Completion, Confirm, Confirm, Auto);
choices!(Difficulty, Normal, Normal, Expert, Master);
choices!(Backspace, Mistakes, Mistakes, Current, Full, None);
choices!(StopOnError, Off, Off, Letter, Word);
choices!(State, Ready, Ready, Running, Results);
choices!(
    Outcome,
    Complete,
    Complete,
    Failed,
    Aborted,
    Interrupted,
    Incomplete
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    pub difficulty: Difficulty,
    pub backspace: Backspace,
    pub stop_on_error: StopOnError,
    pub blind: bool,
    pub minimum_wpm: Option<f64>,
    pub minimum_accuracy: Option<f64>,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            difficulty: Difficulty::Normal,
            backspace: Backspace::Mistakes,
            stop_on_error: StopOnError::Off,
            blind: false,
            minimum_wpm: None,
            minimum_accuracy: None,
        }
    }
}
impl Rules {
    pub fn validate(&self) -> Result<(), String> {
        if self.stop_on_error == StopOnError::Word && self.backspace == Backspace::None {
            return Err("rules.stop_on_error=word conflicts with rules.backspace=none".into());
        }
        if self.stop_on_error == StopOnError::Word && self.difficulty == Difficulty::Expert {
            return Err("rules.stop_on_error=word conflicts with rules.difficulty=expert".into());
        }
        for (name, value, max) in [
            ("minimum_wpm", self.minimum_wpm, 1000.0),
            ("minimum_accuracy", self.minimum_accuracy, 100.0),
        ] {
            if let Some(value) = value
                && (!value.is_finite() || value <= 0.0 || value > max)
            {
                return Err(format!(
                    "rules.{name}={value}: expected a finite value in (0, {max}]"
                ));
            }
        }
        Ok(())
    }
}

/// Immutable effective conditions. Cosmetic preferences deliberately live elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneratorParameters {
    pub sentence_min_words: u8,
    pub sentence_max_words: u8,
    pub comma_percent: u8,
    pub number_percent: u8,
    pub number_min_digits: u8,
    pub number_max_digits: u8,
}
impl Default for GeneratorParameters {
    fn default() -> Self {
        Self {
            sentence_min_words: 4,
            sentence_max_words: 12,
            comma_percent: 10,
            number_percent: 10,
            number_min_digits: 1,
            number_max_digits: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TestSpec {
    pub mode: Mode,
    pub seconds: u32,
    pub words: u32,
    pub source_id: String,
    pub source_revision: String,
    pub content_hash: String,
    pub approved_content: bool,
    pub seed: u64,
    pub explicit_seed: bool,
    pub generator_version: u32,
    pub scoring_version: u32,
    pub punctuation: bool,
    pub numbers: bool,
    pub generator_parameters: GeneratorParameters,
    pub policy: Policy,
    pub normalize: bool,
    pub completion: Completion,
    pub rules: Rules,
    pub pace_wpm: Option<f64>,
    pub auto_indent: bool,
    pub repeated: bool,
    pub practice_reason: Option<String>,
}
impl Default for TestSpec {
    fn default() -> Self {
        Self {
            mode: Mode::Time,
            seconds: 30,
            words: 50,
            source_id: "english_200".into(),
            source_revision: "1".into(),
            content_hash: String::new(),
            approved_content: true,
            seed: 0,
            explicit_seed: false,
            generator_version: 1,
            scoring_version: SCORING_VERSION,
            punctuation: false,
            numbers: false,
            generator_parameters: GeneratorParameters::default(),
            policy: Policy::Prose,
            normalize: true,
            completion: Completion::Confirm,
            rules: Rules::default(),
            pace_wpm: None,
            auto_indent: false,
            repeated: false,
            practice_reason: None,
        }
    }
}
impl TestSpec {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=3600).contains(&self.seconds) {
            return Err("test.seconds: expected 1..=3600".into());
        }
        if !(1..=10000).contains(&self.words) {
            return Err("test.words: expected 1..=10000".into());
        }
        if self.scoring_version != SCORING_VERSION {
            return Err("unsupported scoring_version".into());
        }
        let parameters = self.generator_parameters;
        if parameters.sentence_min_words == 0
            || parameters.sentence_min_words > parameters.sentence_max_words
            || parameters.sentence_max_words > 64
            || parameters.comma_percent > 100
            || parameters.number_percent > 100
            || parameters.number_min_digits == 0
            || parameters.number_min_digits > parameters.number_max_digits
            || parameters.number_max_digits > 4
        {
            return Err(
                "test.generator_parameters: invalid sentence, probability, or digit bounds".into(),
            );
        }
        if self.auto_indent && self.policy != Policy::Exact {
            return Err("practice.auto_indent requires exact policy".into());
        }
        if self.mode == Mode::Code && self.policy != Policy::Exact {
            return Err("code requires exact policy".into());
        }
        if matches!(self.mode, Mode::Time | Mode::Words | Mode::Quote)
            && self.policy != Policy::Prose
        {
            return Err("time, words, and quote modes require prose policy; use custom or code for exact text".into());
        }
        if let Some(pace) = self.pace_wpm
            && (!pace.is_finite() || !(1.0..=1000.0).contains(&pace))
        {
            return Err("practice.pace: expected 1..=1000 WPM".into());
        }
        self.rules.validate()
    }
    pub fn profile_key(&self) -> String {
        // Struct field order is fixed; seeds and inactive limits are deliberately absent.
        #[derive(Serialize)]
        struct Profile<'a> {
            mode: Mode,
            limit: Option<u32>,
            source: &'a str,
            revision: &'a str,
            passage: Option<&'a str>,
            generator: u32,
            scoring: u32,
            punctuation: bool,
            numbers: bool,
            generator_parameters: Option<GeneratorParameters>,
            policy: Policy,
            normalize: bool,
            completion: Option<Completion>,
            rules: &'a Rules,
            pace: Option<f64>,
            auto_indent: bool,
        }
        let random = matches!(self.mode, Mode::Time | Mode::Words);
        let zen = self.mode == Mode::Zen;
        let mut scoring_rules = if zen {
            Rules {
                backspace: if self.rules.backspace == Backspace::Mistakes {
                    Backspace::Current
                } else {
                    self.rules.backspace
                },
                ..Rules::default()
            }
        } else {
            self.rules.clone()
        };
        scoring_rules.blind = false; // Error visibility is cosmetic; matching and failure are unchanged.
        let mut parameters = self.generator_parameters;
        let defaults = GeneratorParameters::default();
        if !self.punctuation {
            parameters.sentence_min_words = defaults.sentence_min_words;
            parameters.sentence_max_words = defaults.sentence_max_words;
            parameters.comma_percent = defaults.comma_percent;
        }
        if !self.numbers {
            parameters.number_percent = defaults.number_percent;
            parameters.number_min_digits = defaults.number_min_digits;
            parameters.number_max_digits = defaults.number_max_digits;
        }
        let p = Profile {
            mode: self.mode,
            limit: match self.mode {
                Mode::Time => Some(self.seconds),
                Mode::Words => Some(self.words),
                _ => None,
            },
            source: if zen { "zen" } else { &self.source_id },
            revision: if zen { "" } else { &self.source_revision },
            passage: if zen { None } else { Some(&self.content_hash) },
            generator: if random { self.generator_version } else { 0 },
            scoring: self.scoring_version,
            punctuation: random && self.punctuation,
            numbers: random && self.numbers,
            generator_parameters: random.then_some(parameters),
            policy: if zen { Policy::Prose } else { self.policy },
            normalize: !zen && (self.policy == Policy::Prose || self.normalize),
            completion: (!zen && self.policy == Policy::Exact).then_some(self.completion),
            rules: &scoring_rules,
            pace: if zen { None } else { self.pace_wpm },
            auto_indent: !zen && self.auto_indent,
        };
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&p).expect("validated serializable profile"))
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub attempts_total: u64,
    pub attempts_correct: u64,
    pub deletion_count: u64,
    pub retained_units: u64,
    pub credited_units: u64,
    pub final_correct: u64,
    pub final_incorrect: u64,
    pub final_extra: u64,
    pub final_missed: u64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub wpm: Option<f64>,
    pub raw_wpm: Option<f64>,
    pub accuracy: Option<f64>,
    pub cpm: Option<f64>,
    pub consistency: Option<f64>,
}
impl Metrics {
    pub fn calculate(c: Counts, elapsed_us: u64, zen: bool, live: bool) -> Self {
        let time_ok = c.attempts_total > 0 && elapsed_us > 0 && (!live || elapsed_us >= 1_000_000);
        let speed = |units| time_ok.then(|| 12_000_000.0 * units as f64 / elapsed_us as f64);
        let wpm = if zen { None } else { speed(c.credited_units) };
        Self {
            wpm,
            raw_wpm: speed(c.retained_units),
            accuracy: if !zen && c.attempts_total > 0 {
                Some(100.0 * c.attempts_correct as f64 / c.attempts_total as f64)
            } else {
                None
            },
            cpm: wpm.map(|w| w * 5.0),
            consistency: None,
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sample {
    pub bucket_index: u64,
    pub duration_us: u64,
    pub counts: Counts,
    pub attempts: u64,
    pub errors: u64,
    pub metrics: Metrics,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Integrity {
    pub paste_attempted: bool,
    pub focus_lost: bool,
    pub input_overload: bool,
    pub clock_interrupted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultSnapshot {
    pub export_version: u32,
    pub app_version: String,
    pub spec: TestSpec,
    pub profile_key: String,
    pub outcome: Outcome,
    pub reason: Option<String>,
    pub elapsed_us: u64,
    pub counts: Counts,
    pub metrics: Metrics,
    pub integrity: Integrity,
    pub samples: Vec<Sample>,
    pub words: Vec<WordSummary>,
    pub personal_best_eligible: bool,
}
impl ResultSnapshot {
    /// Shared by the reducer and durable storage boundary. Consumers may revoke
    /// eligibility after an epoch-integrity verdict, but cannot relax these rules.
    pub fn eligible_for_standard_best(&self) -> bool {
        self.outcome == Outcome::Complete
            && self.elapsed_us > 0
            && self.counts.attempts_total > 0
            && self.spec.mode != Mode::Zen
            && !self.spec.explicit_seed
            && !self.spec.repeated
            && self.spec.practice_reason.is_none()
            && !self.spec.auto_indent
            && self.spec.pace_wpm.is_none()
            && !self.integrity.paste_attempted
            && !self.integrity.input_overload
            && !self.integrity.clock_interrupted
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WordSummary {
    pub token: String,
    pub entered: String,
    pub attempts: u64,
    pub errors: u64,
    pub elapsed_us: u64,
    pub expected_units: u64,
    pub corrected: bool,
    pub completed_correct: bool,
}
