//! Independent edge fixtures for source validation and versioned generation.
use clack::content::{self, Generator, InputPolicy, LanguagePack, Modifiers, PackMetadata};
use unicode_normalization::UnicodeNormalization;

fn pack(text: &str) -> Result<LanguagePack, content::ContentError> {
    let canonical: String = text.nfc().collect();
    let metadata = PackMetadata {
        schema_version: 1,
        id: "unicode_edge_fixture".into(),
        language_tag: "en".into(),
        revision: "1".into(),
        source: "Original Unicode edge fixture".into(),
        license: "CC0-1.0".into(),
        content_hash: content::content_hash(canonical.as_bytes()),
        text_direction: "ltr".into(),
        supported_input_policy: InputPolicy::Prose,
        token_count: text.lines().count(),
    };
    LanguagePack::from_parts(text.as_bytes(), &serde_json::to_vec(&metadata).unwrap())
}

#[test]
fn unicode_17_capitalization_goldens_match_on_msrv_and_current_rust() {
    // These Latin mappings were added in Unicode 17. Rust 1.88 embeds Unicode
    // 16 case tables; generator v1 must still produce these exact targets.
    let cases = [
        ("\u{a7cf}\n", "\u{a7ce}. \u{a7ce}. \u{a7ce}."),
        ("\u{a7d3}\n", "\u{a7d2}. \u{a7d2}. \u{a7d2}."),
        ("\u{a7d5}\n", "\u{a7d4}. \u{a7d4}. \u{a7d4}."),
        ("ßeta\n", "SSeta. SSeta. SSeta."),
        ("ΐτα\n", "Ϊ\u{301}τα. Ϊ\u{301}τα. Ϊ\u{301}τα."),
        ("éclair\n", "Éclair. Éclair. Éclair."),
    ];
    let parameters = Modifiers {
        punctuation: true,
        sentence_min_words: 1,
        sentence_max_words: 1,
        comma_percent: 0,
        ..Modifiers::default()
    };
    for (source, expected) in cases {
        let pack = pack(source).unwrap();
        for seed in [0, 42, u64::MAX] {
            let mut generator = Generator::new(&pack, seed, parameters).unwrap();
            let identity = generator.identity().clone();
            let prepared = generator.finite_words(3).unwrap();
            assert_eq!(prepared.text, expected);
            assert_eq!(prepared.tokens.len(), 3);
            content::validate_text(&prepared.text, InputPolicy::Prose).unwrap();
            let mut replay = Generator::from_identity(&pack, &identity).unwrap();
            assert_eq!(replay.finite_words(3).unwrap(), prepared);
        }
    }
}

#[test]
fn full_case_expansion_preserves_cluster_limits_after_normalization() {
    let accepted = format!("ΐ{}\n", "\u{301}".repeat(30));
    let accepted_pack = pack(&accepted).unwrap();
    assert_eq!(accepted_pack.tokens[0].capitalized.chars().count(), 32);
    let rejected = format!("ΐ{}\n", "\u{301}".repeat(31));
    // The original cluster has 32 scalars and is valid. Capitalization adds
    // one scalar after NFC, so the prepared form must be rejected at import.
    content::validate_text(rejected.trim_end(), InputPolicy::Prose).unwrap();
    let error = pack(&rejected).unwrap_err();
    assert_eq!(error.kind, "grapheme_too_large");
    assert_eq!(error.line, Some(1));
    assert!(!error.to_string().contains('ΐ'));
}

#[test]
fn capitalized_metadata_tracks_multiple_graphemes_without_changing_source_identity() {
    let pack = pack("ß\n").unwrap();
    assert_eq!(pack.tokens[0].widths, [1]);
    assert_eq!(pack.tokens[0].capitalized, "SS");
    assert_eq!(pack.canonical_words(), "ß\n");
    let mut generator = Generator::new(
        &pack,
        42,
        Modifiers {
            punctuation: true,
            ..Modifiers::default()
        },
    )
    .unwrap();
    let target = generator.finite_words(1).unwrap();
    assert_eq!(target.text, "SS.");
    assert_eq!(target.graphemes.len(), 3);
    assert_eq!(target.tokens[0].units, 0..3);
    assert_eq!(target.content_hash, content::content_hash(b"SS."));
}

