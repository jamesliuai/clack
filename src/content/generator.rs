use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{ContentError, InputPolicy, LanguagePack, Normalization, PreparedText};

/// `clack-splitmix64-v1`: see docs/content.md for every draw and transformation.
pub const GENERATOR_VERSION: u32 = 1;
pub const TIMED_CHUNK_WORDS: usize = 256;

/// Non-cryptographic SplitMix64. Integer operations and seed mapping are portable.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let z = (self.state ^ (self.state >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Modulo reduction after rejecting the short prefix of the u64 range.
    /// The accepted range has a size divisible by `bound`; no float/usize RNG.
    pub fn bounded(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "a random range must be nonempty");
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u64();
            if value >= threshold {
                return value % bound;
            }
        }
    }

    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        for index in (1..slice.len()).rev() {
            let other = self.bounded((index + 1) as u64) as usize;
            slice.swap(index, other);
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modifiers {
    pub punctuation: bool,
    pub numbers: bool,
    pub sentence_min_words: u8,
    pub sentence_max_words: u8,
    pub comma_percent: u8,
    pub number_percent: u8,
    pub number_min_digits: u8,
    pub number_max_digits: u8,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self {
            punctuation: false,
            numbers: false,
            sentence_min_words: 4,
            sentence_max_words: 12,
            comma_percent: 10,
            number_percent: 10,
            number_min_digits: 1,
            number_max_digits: 4,
        }
    }
}

