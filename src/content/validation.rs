use std::{fmt, ops::Range};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub const MAX_CUSTOM_BYTES: usize = 1024 * 1024;
/// Raw sources remain limited to 1 MiB. Canonical normalization can expand
/// their UTF-8 representation, so prepared targets and storage use a distinct
/// bound. The pinned Unicode 17 tables are exhaustively checked in the content
/// acceptance tests: canonical decomposition expands by at most 3x in bytes,
/// and NFC composition cannot increase that length. 4x leaves a conservative
/// margin without rejecting a source admitted by the raw-byte limit.
pub const MAX_CANONICAL_CUSTOM_BYTES: usize = 4 * MAX_CUSTOM_BYTES;
pub const MAX_PACK_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PACK_TOKEN_BYTES: usize = 8 * 1024;
pub const MAX_PACK_TOKEN_GRAPHEMES: usize = 128;
pub const MAX_GRAPHEME_SCALARS: usize = 32;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputPolicy {
    Prose,
    Exact,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Normalization {
    Nfc,
    Preserve,
}

/// A diagnostic contains positions and limits, never private source text.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContentError {
    pub kind: &'static str,
    pub byte_offset: Option<usize>,
    pub line: Option<usize>,
    pub message: String,
}

impl ContentError {
    pub(crate) fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            byte_offset: None,
            line: None,
            message: message.into(),
        }
    }

    pub(crate) fn at(mut self, offset: usize) -> Self {
        self.byte_offset = Some(offset);
        self
    }

    pub(crate) fn on_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(line) = self.line {
            write!(f, " (line {line})")?;
        }
        if let Some(offset) = self.byte_offset {
            write!(f, " (byte {offset})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ContentError {}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GraphemeSpan {
    pub bytes: Range<usize>,
    /// Tabs/newlines have zero cached width: tab width depends on the display column.
    pub width: u16,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TokenSpan {
    pub bytes: Range<usize>,
    pub units: Range<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PreparedText {
    pub text: String,
    pub graphemes: Vec<GraphemeSpan>,
    /// Non-whitespace target token spans; exact whitespace remains in `graphemes`.
    pub tokens: Vec<TokenSpan>,
    pub content_hash: String,
    pub policy: InputPolicy,
    pub normalization: Normalization,
}

impl PreparedText {
    pub fn unit(&self, index: usize) -> Option<&str> {
        self.graphemes
            .get(index)
            .map(|span| &self.text[span.bytes.clone()])
    }

    pub fn token(&self, index: usize) -> Option<&str> {
        self.tokens
            .get(index)
            .map(|span| &self.text[span.bytes.clone()])
    }

    pub(crate) fn from_validated(
        text: String,
        policy: InputPolicy,
        normalization: Normalization,
    ) -> Self {
        let graphemes: Vec<_> = text
            .grapheme_indices(true)
            .map(|(start, grapheme)| GraphemeSpan {
                bytes: start..start + grapheme.len(),
                width: if grapheme == "\t" || grapheme == "\n" {
                    0
                } else {
                    UnicodeWidthStr::width(grapheme) as u16
                },
            })
            .collect();
        let mut tokens = Vec::new();
        let mut start: Option<usize> = None;
        for (index, span) in graphemes.iter().enumerate() {
            let whitespace = text[span.bytes.clone()].chars().all(char::is_whitespace);
            if whitespace {
                if let Some(first) = start.take() {
                    tokens.push(TokenSpan {
                        bytes: graphemes[first].bytes.start..span.bytes.start,
                        units: first..index,
                    });
                }
            } else if start.is_none() {
                start = Some(index);
            }
        }
        if let Some(first) = start {
            tokens.push(TokenSpan {
                bytes: graphemes[first].bytes.start..text.len(),
                units: first..graphemes.len(),
            });
        }
        Self {
            content_hash: content_hash(text.as_bytes()),
            text,
            graphemes,
            tokens,
            policy,
            normalization,
        }
    }
}

pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Normalize a bounded UTF-8 target before it is admitted to Ready.
pub fn prepare_custom(
    bytes: &[u8],
    policy: InputPolicy,
    normalize_exact: bool,
) -> Result<PreparedText, ContentError> {
    if bytes.len() > MAX_CUSTOM_BYTES {
        return Err(ContentError::new(
            "input_too_large",
            format!("custom input exceeds {MAX_CUSTOM_BYTES} bytes (1 MiB)"),
        ));
    }
    let source = std::str::from_utf8(bytes).map_err(|error| {
        ContentError::new("invalid_utf8", "custom input is not valid UTF-8").at(error.valid_up_to())
    })?;
    validate_text(source, policy)?;
    let lines = source.replace("\r\n", "\n");
    let normalization = if policy == InputPolicy::Prose || normalize_exact {
        Normalization::Nfc
    } else {
        Normalization::Preserve
    };
    let text = match policy {
        InputPolicy::Prose => lines.split_whitespace().collect::<Vec<_>>().join(" "),
        InputPolicy::Exact => lines,
    };
    let text = if normalization == Normalization::Nfc {
        text.nfc().collect()
    } else {
        text
    };
    if text.len() > MAX_CANONICAL_CUSTOM_BYTES {
        return Err(ContentError::new(
            "normalized_input_too_large",
            format!("normalized custom target exceeds {MAX_CANONICAL_CUSTOM_BYTES} bytes"),
        ));
    }
    if text.is_empty() {
        return Err(ContentError::new(
            "empty_target",
            "the normalized target is empty",
        ));
    }
    // Whitespace normalization can expose an orphan combining mark that was
    // previously attached to a leading space, so validate the final target too.
    validate_text(&text, policy)?;
    Ok(PreparedText::from_validated(text, policy, normalization))
}

/// Conservative 1.0 repertoire: Latin, Greek, Cyrillic, CJK, kana, Hangul and symbols/emoji.
/// Unsupported bidi and shaping scripts require a separately validated rendering/input profile.
pub fn validate_text(text: &str, policy: InputPolicy) -> Result<(), ContentError> {
    for (offset, character) in text.char_indices() {
        let scalar = character as u32;
        if character == '\r' {
            if text.as_bytes().get(offset + 1) != Some(&b'\n') {
                return Err(ContentError::new(
                    "terminal_control",
                    "bare carriage return is unsupported; use LF or CRLF line endings",
                )
                .at(offset));
            }
            continue;
        }
        if character == '\t' || character == '\n' {
            continue;
        }
        if character.is_whitespace() && !character.is_control() && policy == InputPolicy::Prose {
            continue;
        }
        validate_input_scalar(character).map_err(|error| error.at(offset))?;
        if policy == InputPolicy::Exact && character.is_whitespace() && character != ' ' {
            return Err(ContentError::new(
                "unsupported_whitespace",
                format!("exact text supports Space, Tab and LF; U+{scalar:04X} is unsupported"),
            )
            .at(offset));
        }
    }
    for (offset, grapheme) in text.grapheme_indices(true) {
        if grapheme.chars().count() > MAX_GRAPHEME_SCALARS {
            return Err(ContentError::new(
                "grapheme_too_large",
                format!("one grapheme exceeds {MAX_GRAPHEME_SCALARS} Unicode scalars"),
            )
            .at(offset));
        }
        if grapheme.contains('\u{200d}')
            && !grapheme
                .chars()
                .any(|c| (0x1f000..=0x1faff).contains(&(c as u32)))
        {
            return Err(ContentError::new(
                "unsupported_joiner",
                "zero-width joiners are supported only inside emoji graphemes",
            )
            .at(offset));
        }
        if !grapheme.chars().all(char::is_whitespace) && UnicodeWidthStr::width(grapheme) == 0 {
            return Err(ContentError::new(
                "zero_width_grapheme",
                "a standalone zero-width grapheme cannot form a visible target",
            )
            .at(offset));
        }
    }
    Ok(())
}

/// Admit one delivered input scalar without assuming its grapheme is complete.
/// Combining marks, emoji joiners/tags and variation selectors may continue an
/// earlier event. Targets additionally undergo complete-grapheme validation.
pub fn validate_input_scalar(character: char) -> Result<(), ContentError> {
    let scalar = character as u32;
    if matches!(scalar, 0x061c | 0x200e..=0x200f | 0x202a..=0x202e | 0x2066..=0x2069) {
        return Err(ContentError::new(
            "bidi_control",
            format!("bidirectional control U+{scalar:04X} is unsupported"),
        ));
    }
    if matches!(character, '\t' | '\n') {
        return Ok(());
    }
    if scalar <= 0x1f || (0x7f..=0x9f).contains(&scalar) {
        return Err(ContentError::new(
            "terminal_control",
            format!("terminal control U+{scalar:04X} is forbidden"),
        ));
    }
    if matches!(scalar, 0x00ad | 0x034f | 0x200b..=0x200c | 0x2060..=0x2065 | 0x206a..=0x206f | 0xfeff)
    {
        return Err(ContentError::new(
            "invisible_character",
            format!("invisible formatting character U+{scalar:04X} is unsupported"),
        ));
    }
    if !supported_scalar(scalar) {
        return Err(ContentError::new(
            "unsupported_script",
            format!("U+{scalar:04X} requires an unsupported script, shaping, or input profile"),
        ));
    }
    Ok(())
}

fn supported_scalar(c: u32) -> bool {
    matches!(c,
        0x0020..=0x024f | 0x0300..=0x052f | 0x1100..=0x11ff |
        0x1ab0..=0x1aff | 0x1d00..=0x1fff | 0x2000..=0x2bff |
        0x2de0..=0x2fdf | 0x3000..=0x31ff | 0x3200..=0x9fff |
        0xa640..=0xa69f | 0xa720..=0xa7ff | 0xa960..=0xa97f |
        0xab30..=0xab6f | 0xac00..=0xd7ff | 0xf900..=0xfaff |
        0xfe00..=0xfe2f | 0xfe30..=0xfe6f | 0xff00..=0xffef |
        0x1f000..=0x1faff | 0x20000..=0x323af | 0xe0020..=0xe007f |
        0xe0100..=0xe01ef)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn arbitrary_unicode_validation_is_bounded_and_spans_are_sound(input in ".{0,256}", exact in any::<bool>()) {
            let policy = if exact { InputPolicy::Exact } else { InputPolicy::Prose };
            if let Ok(target) = prepare_custom(input.as_bytes(), policy, false) {
                prop_assert!(!target.text.is_empty());
                let mut end = 0;
                for span in &target.graphemes {
                    prop_assert_eq!(span.bytes.start, end);
                    prop_assert!(span.bytes.end <= target.text.len());
                    prop_assert!(target.text.is_char_boundary(span.bytes.start));
                    prop_assert!(target.text.is_char_boundary(span.bytes.end));
                    prop_assert!(target.text[span.bytes.clone()].chars().count() <= MAX_GRAPHEME_SCALARS);
                    end = span.bytes.end;
                }
                prop_assert_eq!(end, target.text.len());
                for token in &target.tokens {
                    prop_assert!(token.units.start < token.units.end);
                    prop_assert!(token.units.end <= target.graphemes.len());
                    prop_assert!(token.bytes.end <= target.text.len());
                    prop_assert!(!target.text[token.bytes.clone()].chars().any(char::is_whitespace));
                }
                prop_assert_eq!(&target, &prepare_custom(target.text.as_bytes(), policy, false).unwrap());
            }
        }
    }

    #[test]
    fn prose_and_exact_preserve_their_distinct_contracts() {
        let prose = prepare_custom(
            "  cafe\u{301}\r\n\t世界  ".as_bytes(),
            InputPolicy::Prose,
            false,
        )
        .unwrap();
        assert_eq!(prose.text, "café 世界");
        assert_eq!(prose.graphemes.len(), 7);
        assert_eq!(prose.graphemes[5].width, 2);
        assert_eq!(prose.token(1), Some("世界"));
        let exact = prepare_custom(b"\tlet a = 1;\r\n", InputPolicy::Exact, false).unwrap();
        assert_eq!(exact.text, "\tlet a = 1;\n");
        assert_eq!(exact.graphemes[0].width, 0);
        assert_eq!(
            prepare_custom("e\u{301}".as_bytes(), InputPolicy::Exact, false)
                .unwrap()
                .text,
            "e\u{301}"
        );
        assert_eq!(
            prepare_custom("e\u{301}".as_bytes(), InputPolicy::Exact, true)
                .unwrap()
                .text,
            "é"
        );
    }

    #[test]
    fn validation_never_echoes_private_text() {
        for (input, kind) in [
            ("secret\u{1b}[31m", "terminal_control"),
            ("private\u{202e}", "bidi_control"),
            ("خاص", "unsupported_script"),
            ("नमस्ते", "unsupported_script"),
            ("\u{301}", "zero_width_grapheme"),
            ("a\r", "terminal_control"),
        ] {
            let error = prepare_custom(input.as_bytes(), InputPolicy::Prose, false).unwrap_err();
            assert_eq!(error.kind, kind);
            assert!(!error.to_string().contains("secret"));
            assert!(!error.to_string().contains("private"));
        }
    }

    #[test]
    fn reject_before_normalizing_pathological_clusters() {
        let input = format!("a{}", "\u{301}".repeat(32));
        assert_eq!(
            prepare_custom(input.as_bytes(), InputPolicy::Prose, false)
                .unwrap_err()
                .kind,
            "grapheme_too_large"
        );
        let valid = format!("a{}", "\u{301}".repeat(31));
        assert!(prepare_custom(valid.as_bytes(), InputPolicy::Prose, false).is_ok());
        assert!(prepare_custom("👩‍💻".as_bytes(), InputPolicy::Prose, false).is_ok());
        assert_eq!(
            prepare_custom(" \u{301}".as_bytes(), InputPolicy::Prose, false)
                .unwrap_err()
                .kind,
            "zero_width_grapheme"
        );
    }

    #[test]
    fn reject_bytes_empty_and_terminal_controls() {
        assert_eq!(
            prepare_custom(&[0xff], InputPolicy::Prose, false)
                .unwrap_err()
                .kind,
            "invalid_utf8"
        );
        assert_eq!(
            prepare_custom(b" \n\t", InputPolicy::Prose, false)
                .unwrap_err()
                .kind,
            "empty_target"
        );
        assert!(prepare_custom(b" \n\t", InputPolicy::Exact, false).is_ok());
        assert_eq!(
            prepare_custom(&vec![b'a'; MAX_CUSTOM_BYTES + 1], InputPolicy::Exact, false)
                .unwrap_err()
                .kind,
            "input_too_large"
        );
        for scalar in (0..=0x1f).chain(0x7f..=0x9f) {
            if matches!(scalar, 9 | 10 | 13) {
                continue;
            }
            let input = char::from_u32(scalar).unwrap().to_string();
            assert_eq!(
                prepare_custom(input.as_bytes(), InputPolicy::Prose, false)
                    .unwrap_err()
                    .kind,
                "terminal_control"
            );
        }
    }
}