#[test]
fn pinned_unicode_canonical_expansion_has_a_conservative_storage_ceiling() {
    assert_eq!(unicode_normalization::UNICODE_VERSION, (17, 0, 0));
    assert_eq!(
        content::MAX_CANONICAL_CUSTOM_BYTES,
        4 * content::MAX_CUSTOM_BYTES
    );
    let mut checked = 0;
    let mut largest_nfd_bytes_per_source_byte = 0;
    for scalar in 0..=0x10ffff {
        let Some(character) = char::from_u32(scalar) else {
            continue;
        };
        let mut nfd_bytes = 0;
        unicode_normalization::char::decompose_canonical(character, |part| {
            nfd_bytes += part.len_utf8();
        });
        assert!(nfd_bytes <= 3 * character.len_utf8(), "U+{scalar:04X}");
        largest_nfd_bytes_per_source_byte =
            largest_nfd_bytes_per_source_byte.max(nfd_bytes.div_ceil(character.len_utf8()));
        if nfd_bytes < character.len_utf8() {
            let source = character.to_string();
            assert_ne!(source.nfc().collect::<String>(), source, "U+{scalar:04X}");
        }
        checked += 1;
    }
    assert_eq!(checked, 0x110000 - 0x800);
    assert_eq!(largest_nfd_bytes_per_source_byte, 3);
    // Canonical reordering preserves byte count. Every scalar retained by NFC
    // has an NFD at least as large as itself (checked above), so recomposition
    // cannot make the final NFC string larger than its 3x-bounded NFD form.
}

#[test]
fn nfc_expansion_does_not_turn_the_raw_input_limit_into_a_prepared_target_limit() {
    let source = format!("{}a", "a\u{344}".repeat(content::MAX_CUSTOM_BYTES / 3));
    assert_eq!(source.len(), content::MAX_CUSTOM_BYTES);
    for (policy, normalize) in [(InputPolicy::Prose, false), (InputPolicy::Exact, true)] {
        let prepared = content::prepare_custom(source.as_bytes(), policy, normalize).unwrap();
        assert!(prepared.text.len() > content::MAX_CUSTOM_BYTES);
        assert!(prepared.text.len() <= content::MAX_CANONICAL_CUSTOM_BYTES);
        assert!(prepared.text.starts_with("ä\u{301}"));
        assert_eq!(prepared.text, source.nfc().collect::<String>());
        assert_eq!(
            content::prepare_custom(prepared.text.as_bytes(), policy, normalize)
                .unwrap_err()
                .kind,
            "input_too_large",
            "the larger canonical representation is not permission for a larger raw source"
        );
    }
}

#[test]
fn imported_token_byte_and_grapheme_limits_apply_after_canonical_normalization() {
    // 69 legal clusters and exactly 8 KiB of raw token bytes. NFC expands the
    // Greek dialytika-tonos mark after 'a', crossing the normalized token bound.
    let cluster = format!("a\u{344}{}", "\u{e0100}".repeat(29));
    let raw = format!("{}𠮷{}", cluster.repeat(68), "\u{e0100}".repeat(24));
    assert_eq!(raw.len(), content::MAX_PACK_TOKEN_BYTES);
    content::validate_text(&raw, InputPolicy::Prose).unwrap();
    assert!(raw.nfc().collect::<String>().len() > content::MAX_PACK_TOKEN_BYTES);
    let error = pack(&(raw + "\n")).unwrap_err();
    assert_eq!(error.kind, "token_too_large");
    assert_eq!(error.line, Some(1));

    let raw = format!("q\u{344}{}", "\u{301}".repeat(30));
    content::validate_text(&raw, InputPolicy::Prose).unwrap();
    let error = pack(&(raw + "\n")).unwrap_err();
    assert_eq!(error.kind, "grapheme_too_large");
    assert_eq!(error.line, Some(1));
}

#[test]
fn prepared_practice_validates_original_tokens_and_has_its_own_derived_bound() {
    assert_eq!(
        content::MAX_PRACTICE_BYTES,
        25 * content::MAX_CANONICAL_CUSTOM_BYTES + 24
    );
    let target = content::prepare_practice(&["é,".into(), "e\u{301},".into()], 42).unwrap();
    assert_eq!(target.tokens.len(), 25);
    assert!(target.text.split(' ').all(|token| token == "é,"));
    for candidate in ["private\u{1b}[2J", "two words", "\u{301}"] {
        let error = content::prepare_practice(&[candidate.into()], 42).unwrap_err();
        assert!(!error.to_string().contains(candidate));
    }
    let oversized = "a".repeat(content::MAX_CANONICAL_CUSTOM_BYTES + 1);
    assert_eq!(
        content::prepare_practice(&[oversized], 42)
            .unwrap_err()
            .kind,
        "practice_token_too_large"
    );
}
