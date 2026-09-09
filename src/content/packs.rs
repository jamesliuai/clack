use std::{collections::HashSet, sync::OnceLock};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{
    ContentError, InputPolicy, MAX_PACK_BYTES, MAX_PACK_TOKEN_BYTES, MAX_PACK_TOKEN_GRAPHEMES,
    content_hash, validate_text,
};

pub const BUNDLED_PACK_IDS: &[&str] = &[
    "english_200",
    "english_1000",
    "english_10000",
    "french_200",
    "german_200",
    "spanish_200",
];
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_PACK_TOKENS: usize = 100_000;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackMetadata {
    pub schema_version: u32,
    pub id: String,
    pub language_tag: String,
    pub revision: String,
    pub source: String,
    pub license: String,
    pub content_hash: String,
    pub text_direction: String,
    pub supported_input_policy: InputPolicy,
    pub token_count: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PackToken {
    pub text: String,
    pub widths: Vec<u16>,
    /// First grapheme uppercasing is prepared once, independently of keypresses.
    pub capitalized: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LanguagePack {
    pub metadata: PackMetadata,
    pub tokens: Vec<PackToken>,
}

impl LanguagePack {
    /// Import is pure: callers read bounded files, then pass the bytes here.
    /// Hashes use NFC tokens in declared order, joined by LF with one final LF.
    pub fn from_parts(words_utf8: &[u8], metadata_json: &[u8]) -> Result<Self, ContentError> {
        if metadata_json.len() > MAX_METADATA_BYTES {
            return Err(ContentError::new(
                "metadata_too_large",
                "pack metadata exceeds 64 KiB",
            ));
        }
        if words_utf8.len() > MAX_PACK_BYTES {
            return Err(ContentError::new(
                "pack_too_large",
                "pack text exceeds 16 MiB",
            ));
        }
        let metadata: PackMetadata = serde_json::from_slice(metadata_json).map_err(|error| {
            ContentError::new(
                "invalid_metadata",
                format!(
                    "pack metadata must match schema 1 (JSON line {}, column {})",
                    error.line(),
                    error.column()
                ),
            )
        })?;
        validate_metadata(&metadata)?;
        let words = std::str::from_utf8(words_utf8).map_err(|error| {
            ContentError::new("invalid_utf8", "pack text is not valid UTF-8")
                .at(error.valid_up_to())
        })?;
        let mut seen = HashSet::new();
        let mut canonical = String::with_capacity(words.len());
        let mut tokens = Vec::with_capacity(metadata.token_count);
        for (line_index, raw) in words.lines().enumerate() {
            let line = line_index + 1;
            if tokens.len() == MAX_PACK_TOKENS {
                return Err(
                    ContentError::new("too_many_tokens", "pack exceeds 100,000 tokens")
                        .on_line(line),
                );
            }
            if raw.is_empty() {
                return Err(
                    ContentError::new("empty_token", "pack contains an empty token").on_line(line),
                );
            }
            if raw.len() > MAX_PACK_TOKEN_BYTES {
                return Err(
                    ContentError::new("token_too_large", "pack token exceeds 8 KiB").on_line(line),
                );
            }
            validate_text(raw, InputPolicy::Prose).map_err(|error| error.on_line(line))?;
            if raw.chars().any(char::is_whitespace) {
                return Err(ContentError::new(
                    "token_whitespace",
                    "pack requires exactly one token per line, with no whitespace inside a token",
                )
                .on_line(line));
            }
            let text: String = raw.nfc().collect();
            validate_text(&text, InputPolicy::Prose).map_err(|error| error.on_line(line))?;
            if text.len() > MAX_PACK_TOKEN_BYTES {
                return Err(ContentError::new(
                    "token_too_large",
                    "normalized pack token exceeds 8 KiB",
                )
                .on_line(line));
            }
            let widths: Vec<_> = text
                .graphemes(true)
                .map(|grapheme| UnicodeWidthStr::width(grapheme) as u16)
                .collect();
            if widths.len() > MAX_PACK_TOKEN_GRAPHEMES {
                return Err(ContentError::new(
                    "token_too_long",
                    "pack token exceeds 128 graphemes",
                )
                .on_line(line));
            }
            if !seen.insert(text.clone()) {
                return Err(ContentError::new(
                    "duplicate_token",
                    "pack contains a duplicate after NFC normalization",
                )
                .on_line(line));
            }
            canonical.push_str(&text);
            canonical.push('\n');
            let first_len = text.graphemes(true).next().map_or(0, str::len);
            let mut capitalized = super::case::uppercase(&text[..first_len]);
            capitalized.push_str(&text[first_len..]);
            let capitalized: String = capitalized.nfc().collect();
            validate_text(&capitalized, InputPolicy::Prose).map_err(|error| error.on_line(line))?;
            tokens.push(PackToken {
                text,
                widths,
                capitalized,
            });
        }
        if tokens.is_empty() {
            return Err(ContentError::new("empty_pack", "pack contains no tokens"));
        }
        if tokens.len() != metadata.token_count {
            return Err(ContentError::new(
                "token_count_mismatch",
                format!(
                    "pack token_count declares {} but validated text contains {} tokens",
                    metadata.token_count,
                    tokens.len()
                ),
            ));
        }
        if content_hash(canonical.as_bytes()) != metadata.content_hash {
            return Err(ContentError::new(
                "hash_mismatch",
                "pack content_hash does not match SHA-256 of normalized tokens with LF separators and a final LF",
            ));
        }
        Ok(Self { metadata, tokens })
    }

    pub fn canonical_words(&self) -> String {
        let mut text = String::new();
        for token in &self.tokens {
            text.push_str(&token.text);
            text.push('\n');
        }
        text
    }
}

fn validate_metadata(metadata: &PackMetadata) -> Result<(), ContentError> {
    if metadata.schema_version != 1 {
        return Err(ContentError::new(
            "metadata_version",
            "pack schema_version must be 1",
        ));
    }
    for (field, value) in [("id", &metadata.id), ("revision", &metadata.revision)] {
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            || value.starts_with('.')
            || value.contains("..")
        {
            return Err(ContentError::new(
                "metadata_identifier",
                format!(
                    "pack {field} must be 1–64 ASCII letters, digits, underscore, hyphen or dot, with no leading dot or '..'"
                ),
            ));
        }
    }
    let mut parts = metadata.language_tag.split('-');
    let primary = parts.next().unwrap_or("");
    if !(2..=8).contains(&primary.len())
        || !primary.bytes().all(|b| b.is_ascii_alphabetic())
        || parts.any(|part| {
            part.is_empty() || part.len() > 8 || !part.bytes().all(|b| b.is_ascii_alphanumeric())
        })
    {
        return Err(ContentError::new(
            "language_tag",
            "pack language_tag must have a 2–8 letter language subtag followed by optional 1–8 alphanumeric subtags",
        ));
    }
    for (field, value) in [("source", &metadata.source), ("license", &metadata.license)] {
        if value.trim().is_empty() || value.len() > 2048 || value.contains(['\r', '\n', '\t']) {
            return Err(ContentError::new(
                "metadata_attribution",
                format!("pack {field} must be nonempty single-line text of at most 2048 bytes"),
            ));
        }
        validate_text(value, InputPolicy::Prose).map_err(|_| {
            ContentError::new(
                "metadata_attribution",
                format!("pack {field} contains unsupported display characters"),
            )
        })?;
    }
    if metadata.text_direction != "ltr" {
        return Err(ContentError::new(
            "unsupported_direction",
            "pack text_direction must be ltr; bidirectional text is not supported in 1.0",
        ));
    }
    if metadata.supported_input_policy != InputPolicy::Prose {
        return Err(ContentError::new(
            "unsupported_policy",
            "random language packs require the prose input policy",
        ));
    }
    if metadata.content_hash.len() != 64
        || !metadata
            .content_hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ContentError::new(
            "invalid_hash",
            "pack content_hash must be a lowercase 64-digit SHA-256 hex digest",
        ));
    }
    if metadata.token_count == 0 || metadata.token_count > MAX_PACK_TOKENS {
        return Err(ContentError::new(
            "invalid_token_count",
            "pack token_count must be between 1 and 100,000",
        ));
    }
    Ok(())
}

