//! Validate immutable bundled sources with the same importer used by the app.
#![allow(dead_code)]

#[path = "src/content/case.rs"]
mod case;
#[path = "src/content/case_tables.rs"]
mod case_tables;
#[path = "src/content/packs.rs"]
mod packs;
#[path = "src/content/validation.rs"]
mod validation;

use std::{env, fmt::Write, fs, path::PathBuf};
use validation::{
    ContentError, InputPolicy, MAX_PACK_BYTES, MAX_PACK_TOKEN_BYTES, MAX_PACK_TOKEN_GRAPHEMES,
    content_hash, validate_text,
};

fn main() {
    for file in [
        "build.rs",
        "src/content/case.rs",
        "src/content/case_tables.rs",
        "src/content/packs.rs",
        "src/content/validation.rs",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    let mut generated = String::new();
    for id in packs::BUNDLED_PACK_IDS {
        let directory = PathBuf::from("data/packs").join(id);
        let words_path = directory.join("words.txt");
        let metadata_path = directory.join("metadata.json");
        println!("cargo:rerun-if-changed={}", words_path.display());
        println!("cargo:rerun-if-changed={}", metadata_path.display());
        let words = fs::read(&words_path).expect("bundled words must be readable");
        let metadata = fs::read(&metadata_path).expect("bundled metadata must be readable");
        let pack = packs::LanguagePack::from_parts(&words, &metadata)
            .unwrap_or_else(|error| panic!("invalid bundled pack {id}: {error}"));
        assert_eq!(
            pack.metadata.id, *id,
            "bundled directory must match pack ID"
        );
        writeln!(generated, "fn prepare_{id}() -> LanguagePack {{").unwrap();
        writeln!(generated, "let prepared: &[(&str, &str, &[u16])] = &[").unwrap();
        for token in &pack.tokens {
            writeln!(
                generated,
                "({:?}, {:?}, &{:?}),",
                token.text, token.capitalized, token.widths
            )
            .unwrap();
        }
        writeln!(generated, "];\nLanguagePack {{ metadata: PackMetadata {{").unwrap();
        let metadata = &pack.metadata;
        writeln!(generated, "schema_version: {},", metadata.schema_version).unwrap();
        for (field, value) in [
            ("id", &metadata.id),
            ("language_tag", &metadata.language_tag),
            ("revision", &metadata.revision),
            ("source", &metadata.source),
            ("license", &metadata.license),
            ("content_hash", &metadata.content_hash),
            ("text_direction", &metadata.text_direction),
        ] {
            writeln!(generated, "{field}: {value:?}.into(),").unwrap();
        }
        writeln!(generated, "supported_input_policy: InputPolicy::Prose,").unwrap();
        writeln!(generated, "token_count: {},", metadata.token_count).unwrap();
        writeln!(generated, "}}, tokens: prepared.iter().map(|&(text, capitalized, widths)| PackToken {{ text: text.into(), capitalized: capitalized.into(), widths: widths.to_vec() }}).collect() }}\n}}").unwrap();
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"));
    fs::write(output.join("bundled_packs.rs"), generated)
        .expect("prepared bundled data must be writable");
}