impl Modifiers {
    pub fn validate(&self) -> Result<(), ContentError> {
        if self.sentence_min_words == 0
            || self.sentence_min_words > self.sentence_max_words
            || self.sentence_max_words > 64
        {
            return Err(ContentError::new(
                "invalid_punctuation",
                "sentence lengths must be ordered within 1–64 words",
            ));
        }
        if self.comma_percent > 100 || self.number_percent > 100 {
            return Err(ContentError::new(
                "invalid_probability",
                "modifier percentages must be between 0 and 100",
            ));
        }
        if self.number_min_digits == 0
            || self.number_min_digits > self.number_max_digits
            || self.number_max_digits > 4
        {
            return Err(ContentError::new(
                "invalid_numbers",
                "number lengths must be ordered within 1–4 digits",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct GenerationIdentity {
    pub generator_version: u32,
    pub prng: String,
    pub seed: u64,
    pub pack_id: String,
    pub pack_revision: String,
    pub pack_content_hash: String,
    pub modifiers: Modifiers,
}

/// State is advanced solely by generated word count, never by event timing or geometry.
#[derive(Clone)]
pub struct Generator {
    pack: Arc<LanguagePack>,
    identity: GenerationIdentity,
    rng: SplitMix64,
    previous_index: Option<usize>,
    sentence_remaining: u8,
    generated_words: u64,
}

impl Generator {
    pub fn new(pack: &LanguagePack, seed: u64, modifiers: Modifiers) -> Result<Self, ContentError> {
        Self::from_shared(Arc::new(pack.clone()), seed, modifiers)
    }

    /// Reuse a prepared pack owned by application composition. Cloning the Arc
    /// never parses, regenerates, or copies the pack's prepared token metadata.
    pub fn from_shared(
        pack: Arc<LanguagePack>,
        seed: u64,
        modifiers: Modifiers,
    ) -> Result<Self, ContentError> {
        modifiers.validate()?;
        if pack.tokens.is_empty() {
            return Err(ContentError::new(
                "empty_pack",
                "cannot generate from an empty language pack",
            ));
        }
        Ok(Self {
            pack: Arc::clone(&pack),
            identity: GenerationIdentity {
                generator_version: GENERATOR_VERSION,
                prng: "splitmix64".into(),
                seed,
                pack_id: pack.metadata.id.clone(),
                pack_revision: pack.metadata.revision.clone(),
                pack_content_hash: pack.metadata.content_hash.clone(),
                modifiers,
            },
            rng: SplitMix64::new(seed),
            previous_index: None,
            sentence_remaining: 0,
            generated_words: 0,
        })
    }

    pub fn identity(&self) -> &GenerationIdentity {
        &self.identity
    }
    pub fn generated_words(&self) -> u64 {
        self.generated_words
    }

    pub fn from_identity(
        pack: &LanguagePack,
        identity: &GenerationIdentity,
    ) -> Result<Self, ContentError> {
        Self::from_shared_identity(Arc::new(pack.clone()), identity)
    }

    pub fn from_shared_identity(
        pack: Arc<LanguagePack>,
        identity: &GenerationIdentity,
    ) -> Result<Self, ContentError> {
        if identity.generator_version != GENERATOR_VERSION || identity.prng != "splitmix64" {
            return Err(ContentError::new(
                "unsupported_generator",
                "this replay needs an unsupported generator or PRNG version",
            ));
        }
        if pack.metadata.id != identity.pack_id
            || pack.metadata.revision != identity.pack_revision
            || pack.metadata.content_hash != identity.pack_content_hash
        {
            return Err(ContentError::new(
                "replay_pack_mismatch",
                "this replay needs the original pack ID, revision and content hash",
            ));
        }
        Self::from_shared(pack, identity.seed, identity.modifiers)
    }

    /// Stream chunks have no artificial sentence endpoint. Join chunks with one Space.
    pub fn next_words(&mut self, count: usize) -> Result<PreparedText, ContentError> {
        if !(1..=10_000).contains(&count) {
            return Err(ContentError::new(
                "invalid_word_count",
                "a content request must contain 1–10,000 words",
            ));
        }
        let mut text = String::with_capacity(count.saturating_mul(8));
        for _ in 0..count {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&self.next_word());
        }
        Ok(PreparedText::from_validated(
            text,
            InputPolicy::Prose,
            Normalization::Nfc,
        ))
    }

    /// Fixed 256-word chunks; the caller prepares a new chunk outside the input reducer
    /// when fewer than two screens remain. Chunk boundaries never reset PRNG/sentences.
    pub fn next_chunk(&mut self) -> Result<PreparedText, ContentError> {
        self.next_words(TIMED_CHUNK_WORDS)
    }

    /// Finite tests end with a period when punctuation is enabled. The last sentence
    /// may be truncated to meet the exact requested count, including counts below four.
    pub fn finite_words(&mut self, count: usize) -> Result<PreparedText, ContentError> {
        let mut target = self.next_words(count)?;
        if self.identity.modifiers.punctuation && !target.text.ends_with('.') {
            if target.text.ends_with(',') {
                target.text.pop();
            }
            target.text.push('.');
            self.sentence_remaining = 0;
            target =
                PreparedText::from_validated(target.text, InputPolicy::Prose, Normalization::Nfc);
        }
        Ok(target)
    }

    fn next_word(&mut self) -> String {
        let size = self.pack.tokens.len();
        let index = match (self.previous_index, size) {
            (Some(previous), 2..) => {
                // Uniform conditional sampling over all indexes except the previous one.
                let draw = self.rng.bounded((size - 1) as u64) as usize;
                if draw >= previous { draw + 1 } else { draw }
            }
            _ => self.rng.bounded(size as u64) as usize,
        };
        self.previous_index = Some(index);
        let parameters = self.identity.modifiers;
        let replaced =
            parameters.numbers && self.rng.bounded(100) < u64::from(parameters.number_percent);
        let mut word = if replaced {
            let digits = u32::from(parameters.number_min_digits)
                + self.rng.bounded(u64::from(
                    parameters.number_max_digits - parameters.number_min_digits + 1,
                )) as u32;
            let low = 10_u64.pow(digits - 1);
            (low + self.rng.bounded(9 * low)).to_string()
        } else {
            self.pack.tokens[index].text.clone()
        };
        // Number replacement intentionally precedes all punctuation draws.
        if parameters.punctuation {
            if self.sentence_remaining == 0 {
                self.sentence_remaining = parameters.sentence_min_words
                    + self.rng.bounded(u64::from(
                        parameters.sentence_max_words - parameters.sentence_min_words + 1,
                    )) as u8;
                if !replaced {
                    word.clone_from(&self.pack.tokens[index].capitalized);
                }
            }
            self.sentence_remaining -= 1;
            if self.sentence_remaining == 0 {
                word.push('.');
            } else if self.rng.bounded(100) < u64::from(parameters.comma_percent) {
                word.push(',');
            }
        }
        self.generated_words = self.generated_words.saturating_add(1);
        word
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::bundled_pack;

    #[test]
    fn independent_portable_golden_vectors() {
        #[derive(Deserialize)]
        struct Golden {
            seed: u64,
            pack_id: String,
            pack_revision: String,
            pack_content_hash: String,
            punctuation: bool,
            numbers: bool,
            word_count: usize,
            text: String,
        }
        #[derive(Deserialize)]
        struct Fixture {
            generator_version: u32,
            cases: Vec<Golden>,
        }
        let fixture: Fixture =
            serde_json::from_slice(include_bytes!("../../data/generator-v1-golden.json")).unwrap();
        assert_eq!(fixture.generator_version, GENERATOR_VERSION);
        for case in fixture.cases {
            let pack = bundled_pack(&case.pack_id).unwrap();
            assert_eq!(pack.metadata.revision, case.pack_revision);
            assert_eq!(pack.metadata.content_hash, case.pack_content_hash);
            let modifiers = Modifiers {
                punctuation: case.punctuation,
                numbers: case.numbers,
                ..Modifiers::default()
            };
            assert_eq!(
                Generator::new(pack, case.seed, modifiers)
                    .unwrap()
                    .finite_words(case.word_count)
                    .unwrap()
                    .text,
                case.text
            );
        }
    }

    #[test]
    fn replay_rejects_a_seed_without_the_original_identity() {
        let pack = bundled_pack("english_200").unwrap();
        let mut original = Generator::new(pack, 42, Modifiers::default()).unwrap();
        let mut replay = Generator::from_identity(pack, original.identity()).unwrap();
        assert_eq!(
            original.next_words(50).unwrap(),
            replay.next_words(50).unwrap()
        );
        let mut wrong = original.identity().clone();
        wrong.generator_version += 1;
        assert!(Generator::from_identity(pack, &wrong).is_err());
        wrong = original.identity().clone();
        wrong.pack_revision.push_str("-changed");
        assert!(Generator::from_identity(pack, &wrong).is_err());
        assert!(
            Generator::from_identity(bundled_pack("english_1000").unwrap(), original.identity())
                .is_err()
        );
    }

    #[test]
    fn shared_pack_outlives_its_application_cache_reference() {
        let shared = Arc::new(bundled_pack("english_200").unwrap().clone());
        let weak = Arc::downgrade(&shared);
        let mut generator =
            Generator::from_shared(Arc::clone(&shared), 42, Modifiers::default()).unwrap();
        assert!(Arc::ptr_eq(&shared, &generator.pack));
        drop(shared);
        assert!(generator.next_chunk().is_ok());
        assert!(weak.upgrade().is_some());
        drop(generator);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn splitmix_reference_vector() {
        let mut rng = SplitMix64::new(0);
        assert_eq!(rng.next_u64(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(rng.next_u64(), 0x6e78_9e6a_a1b9_65f4);
        assert_eq!(rng.next_u64(), 0x06c4_5d18_8009_454f);
    }

    #[test]
    fn stream_is_independent_of_chunk_request_boundaries() {
        let pack = bundled_pack("english_200").unwrap();
        for modifiers in [
            Modifiers::default(),
            Modifiers {
                punctuation: true,
                ..Modifiers::default()
            },
            Modifiers {
                numbers: true,
                ..Modifiers::default()
            },
            Modifiers {
                numbers: true,
                punctuation: true,
                ..Modifiers::default()
            },
        ] {
            let mut one = Generator::new(pack, 42, modifiers).unwrap();
            let mut chunks = Generator::new(pack, 42, modifiers).unwrap();
            let whole = one.next_words(1024).unwrap().text;
            let pieces = (0..4)
                .map(|_| chunks.next_chunk().unwrap().text)
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(whole, pieces);
            assert_eq!(one.generated_words(), chunks.generated_words());
        }
    }

    #[test]
    fn no_adjacent_source_duplicates_and_finite_count() {
        let pack = bundled_pack("english_200").unwrap();
        let target = Generator::new(pack, 99, Modifiers::default())
            .unwrap()
            .finite_words(10_000)
            .unwrap();
        assert_eq!(target.tokens.len(), 10_000);
        assert!(
            target
                .text
                .split(' ')
                .collect::<Vec<_>>()
                .windows(2)
                .all(|pair| pair[0] != pair[1])
        );
        let mut singleton = pack.clone();
        singleton.tokens.truncate(1);
        assert_eq!(
            Generator::new(&singleton, 1, Modifiers::default())
                .unwrap()
                .next_words(2)
                .unwrap()
                .text,
            "the the"
        );
    }

    #[test]
    fn sentence_and_number_rules_are_explicit() {
        let pack = bundled_pack("english_200").unwrap();
        let modifiers = Modifiers {
            punctuation: true,
            numbers: true,
            number_percent: 100,
            ..Modifiers::default()
        };
        let target = Generator::new(pack, 11, modifiers)
            .unwrap()
            .next_words(1000)
            .unwrap();
        let mut sentence_length = 0;
        for word in target.text.split(' ') {
            sentence_length += 1;
            let number = word.trim_end_matches(['.', ',']);
            assert!((1..=4).contains(&number.len()));
            assert!(!number.starts_with('0'));
            assert!(number.bytes().all(|b| b.is_ascii_digit()));
            if word.ends_with('.') {
                assert!((4..=12).contains(&sentence_length));
                sentence_length = 0;
            }
        }
        for count in 1..=25 {
            let target = Generator::new(pack, 11, modifiers)
                .unwrap()
                .finite_words(count)
                .unwrap();
            assert_eq!(target.tokens.len(), count);
            assert!(target.text.ends_with('.'));
        }
    }

    #[test]
    fn uniform_range_sanity_and_invalid_parameters() {
        let mut rng = SplitMix64::new(71);
        let mut counts = [0usize; 7];
        for _ in 0..70_000 {
            counts[rng.bounded(7) as usize] += 1;
        }
        assert!(counts.iter().all(|&count| (9500..=10_500).contains(&count)));
        assert!(
            Modifiers {
                number_percent: 101,
                ..Modifiers::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Modifiers {
                number_min_digits: 0,
                ..Modifiers::default()
            }
            .validate()
            .is_err()
        );
    }
}
