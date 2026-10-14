# Security and performance review fixes

This revision addresses confirmed memory, rendering, storage, terminal-input,
and distribution findings. The finding labels below refer to that review;
regressions exercise the resulting behavior and its resource bounds.

| Finding | Resolution |
| --- | --- |
| P1 | Untouched words have no correction buffers, and visited words no longer reserve 32 unused spare slots. A small caret lookahead, reachable words in each text event, and the full prepared commit stack are warmed outside the reducer. Grapheme counts and automatic indentation keep multi-character events bounded and prepared. |
| P2 | Layout stops a few rows beyond the caret, resumes unfinished tokens, and preserves unchanged character prefixes for long-token edits. It no longer caches every untyped character. |
| P3 | File exports use `BufWriter`, explicitly flush before `sync_all`, and remove failed partial outputs. JSON and JSONL files match stdout bytes. |
| P4 | Composite partial classification indexes include profile and history order. Existing schema-1 databases receive them atomically on writable opens; read-only commands do not mutate databases. |
| P5 | Current-profile queries resolve the effective source identity without generating a sample or constructing an engine. |
| P6 | Settings changes reuse the session's validated imported pack. Fresh launches still validate mutable imported files; see the limitation below. |
| P7 | Unrestricted all-profile statistics aggregate the existing `profile_stats` summaries, including outcome-only filters. |
| P8 | Bundled packs are validated and prepared during the build, then shared through `Arc` at runtime. |
| P9 | Custom-source preparation retains normalized text/hash without unused span arrays. Raw bytes and prepared sources are shared, and unchanged normalization settings reuse preparation. |
| P10 | Privacy filtering and cheap memory admission precede enqueueing. Diagnostic budgeting and validation run once on the existing storage worker. Memoized immutable records preserve retry identities, bounded flush, overflow accounting, and explicit recovery. |
| P11 | Review caches grapheme boundaries for displayed words. Panning and subsequent frames avoid recounting or rescanning long prefixes. |
| R1 | Unsupported terminal sequences are consumed without closing the input reader. Keypad Begin/Enter and modifyOtherKeys are supported; malformed associated text still reports integrity errors. |
| R2 | Every TTY read follows readiness polling, including reads after full buffers. Partial sequences remain interruptible by signals, wakeups and deadlines. |
| R3 | Clack disables legacy mouse parsing when mouse capture is unused. A stray private-reply prefix preserves the following nonnumeric key. |
| R4 | Unmatched terminal replies are coalesced by type, and reply floods yield to input-owner controls. |
| R5 | An unwind guard closes and wakes the input queue if its reader panics. Terminal coordinates use checked arithmetic. |
| R6 | Left unchanged: a bounded query cannot cancel an arbitrarily delayed terminal reply. The protocol limitation is documented in the terminal fork notes. |
| R7 | File sources must be regular files. Unix nonblocking opens and descriptor checks prevent a FIFO replacement race from blocking the UI. |
| R8 | Language listing ignores unrelated directories without both pack files, while preserving diagnostics for malformed complete packs. |
| R9 | A bounded change journal catches layout changes even when the caret returns to the same word between frames. Appended lookahead also supplies the former final word's separator. |
| S1 | SQLite value/row size is limited, size guards count bytes through BLOB casts, and required schema objects must be actual tables. |
| S2 | Distributed binaries remap home, dependency, repository and generated-source paths. Source/native archives and crates.io packages exclude local measurement files. The remaining private path in public documentation was scrubbed. |

## Behaviors intentionally retained

Imported packs are mutable local files. Each fresh process validates their
contents before using them; session reuse fixes repeated settings freezes without
introducing an unvalidated persistent cache. Large imported packs therefore still
have a validation cost on their first load.

A custom-mode configuration without a file is valid because interactive runs can
receive `--text` or `--stdin`. Its current history profile cannot be inferred
without source content and its hash. The query now explains how to supply a source
or use `--profile all` or an explicit profile key.

