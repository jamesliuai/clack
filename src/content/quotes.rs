use std::{collections::HashSet, sync::OnceLock};

use serde::{Deserialize, Serialize};

use super::{ContentError, InputPolicy, PreparedText, SplitMix64, prepare_custom};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteLength {
    Short,
    Medium,
    Long,
    Extended,
}

impl QuoteLength {
    pub fn contains(self, word_count: usize) -> bool {
        match self {
            Self::Short => (1..=30).contains(&word_count),
            Self::Medium => (31..=75).contains(&word_count),
            Self::Long => (76..=150).contains(&word_count),
            Self::Extended => (151..=300).contains(&word_count),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quote {
    pub id: String,
    pub author: String,
    pub work: String,
    pub source: String,
    pub license: String,
    pub revision: String,
    pub content_hash: String,
    pub text: String,
}

impl Quote {
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }
    pub fn prepare(&self) -> Result<PreparedText, ContentError> {
        prepare_custom(self.text.as_bytes(), InputPolicy::Prose, false)
    }
}

#[derive(Debug, Clone)]
pub struct Quotes {
    pub passages: Vec<Quote>,
}

impl Quotes {
    pub fn bundled() -> Result<&'static Self, ContentError> {
        static CATALOG: OnceLock<Result<Quotes, ContentError>> = OnceLock::new();
        CATALOG
            .get_or_init(|| Self::from_json(include_bytes!("../../data/quotes/quotes.json")))
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, ContentError> {
        if bytes.len() > 1024 * 1024 {
            return Err(ContentError::new(
                "quote_catalog_too_large",
                "quote catalog exceeds 1 MiB",
            ));
        }
        let passages: Vec<Quote> = serde_json::from_slice(bytes).map_err(|error| {
            ContentError::new(
                "invalid_quotes",
                format!(
                    "invalid quote catalog (JSON line {}, column {})",
                    error.line(),
                    error.column()
                ),
            )
        })?;
        let mut ids = HashSet::new();
        for quote in &passages {
            if quote.id.is_empty()
                || quote.id.len() > 128
                || !quote
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                || !ids.insert(&quote.id)
            {
                return Err(ContentError::new(
                    "quote_identity",
                    "quote IDs must be unique, nonempty ASCII identifiers of at most 128 bytes",
                ));
            }
            for field in [
                &quote.author,
                &quote.work,
                &quote.source,
                &quote.license,
                &quote.revision,
            ] {
                if field.is_empty()
                    || field.len() > 2048
                    || field.contains(['\n', '\r', '\t'])
                    || super::validate_text(field, InputPolicy::Prose).is_err()
                {
                    return Err(ContentError::new(
                        "quote_attribution",
                        "every quote needs safe, nonempty author, work, source, license and revision metadata",
                    ));
                }
            }
            let prepared = quote.prepare()?;
            if prepared.text != quote.text {
                return Err(ContentError::new(
                    "quote_normalization",
                    "quote catalog text must already use NFC and normalized prose whitespace",
                ));
            }
            if prepared.content_hash != quote.content_hash {
                return Err(ContentError::new(
                    "quote_hash",
                    "quote content hash does not match its normalized text",
                ));
            }
            if !(1..=300).contains(&quote.word_count()) {
                return Err(ContentError::new(
                    "quote_length",
                    "quote length must be between 1 and 300 words",
                ));
            }
        }
        if passages.is_empty() {
            return Err(ContentError::new(
                "empty_quotes",
                "quote catalog contains no passages",
            ));
        }
        Ok(Self { passages })
    }

    pub fn get(&self, id: &str) -> Result<&Quote, ContentError> {
        self.passages
            .iter()
            .find(|quote| quote.id == id)
            .ok_or_else(|| {
                ContentError::new(
                    "unknown_quote",
                    "quote ID was not found in the local catalog",
                )
            })
    }

    /// Explicit ID selection uses `get`; random selection excludes the previous ID
    /// whenever that length category has another passage.
    pub fn select(
        &self,
        seed: u64,
        length: QuoteLength,
        last_id: Option<&str>,
    ) -> Result<&Quote, ContentError> {
        let matching: Vec<_> = self
            .passages
            .iter()
            .filter(|quote| length.contains(quote.word_count()))
            .collect();
        if matching.is_empty() {
            return Err(ContentError::new(
                "no_matching_quote",
                "no local quotation has the requested length",
            ));
        }
        let candidates: Vec<_> = matching
            .iter()
            .copied()
            .filter(|quote| matching.len() == 1 || Some(quote.id.as_str()) != last_id)
            .collect();
        let mut rng = SplitMix64::new(seed);
        Ok(candidates[rng.bounded(candidates.len() as u64) as usize])
    }
}

pub fn code_preset() -> Result<PreparedText, ContentError> {
    prepare_custom(
        include_bytes!("../../data/code-sample.rs.txt"),
        InputPolicy::Exact,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_length_categories_have_attributed_choices_and_never_repeat() {
        let catalog = Quotes::bundled().unwrap();
        for length in [
            QuoteLength::Short,
            QuoteLength::Medium,
            QuoteLength::Long,
            QuoteLength::Extended,
        ] {
            assert!(
                catalog
                    .passages
                    .iter()
                    .filter(|quote| length.contains(quote.word_count()))
                    .count()
                    >= 2
            );
            let first = catalog.select(42, length, None).unwrap();
            let next = catalog.select(42, length, Some(&first.id)).unwrap();
            assert_ne!(first.id, next.id);
            assert_eq!(catalog.get(&first.id).unwrap(), first);
            assert_eq!(catalog.select(42, length, None).unwrap(), first);
        }
    }

    #[test]
    fn boundaries_and_code_are_literal() {
        for (length, min, max) in [
            (QuoteLength::Short, 1, 30),
            (QuoteLength::Medium, 31, 75),
            (QuoteLength::Long, 76, 150),
            (QuoteLength::Extended, 151, 300),
        ] {
            assert!(length.contains(min));
            assert!(length.contains(max));
            assert!(!length.contains(min - 1));
            assert!(!length.contains(max + 1));
        }
        let code = code_preset().unwrap();
        assert_eq!(code.policy, InputPolicy::Exact);
        assert!(code.text.contains('\n'));
        assert!(code.text.contains("    "));
    }
}
