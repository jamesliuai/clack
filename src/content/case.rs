//! Generator v1 pins Unicode full uppercase independently of the Rust compiler.
use super::case_tables::UPPERCASE;

pub const CASE_MAPPING_VERSION: &str = "Unicode-17.0.0";

pub(super) fn uppercase(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii() {
            output.push(character.to_ascii_uppercase());
        } else if let Ok(index) = UPPERCASE.binary_search_by_key(&character, |entry| entry.0) {
            output.push_str(UPPERCASE[index].1);
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_unicode_tables_do_not_follow_the_compiler_version() {
        assert_eq!(CASE_MAPPING_VERSION, "Unicode-17.0.0");
        assert_eq!(unicode_normalization::UNICODE_VERSION, (17, 0, 0));
        assert_eq!(unicode_segmentation::UNICODE_VERSION, (17, 0, 0));
        assert_eq!(unicode_width::UNICODE_VERSION, (17, 0, 0));
        assert_eq!(
            uppercase("\u{a7cf}\u{a7d3}\u{a7d5}"),
            "\u{a7ce}\u{a7d2}\u{a7d4}"
        );
        assert_eq!(uppercase("ß\u{390}éσ"), "SSΙ\u{308}\u{301}ÉΣ");
        assert_eq!(uppercase("Aé字👩\u{200d}💻"), "AÉ字👩\u{200d}💻");
        assert!(UPPERCASE.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }
}
