use std::{borrow::Cow, collections::HashSet};
use unicode_normalization::{UnicodeNormalization, is_nfc};

use super::{
    ContentError, InputPolicy, MAX_CANONICAL_CUSTOM_BYTES, Normalization, PreparedText, SplitMix64,
    validate_text,
};

pub const PRACTICE_WORDS: usize = 25;
pub const MAX_PRACTICE_BYTES: usize =
    PRACTICE_WORDS * MAX_CANONICAL_CUSTOM_BYTES + PRACTICE_WORDS - 1;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PracticeKind {
    Missed,
    Slow,
}

/// Immutable per-token summaries produced by the engine. Durations begin at the
/// previous token boundary; input construction and timing remain engine-owned.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PracticeToken {
    pub target: String,
    pub expected_units: usize,
    pub elapsed_us: u64,
    pub correctly_completed: bool,
    pub had_incorrect_attempt: bool,
    pub has_final_mistake_or_omission: bool,
    pub had_correction: bool,
}

pub fn practice_candidates(
    tokens: &[PracticeToken],
    kind: PracticeKind,
) -> Result<Vec<String>, ContentError> {
    let candidates: Vec<&PracticeToken> = match kind {
        PracticeKind::Missed => tokens
            .iter()
            .filter(|token| token.had_incorrect_attempt || token.has_final_mistake_or_omission)
            .collect(),
        PracticeKind::Slow => {
            let mut eligible: Vec<_> = tokens
                .iter()
                .filter(|token| !token.target.trim().is_empty())
                .enumerate()
                .skip(1)
                .filter(|(_, token)| {
                    token.correctly_completed && !token.had_correction && token.expected_units > 0
                })
                .collect();
            if eligible.len() < 8 {
                return Err(ContentError::new(
                    "insufficient_slow_tokens",
                    "slow-word practice needs eight correctly completed, uncorrected tokens after the first token",
                ));
            }
            eligible.sort_by(|(left_index, left), (right_index, right)| {
                (u128::from(right.elapsed_us) * left.expected_units as u128)
                    .cmp(&(u128::from(left.elapsed_us) * right.expected_units as u128))
                    .then(left_index.cmp(right_index))
            });
            let count = eligible.len().div_ceil(4);
            eligible
                .into_iter()
                .take(count)
                .map(|(_, token)| token)
                .collect()
        }
    };
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for candidate in candidates {
        let target = candidate.target.trim();
        if !target.is_empty() && seen.insert(target) {
            unique.push(target.to_owned());
        }
    }
    if unique.is_empty() {
        return Err(ContentError::new(
            "no_practice_candidates",
            "there are no missed words to practice in this result",
        ));
    }
    Ok(unique)
}

