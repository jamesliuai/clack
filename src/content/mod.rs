//! Validated, prepared content. All parsing and generation happens outside the reducer.
mod case;
mod case_tables;
mod generator;
mod packs;
mod practice;
mod quotes;
mod validation;

pub use case::CASE_MAPPING_VERSION;
pub use generator::{
    GENERATOR_VERSION, GenerationIdentity, Generator, Modifiers, SplitMix64, TIMED_CHUNK_WORDS,
};
pub use packs::{BUNDLED_PACK_IDS, LanguagePack, PackMetadata, PackToken, bundled_pack};
pub use practice::{
    MAX_PRACTICE_BYTES, PRACTICE_WORDS, PracticeKind, PracticeToken, practice_candidates,
    prepare_practice,
};
pub use quotes::{Quote, QuoteLength, Quotes, code_preset};
pub use validation::{
    ContentError, GraphemeSpan, InputPolicy, MAX_CANONICAL_CUSTOM_BYTES, MAX_CUSTOM_BYTES,
    MAX_GRAPHEME_SCALARS, MAX_PACK_BYTES, MAX_PACK_TOKEN_BYTES, MAX_PACK_TOKEN_GRAPHEMES,
    Normalization, PreparedText, TokenSpan, content_hash, prepare_custom, validate_input_scalar,
    validate_text,
};
