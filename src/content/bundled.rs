//! Bundled data is validated and prepared by build.rs, then shared at runtime.
use std::sync::{Arc, OnceLock};

use super::{ContentError, InputPolicy, LanguagePack, PackMetadata, PackToken};

include!(concat!(env!("OUT_DIR"), "/bundled_packs.rs"));

fn cached_pack(id: &str) -> Result<&'static Arc<LanguagePack>, ContentError> {
    macro_rules! pack {
        ($cache:ident, $prepare:ident) => {{
            static $cache: OnceLock<Arc<LanguagePack>> = OnceLock::new();
            Ok($cache.get_or_init(|| Arc::new($prepare())))
        }};
    }
    match id {
        "english_200" => pack!(ENGLISH_200, prepare_english_200),
        "english_1000" => pack!(ENGLISH_1000, prepare_english_1000),
        "english_10000" => pack!(ENGLISH_10000, prepare_english_10000),
        "french_200" => pack!(FRENCH_200, prepare_french_200),
        "german_200" => pack!(GERMAN_200, prepare_german_200),
        "spanish_200" => pack!(SPANISH_200, prepare_spanish_200),
        _ => Err(ContentError::new(
            "unknown_pack",
            "language pack ID is not installed or bundled",
        )),
    }
}

pub fn bundled_pack(id: &str) -> Result<&'static LanguagePack, ContentError> {
    cached_pack(id).map(Arc::as_ref)
}

pub fn shared_bundled_pack(id: &str) -> Result<Arc<LanguagePack>, ContentError> {
    cached_pack(id).map(Arc::clone)
}
