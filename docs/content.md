# Content, replay, and source provenance

The content subsystem performs no filesystem, terminal, network, SQL, or clock
operations. Composition reads bounded files and passes bytes to it before Ready.
The reducer receives prepared targets. It does not parse packs, generate words,
or perform Unicode preparation during ordinary input.

Composition can cache a prepared `Arc<LanguagePack>` and pass it to
`Generator::from_shared`; the generator owns that shared reference and its PRNG
state. Imported packs require no leaked allocations, self-referential application
objects, or regeneration of earlier chunks. `Generator::new(&LanguagePack, ...)`
is a convenience that clones a pack outside the input path.

## Included content

| Stable pack ID | Unique tokens | Language | Revision |
|---|---:|---|---|
| `english_200` | 200 | English (`en`) | `moby-clack-1` |
| `english_1000` | 1,000 | English (`en`) | `moby-clack-1` |
| `english_10000` | 10,000 | English (`en`) | `moby-clack-1` |
| `french_200` | 200 | French (`fr`) | `moby-clack-1` |
| `german_200` | 200 | German (`de`) | `moby-clack-1` |
| `spanish_200` | 200 | Spanish (`es`) | `moby-clack-1` |

All six packs use the same metadata schema and validator as imported packs. Their
exact normalized token order is versioned. Each directory under `data/packs/`
contains `words.txt` and `metadata.json`. The default pack is initialized separately;
launching an ordinary English 200 test does not parse the larger dictionaries.

Eight local passages provide two choices in each quote category: short 1–30 words,
medium 31–75, long 76–150, and extended 151–300. IDs and attribution are in
`data/quotes/quotes.json`. Selection is uniform within the requested category,
excluding the last ID whenever another choice exists. Explicit selection by ID
does not apply the no-repeat rule. Punctuation and capitalization are retained;
prose whitespace is normalized. Random-word modifiers never transform quotations.

The code preset is the original `data/code-sample.rs.txt` function, with its own
metadata. It is literal Rust text with indentation and line breaks.

## Import format

Create a directory containing UTF-8 `words.txt` and `metadata.json`, then use
`clack languages import ./my-pack`. Imports are offline data validation. They do not
execute scripts, inspect package managers, or download dependencies.

For a two-token example, `words.txt` is:

```text
cat
dog
```

The metadata for exactly those two tokens is:

```json
{
  "schema_version": 1,
  "id": "my_pack",
  "language_tag": "en",
  "revision": "1",
  "source": "Original selection by the pack author",
  "license": "CC0-1.0",
  "content_hash": "f641fdcd8af73b2f6334ab63c13d2eb857cd16f2aa4ea0ba20ba0eb9627918b5",
  "text_direction": "ltr",
  "supported_input_policy": "prose",
  "token_count": 2
}
```

Use the actual source and license for your content. IDs/revisions permit 1–64
ASCII letters, digits, underscores, hyphens, and dots, with no leading dot or `..`.
The language tag is checked for language/subtag syntax. Unsupported direction or
input policy is rejected; 1.0 random packs require left-to-right prose.

The hash is lowercase SHA-256 of NFC-normalized tokens in their declared order,
separated by LF and followed by a final LF. CRLF files are accepted. No other
whitespace is permitted within a pack token. Empty lines and duplicates after NFC
normalization are errors rather than silent changes to the sampling distribution.
Unknown metadata fields are rejected. A revision is part of replay identity;
changing token order, spelling, or selection requires a new revision/hash.

Bounds are 16 MiB of raw pack text, 64 KiB of metadata, 100,000 tokens, 8 KiB and
128 graphemes per token, and 32 Unicode scalars per grapheme. Tokens are validated
before and after normalization. Diagnostics identify the line, byte position,
condition, and limit where relevant; they never quote private input content.

## Custom text and supported Unicode

Custom file, literal, and standard-input sources are limited to 1 MiB of raw UTF-8.
The caller must use a bounded read, including one extra byte to detect overflow.
`prepare_custom` rejects oversized buffers, invalid UTF-8, empty normalized targets,
and pathological graphemes before Ready. Raw-byte admission and prepared-target
size are distinct: NFC can expand UTF-8. Prepared custom targets have a conservative
4 MiB ceiling. The exhaustive Unicode 17 test checks every scalar value, proves
canonical decomposition is at most three times its source byte length, and checks
that NFC-stable scalars do not expand on recomposition. Reordering changes no byte
counts; CRLF and prose whitespace normalization only reduce source bytes. Thus the
4 MiB bound accepts every valid source under the 1 MiB raw limit while keeping
canonical storage bounded. A maximum-size expanding source is exercised through
preparation, SQLite review, original-file hash matching, and explicit text export.
A command-line literal can remain in shell history: use a file or standard input
for private material.