Terminal status-query protocols have no cancellation acknowledgement. A grace
period after the probe timeout would only move the window for late replies, so no
speculative timeout extension was added.

## Regression coverage

New tests cover lazy memory use; prepared ASCII input and indentation; large
combining-character payloads; incremental layout versus fresh layout after edits,
resizing, backtracking and source extension; cached long-word review; export byte
equivalence; profile identity; bundled importer equivalence; custom-source memory
and reuse; FIFOs and language folders; classification query plans and legacy index
installation; aggregate equivalence; crafted database bounds; deferred preparation,
privacy, admission limits, retries and recovery; unknown input sequences, terminal
prefixes, partial-read wakeups, deadlines and reader unwinding.

The final all-target/all-feature Rust run passed 291 tests; one storage benchmark
is intentionally ignored. Clippy with warnings denied, formatting, and the Rust
1.88 minimum-compiler check passed. The simulated-hour regression covers 36,000
scored ASCII units, zero reducer allocations, peak engine allocations below
16 MiB, and complete allocation release afterward.

All 27 terminal lifecycle cases and all 28 product workflow cases passed. Two
workflow timeouts during concurrent compilation passed when repeated without
compiler activity; the complete quiet workflow run also passed. The missing-file
workflow initially exposed changed error wording, which was restored and verified.
Release/installer tests passed with one native-Windows-only skip. The final native
and source archives passed verification, shared the current code identity, and
contained neither local home paths nor measurement files.

The subsequent merge review found that registry package verification still
selected the published terminal fork without the new mouse-parser API. The
prepared release now uses application 1.0.1, terminal fork 0.30.1, and backend
0.2.1, with matching manifests, lockfiles and license inventory. All four
normalized Cargo packages are built together by `scripts/verify_crates.py`,
using Cargo's temporary registry before publication. CI also runs the TTY and
MIO parser unit suites. A source push does not publish these versions to crates.io.

Release tests also cover source build inputs, path-remapping flags, measurement
exclusion, archive integrity and installer behavior. Native PTY checks and compute
measurements apply to the current macOS ARM host; they do not establish behavior
on other operating systems or physical terminal emulators.

## Release measurements

These are warm, shared-host measurements, not a controlled before/after replay of
the review's original fixtures. Memory/startup used three sequential isolated PTY
launches against the final packaged binary; frame timings used 500 samples after
100 warmups. An earlier default-launch 363 ms startup outlier remains in the
initial raw results rather than being discarded.

| Measurement | Observed result |
| --- | --- |
| `--words 10000` Ready RSS | 7.95 MiB median |
| Synthetic 1 MiB prose / short words / code Ready RSS | 40.61 / 93.27 / 41.38 MiB median |
| Large-source startup / restart | 77–83 / 24–37 ms median; preparation remains proportional to content |
| 1 MiB extra-letter/backspace full-frame work | 26.0 µs median; 26.9 µs p99 |
| Width-changing edits after 60K units in a long token | 24.5 µs median; 25.3 µs p99 |
| Warm ASCII reducer, 50K events | Zero allocations and allocated bytes; preparation excluded |
| Partial-escape SIGTERM in the final release app | 8.1–8.8 ms exit, terminal restored |
| One-second test after incomplete escape input | 1.021 s observed completion; exactly 1,000,000 µs scored |
| JSONL export, 1,020 results / 19,788,163 bytes | 154 ms file / 207 ms redirected stdout; identical bytes |

On a copied 100,980-result database whose current profile contains 72,171 results,
seven query samples after two warmups produced these medians. Index installation
changed neither output nor schema/export version.

| Current-profile query | Before composite indexes | After |
| --- | ---: | ---: |
| Practice statistics | 128.7 ms | 14.6 ms |
| Paste statistics | 49.0 ms | 8.6 ms |
| Assisted-code history | 77.3 ms | 3.4 ms |

Unfiltered current and all-profile cached statistics both took about 3.6 ms.
Production query plans are asserted using the bundled SQLite in Rust tests;
the supplementary timing fixture's planner capture used Python SQLite 3.45.3.