/// Shuffled cycles make a 25-word target. Already-punctuated original tokens are
/// copied exactly; neither number replacement nor punctuation is reapplied.
pub fn prepare_practice(candidates: &[String], seed: u64) -> Result<PreparedText, ContentError> {
    if candidates.is_empty() {
        return Err(ContentError::new(
            "no_practice_candidates",
            "there are no words to practice",
        ));
    }
    if candidates
        .iter()
        .any(|word| word.is_empty() || word.chars().any(char::is_whitespace))
    {
        return Err(ContentError::new(
            "invalid_practice_token",
            "practice candidates must be nonempty original target tokens without whitespace",
        ));
    }
    // These are prepared target tokens, not a new raw custom file. Applying the
    // 1 MiB source-input cap to their 25-word expansion would reject legitimate
    // practice from a long but valid custom target.
    let mut normalized = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if candidate.len() > MAX_CANONICAL_CUSTOM_BYTES {
            return Err(ContentError::new(
                "practice_token_too_large",
                "practice token exceeds the canonical custom-token bound",
            ));
        }
        validate_text(candidate, InputPolicy::Prose)?;
        let canonical = if is_nfc(candidate) {
            Cow::Borrowed(candidate.as_str())
        } else {
            Cow::Owned(candidate.nfc().collect::<String>())
        };
        if canonical.len() > MAX_CANONICAL_CUSTOM_BYTES {
            return Err(ContentError::new(
                "practice_token_too_large",
                "normalized practice token exceeds the canonical custom-token bound",
            ));
        }
        validate_text(&canonical, InputPolicy::Prose)?;
        normalized.push(canonical);
    }
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for candidate in &normalized {
        if seen.insert(candidate.as_ref()) {
            unique.push(candidate.as_ref());
        }
    }
    let mut rng = SplitMix64::new(seed);
    let mut selected = Vec::with_capacity(PRACTICE_WORDS);
    while selected.len() < PRACTICE_WORDS {
        rng.shuffle(&mut unique);
        if unique.len() > 1 && selected.last() == unique.first() {
            unique.swap(0, 1);
        }
        selected.extend(unique.iter().take(PRACTICE_WORDS - selected.len()).copied());
    }
    let text = selected.join(" ");
    if text.len() > MAX_PRACTICE_BYTES {
        return Err(ContentError::new(
            "practice_target_too_large",
            "practice target exceeds the derived 25-token bound",
        ));
    }
    Ok(PreparedText::from_validated(
        text,
        InputPolicy::Prose,
        Normalization::Nfc,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(target: &str, elapsed_us: u64) -> PracticeToken {
        PracticeToken {
            target: target.into(),
            expected_units: target.len(),
            elapsed_us,
            correctly_completed: true,
            had_incorrect_attempt: false,
            has_final_mistake_or_omission: false,
            had_correction: false,
        }
    }

    #[test]
    fn repaired_mistakes_and_omissions_use_original_tokens_once() {
        let mut repaired = token("Hello,", 100);
        repaired.had_incorrect_attempt = true;
        repaired.had_correction = true;
        let mut omitted = token("world.", 100);
        omitted.has_final_mistake_or_omission = true;
        let candidates =
            practice_candidates(&[repaired.clone(), repaired, omitted], PracticeKind::Missed)
                .unwrap();
        assert_eq!(candidates, ["Hello,", "world."]);
        let target = prepare_practice(&candidates, 9).unwrap();
        assert_eq!(target.tokens.len(), 25);
        assert!(
            target
                .text
                .split(' ')
                .all(|word| candidates.iter().any(|candidate| candidate == word))
        );
        assert_eq!(target, prepare_practice(&candidates, 9).unwrap());
    }

    #[test]
    fn slow_selection_uses_ratio_excludes_first_and_corrections() {
        let mut tokens = vec![token("ignored", u64::MAX)];
        for index in 0..8 {
            tokens.push(token(&format!("word{index}"), (index + 1) * 100));
        }
        let mut corrected = token("corrected", u64::MAX);
        corrected.had_correction = true;
        tokens.push(corrected);
        assert_eq!(
            practice_candidates(&tokens, PracticeKind::Slow).unwrap(),
            ["word7", "word6"]
        );
        tokens.remove(1);
        assert_eq!(
            practice_candidates(&tokens, PracticeKind::Slow)
                .unwrap_err()
                .kind,
            "insufficient_slow_tokens"
        );
    }

    #[test]
    fn exact_token_labels_trim_separators_and_skip_empty_segments() {
        let mut whitespace = token(" \t\n", 1000);
        whitespace.had_incorrect_attempt = true;
        let mut first = token("let ", 100);
        first.had_incorrect_attempt = true;
        let mut duplicate = token("let\n", 200);
        duplicate.has_final_mistake_or_omission = true;
        let candidates = practice_candidates(
            &[whitespace.clone(), first, duplicate],
            PracticeKind::Missed,
        )
        .unwrap();
        assert_eq!(candidates, ["let"]);
        assert_eq!(prepare_practice(&candidates, 42).unwrap().tokens.len(), 25);

        let mut slow = vec![whitespace, token("first ", u64::MAX)];
        for index in 0..8 {
            slow.push(token(&format!("word{index}\n"), (index + 1) * 100));
        }
        assert_eq!(
            practice_candidates(&slow, PracticeKind::Slow).unwrap(),
            ["word7", "word6"]
        );
    }
}