Prose collapses Unicode whitespace to one ASCII Space, trims it at the endpoints,
and normalizes to NFC. Exact text preserves code points and whitespace, normalizing
only CRLF to LF unless exact NFC normalization is explicitly selected. Exact text
supports Space, Tab, and LF; bare CR and other whitespace forms are rejected with a
diagnostic. An all-whitespace exact target is meaningful and is allowed. Tab/newline
are logical units, not expanded into scored spaces. Tab display width depends on
the current column and the configured stops.

ESC, all C0/C1 controls except supported Tab/LF/CRLF, bidi controls, unsafe invisible
formatting, unsupported script ranges, orphan zero-width graphemes, and clusters
longer than 32 scalars are rejected. Validation runs before and after normalization
so trimming a space cannot leave an invisible orphan combining mark.

The conservative repertoire includes Latin, Greek, Cyrillic, CJK, kana, Hangul,
common symbols, and emoji. Emoji joiner sequences are accepted as bounded graphemes;
non-emoji zero-width joiners are rejected. Hebrew/Arabic bidi text and complex
shaping scripts including Indic, Thai, Lao, Myanmar, and Khmer need a separately
validated input/rendering profile and are rejected in 1.0. Accepting a scalar range
does not establish composition-aware IME timing, all font glyphs, or terminal
rendering compatibility. Terminal matrix validation is separate.

`PreparedText` stores the normalized text, content hash, stable grapheme byte spans,
non-whitespace token spans in bytes and logical units, policy, normalization, and
cached cell widths. Byte offsets, scalar count, grapheme positions, and cells remain
different quantities. The engine owns provisional grapheme input and matching.

## Generator version 1

Replay identity stores `generator_version = 1`, `prng = "splitmix64"`, the full
unsigned 64-bit seed, pack ID/revision/content hash, and all modifier parameters.
`Generator::from_identity` rejects unsupported versions or a changed pack. A seed
alone does not identify a sample. Generator v1 pins case data to Unicode 17.0.0,
independently of the Rust compiler's Unicode tables. The locked normalization
(`unicode-normalization` 0.1.25), segmentation (`unicode-segmentation` 1.13.3), and
width (`unicode-width` 0.2.2) crates also use Unicode 17.0.0. Changes to generated
capitalization/normalization require a new generator or pack revision and new
golden fixtures.