pub fn bundled_pack(id: &str) -> Result<&'static LanguagePack, ContentError> {
    macro_rules! pack {
        ($id:literal, $cache:ident) => {{
            static $cache: OnceLock<Result<LanguagePack, ContentError>> = OnceLock::new();
            $cache
                .get_or_init(|| {
                    LanguagePack::from_parts(
                        include_bytes!(concat!("../../data/packs/", $id, "/words.txt")),
                        include_bytes!(concat!("../../data/packs/", $id, "/metadata.json")),
                    )
                })
                .as_ref()
                .map_err(Clone::clone)
        }};
    }
    match id {
        "english_200" => pack!("english_200", ENGLISH_200),
        "english_1000" => pack!("english_1000", ENGLISH_1000),
        "english_10000" => pack!("english_10000", ENGLISH_10000),
        "french_200" => pack!("french_200", FRENCH_200),
        "german_200" => pack!("german_200", GERMAN_200),
        "spanish_200" => pack!("spanish_200", SPANISH_200),
        _ => Err(ContentError::new(
            "unknown_pack",
            "language pack ID is not installed or bundled",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn fixture(words: &str) -> Result<LanguagePack, ContentError> {
        let canonical: String = words
            .lines()
            .map(|line| format!("{}\n", line.nfc().collect::<String>()))
            .collect();
        let metadata = PackMetadata {
            schema_version: 1,
            id: "test".into(),
            language_tag: "en".into(),
            revision: "1".into(),
            source: "Original test fixture".into(),
            license: "CC0-1.0".into(),
            content_hash: content_hash(canonical.as_bytes()),
            text_direction: "ltr".into(),
            supported_input_policy: InputPolicy::Prose,
            token_count: words.lines().count(),
        };
        LanguagePack::from_parts(words.as_bytes(), &serde_json::to_vec(&metadata).unwrap())
    }

    #[test]
    fn all_bundled_packs_match_declared_count_and_hash() {
        for id in BUNDLED_PACK_IDS {
            let pack = bundled_pack(id).unwrap();
            assert_eq!(
                pack.tokens.len(),
                id.rsplit('_').next().unwrap().parse::<usize>().unwrap()
            );
            assert_eq!(
                content_hash(pack.canonical_words().as_bytes()),
                pack.metadata.content_hash
            );
            assert_eq!(pack.metadata.id, *id);
        }
    }

    #[test]
    fn import_normalizes_tokens_and_rejects_ambiguities() {
        assert_eq!(
            fixture("cafe\u{301}\ncat\n").unwrap().tokens[0].text,
            "café"
        );
        assert_eq!(
            fixture("é\ne\u{301}\n").unwrap_err().kind,
            "duplicate_token"
        );
        assert_eq!(fixture("two words\n").unwrap_err().kind, "token_whitespace");
        assert_eq!(fixture("word\n\n").unwrap_err().kind, "empty_token");
        assert_eq!(
            fixture(&format!("{}\n", "a".repeat(129))).unwrap_err().kind,
            "token_too_long"
        );
        assert_eq!(
            fixture(&format!("{}\n", "a".repeat(8193)))
                .unwrap_err()
                .kind,
            "token_too_large"
        );
        assert_eq!(fixture("cat\u{1b}\n").unwrap_err().line, Some(1));
    }

    #[test]
    fn import_checks_hash_direction_policy_and_unknown_fields() {
        let pack = fixture("cat\ndog\n").unwrap();
        let mut metadata = serde_json::to_value(&pack.metadata).unwrap();
        metadata["content_hash"] = "0".repeat(64).into();
        assert_eq!(
            LanguagePack::from_parts(b"cat\ndog\n", &serde_json::to_vec(&metadata).unwrap())
                .unwrap_err()
                .kind,
            "hash_mismatch"
        );
        metadata["text_direction"] = "rtl".into();
        assert_eq!(
            LanguagePack::from_parts(b"cat\ndog\n", &serde_json::to_vec(&metadata).unwrap())
                .unwrap_err()
                .kind,
            "unsupported_direction"
        );
        metadata["private_data"] = "DO NOT ECHO".into();
        let error =
            LanguagePack::from_parts(b"cat\ndog\n", &serde_json::to_vec(&metadata).unwrap())
                .unwrap_err();
        assert_eq!(error.kind, "invalid_metadata");
        assert!(!error.to_string().contains("DO NOT ECHO"));
    }

    #[test]
    fn metadata_rejects_display_injection_and_allocation_bombs() {
        let pack = fixture("cat\ndog\n").unwrap();
        for field in ["source", "license"] {
            for injected in [
                "PRIVATE\u{1b}[31m",
                "PRIVATE\u{009b}31m",
                "PRIVATE\u{202e}",
                "PRIVATE\u{2066}",
            ] {
                let mut metadata = serde_json::to_value(&pack.metadata).unwrap();
                metadata[field] = injected.into();
                let error = LanguagePack::from_parts(
                    b"cat\ndog\n",
                    &serde_json::to_vec(&metadata).unwrap(),
                )
                .unwrap_err();
                assert_eq!(error.kind, "metadata_attribution");
                assert!(!error.to_string().contains("PRIVATE"));
            }
        }
        for count in [100_001usize, usize::MAX] {
            let mut metadata = serde_json::to_value(&pack.metadata).unwrap();
            metadata["token_count"] = count.into();
            let error =
                LanguagePack::from_parts(b"cat\ndog\n", &serde_json::to_vec(&metadata).unwrap())
                    .unwrap_err();
            assert_eq!(error.kind, "invalid_token_count");
        }
    }
}