First-grapheme capitalization applies the locale-independent full uppercase
mappings in the official [Unicode 17 UnicodeData file](https://www.unicode.org/Public/17.0.0/ucd/UnicodeData.txt)
and unconditional [SpecialCasing mappings](https://www.unicode.org/Public/17.0.0/ucd/SpecialCasing.txt),
then NFC-normalizes the result. It can expand one grapheme into several, such as
`ß` to `SS`; prepared target spans/widths are recomputed for that actual output.
The prepared capitalized token is validated again, including the 32-scalar cluster
limit. Rust 1.88 embeds older uppercase tables than current Rust, so using
`str::to_uppercase()` here would silently change seeded samples for newly mapped
Latin letters. Explicit edge goldens run on both compilers.

The generated table, hashes, source URLs, and mapping rule are recorded in
`data/unicode-case-manifest.json`; the full Unicode-3.0 notice is retained in
`data/licenses/Unicode-3.0.txt`. It is built into the executable, with no runtime
locale or download dependency.

The implementation uses the public-domain fixed-increment
[SplitMix64 reference by Sebastiano Vigna](https://prng.di.unimi.it/splitmix64.c).
State starts at the supplied seed. Each draw adds `0x9e3779b97f4a7c15` modulo 2^64,
xor-shifts by 30 and multiplies by `0xbf58476d1ce4e5b9`, xor-shifts by 27 and
multiplies by `0x94d049bb133111eb`, then xor-shifts by 31. Every arithmetic operation
wraps as unsigned 64-bit. The original notice is in `data/licenses/SplitMix64.txt`.

To sample `[0,n)`, reject draws below `(-n mod 2^64) mod n`, then return `draw mod n`.
The accepted range has size divisible by `n`; there is no floating-point or
platform-sized integer random sampling. The first token uses `[0,pack_length)`.
Later tokens use `[0,pack_length-1)` and skip the previous index, producing the
uniform conditional distribution without an immediate source-token duplicate.
A one-token pack necessarily repeats. Duplicates created coincidentally by number
replacement are possible; the nonadjacent rule applies to selected source tokens.

The draw order for every source token is:

1. Select its source index, excluding the preceding source index.
2. If numbers are enabled, draw `[0,100)` and replace when below `number_percent`
   (default 10). On replacement draw an integer digit count in the configured
   inclusive range (default 1–4), then sample uniformly from the decimal numbers
   of that length. The first digit is nonzero, including one-digit replacements.
3. If punctuation is enabled and no sentence remains, sample its length from the
   configured inclusive range (default 4–12). Capitalize the first original
   grapheme when the token was not replaced with a number.
4. Decrease the sentence's remaining count. At zero append a period. Otherwise
   draw `[0,100)` and append a comma when below `comma_percent` (default 10).

Numbers are therefore processed before punctuation. The modifiers are independent,
and inactive modifiers do not consume RNG draws. `Modifiers` stores effective
probabilities and length ranges. Finite word tests enforce the exact requested
count and a final period when punctuation is enabled; the final sentence can be
shortened, including counts below four. A comma at that endpoint becomes a period.

Timed chunks contain 256 words and preserve PRNG, previous-word, and sentence state.
Joining chunks with one Space yields the same target as one larger stream request.
No sentence is artificially ended at a chunk boundary. The application prepares
additional chunks outside the reducer, retaining at least two screens of lookahead.
Chunk preparation thresholds must not change PRNG state or generation order.

Golden vectors cover seeds 0, 42, and `u64::MAX`, English and accented French,
and all four punctuation/numbers combinations. They are stored in
`data/generator-v1-golden.json` and were created with an independent integer Python
reference. Tests compare exact output text and the pack content hashes, not merely
whether two instances of the same implementation agree.

## Practice preparation

Missed-word selection uses original tokens with any historical incorrect attempt
or final mistake/omission; repaired errors remain candidates. Selection is deduped.
Slow-word selection excludes the first token and corrected/incomplete tokens,
requires eight eligible tokens, sorts by elapsed microseconds / expected units
using integer cross products, and takes the slowest `ceil(n/4)` entries. Ties retain
source order. Original punctuation is preserved.

Practice uses seeded Fisher–Yates shuffles and repeated cycles to build exactly
25 tokens. This prepared expansion uses the derived bound of 25 × 4 MiB plus
24 separator bytes; it does not incorrectly reapply the 1 MiB raw-file limit to
repetitions of a legal original token. Candidate validation and NFC occur before
assembly, duplicates after normalization are removed, and each candidate remains
within the canonical token bound. Multiple candidates do not repeat at a cycle
boundary. No-candidate and
insufficient-slow-token cases return explanations suitable for disabled UI actions.
The application owns temporary practice settings, restoration, and record exclusion.

## Provenance and licensing decisions

The application has independent code and branding. No Monkeytype code, word lists,
or quote collections were copied. Bundled data has its own licensing, separate
from the Rust application's MIT license.

Moby Words II and Moby Language II were compiled by Grady Ward. Both downloaded
primary documentation files state that the author granted them to the public
domain in January 2001. Copies are in `data/licenses/Moby-english-author-grant.txt`
and `data/licenses/Moby-language-author-grant.txt`. The
[Moby Words source documentation](https://www.gutenberg.org/files/3201/3201-0.txt)
describes its common dictionary and frequency lists; the
[Moby Language source distribution](https://www.gutenberg.org/files/3206/)
contains its corresponding author grant. `LicenseRef-Moby-Public-Domain` identifies
that explicit author grant. The distributor's additional United States public
domain notice is preserved in `data/licenses/Moby-Public-Domain.txt`.

English 200 takes the first 200 unique lowercase alphabetic entries of the Moby
frequency list. English 1,000 deduplicates that list and supplements it in the
Moby 1992 Internet frequency order until 1,000 tokens exist. Single-letter entries
other than `a` and `i` are excluded. These are historical vocabulary sources;
the names indicate token counts, not a measured modern language frequency ranking.
English 10,000 retains those 1,000 tokens and adds 9,000 distinct 2–12 letter entries
from Moby's common dictionary: SHA-256 ordering of the fixed string
`typ-english-10000-v1:` (the fixed pre-rename seed, retained for reproducibility)
followed by each candidate provides a reproducible spread;
the added tokens are stored alphabetically. It is an expanded vocabulary practice
pack, not a claim to be the 10,000 most frequent contemporary English words.

The three other language packs are small curated vocabulary selections whose
specific source forms were checked against the corresponding Moby list. The
current Moby Language documentation explicitly says its historical accent encoding
is not comprehensively documented. No bulk automatic decoding is assumed correct:
the selection starts from correctly spelled curated words and verifies each known
ASCII source form. `data/language-source-mappings.json` records every selected
token and source spelling. German source lines with unlabelled non-ASCII legacy
bytes are excluded. No unverifiable transliteration is included. The reproducible
selection and limited verified transformations are in `data/rebuild.py`.

Quote passages come from Lewis Carroll's *Alice's Adventures in Wonderland* (1865;
author died 1898), Jane Austen's *Pride and Prejudice* (1813; died 1817), and Charles
Dickens's *A Tale of Two Cities* (1859; died 1870). Their individual primary catalog
records identify authors and United States public-domain status:
[Carroll](https://www.gutenberg.org/ebooks/11),
[Austen](https://www.gutenberg.org/ebooks/1342), and
[Dickens](https://www.gutenberg.org/ebooks/98). Selection uses only the original
literary text, excludes editorial introductions/illustrations, removes the plain
text files' italic-delimiter underscores, and normalizes whitespace/NFC. Each
passage records its source, author, work, stable ID, revision, and content hash.
`LicenseRef-Public-Domain-Literature` means the original literary text's public
domain status, not an inferred license from a quote collection's repository.

The Rust code example and original test fixtures are dedicated by clack contributors
under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/), with the full
legal code in `data/licenses/CC0-1.0.txt`. Their metadata does not imply authorship
of the older literary or dictionary sources.

## Reproduction and coverage

`data/source-manifest.json` records the exact downloaded URLs, source byte SHA-256
digests, filenames, and review date. Place those reviewed inputs in a directory
using the declared filenames, then run:

```sh
python3 data/rebuild.py --source-dir /path/to/reviewed-sources
python3 data/rebuild_unicode_case.py --source-dir /path/to/unicode-17-sources --check
cargo test --lib content
cargo test --test content_adversarial
cargo +1.88.0 test --test content_adversarial
```

The rebuild script has no network capability and fails on a source hash mismatch.
A future upstream file update cannot silently change bundled samples. No build,
runtime launch, or user pack import runs this script.

The Stage D rebuild checked all nine original source hashes and reproduced all
14 pack/quote/mapping artifacts byte-for-byte; raw evidence is in
`docs/measurements/content-rebuild.json`. The independent case-table rebuild
verifies all three official input hashes before reproducing its 1,580 mappings.

| Specification requirement | Content implementation evidence | Status |
|---|---|---|
| §5 English 200/1,000/10,000 and French/German/Spanish | Six manifests; tests verify exact counts, uniqueness, hashes | Implemented/tested |
| §5 offline metadata-backed pack import | `LanguagePack::from_parts`; malformed/duplicate/control/hash tests | Implemented/tested; CLI persistence owned by composition |
| §5 uniform seeded generation and algorithm version | SplitMix64; unbiased range; conditional sampling; golden vectors | Implemented/tested |
| §5 deterministic timed chunks | Partition-invariance tests with all four modifiers | Implemented/tested; viewport lookahead owned by composition |
| §5 punctuation and numbers in required order | Independent golden vectors and number/sentence boundary tests | Implemented/tested |
| §5 attributed quote lengths/IDs/no immediate repeats | Eight validated passages; every category has two choices | Implemented/tested |
| §5 custom/exact/CRLF/code/1 MiB limits | Prepared targets and literal code fixture | Implemented/tested; bounded file reads owned by composition |
| §5 pathological-token/cluster bounds | Import boundary tests and Unicode properties | Implemented/tested |
| §7.4 stable grapheme spans, NFC, exact preservation, widths | Composed/decomposed/CJK/emoji fixtures; arbitrary-Unicode property | Implemented/tested; live input composition owned by engine |
| §9.1 missed/slow deduplication and 25-word practice | Candidate and deterministic practice tests | Implemented/tested; temporary settings owned by application |
| §10 parsing/generation outside input reducer | No terminal/filesystem/clock imports in content module | Implemented; reducer allocation gate owned by engine |
| §12 untrusted text, privacy diagnostics, shaping rejection | Control sweeps, bidi/shaping/zero-width tests; text-free diagnostics | Implemented/tested |
| §15 cross-platform determinism | Exact portable vectors; pinned Unicode 17 tables and expansion bounds; Rust 1.88/current edge goldens | Local compiler tests completed; other OS executions remain external |
| §16 content release manifests | Source hashes, author grants, per-pack/per-quote metadata, CC0 notice | Implemented; packaging must include notices |

Actual terminal IME/glyph behavior and other-OS test executions require the release
terminal/platform matrix. Content-unit tests are not evidence that those external
checks have been performed.
