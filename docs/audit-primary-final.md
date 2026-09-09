# Primary integration and acceptance audit

The primary agent reread all requirements in the original specification and its
workspace copy, preserving the 312 stable IDs in `coverage.md`. The earlier
performance and terminal reports retain build-input source identity
`a3c58706a5ba1de96232ad4d49ab7f734ffea6d4c8dfbfd26bab2b8cd71a1545`.
The final Windows permissions correction has build identity
`31586c3bc81633d5d3f912b7e717800caf185ecfce5358f7feeb92b3aa8129e3`
and separate validation reports; the earlier measurements are not relabeled. This audit
combines the separate content/storage, terminal/UI, and release/CLI reviews. An
external gate is not closed by successful compilation or a PTY.

The complete workspace suite after Windows permissions integration passes
**253 tests on both Rust 1.95 and MSRV 1.88**, with zero failed tests, across 18
test executables. This includes the independent pace viewport, blind
presentation, and Zen lifetime fixtures. The only ignored test is the
intentionally explicit release-mode 100,000-row query benchmark, whose measured
invocation is tracked independently. The complete transcripts are
`measurements/stage-d-frozen-all-tests.txt` and
`measurements/stage-d-frozen-msrv-all-tests.txt`. Both commands use
`--locked --offline --workspace --all-targets --all-features`. The checksummed
`measurements/stage-d-frozen-rust-validation.json` also records warning-free
native all-target/all-feature Clippy and workspace formatting checks. Initial sandbox attempts
failed the two controlling-terminal fixtures with `/dev/tty` permission errors;
their separate `*-sandbox-attempt.txt` logs are preserved. The approved isolated
PTY reruns pass the complete suites. Windows-only tests compile separately and
are not included in these macOS pass counts.

The rebuilt native macOS ARM64 and translated macOS x86-64 applications each pass
27 lifecycle, 25 product and 10 actual tmux/SSH cases, without failures or skips.
The six `{pty,product,transports}-permissions-final{,-rosetta-x86_64}.json`
reports preserve executable hashes and distinguish native ARM64 from Rosetta.
The exact final ARM64 production binary also passes typing and an acknowledged
SQLite save under a process-only IP-network denial profile;
`network-denied-permissions-final.json` records positive and negative TCP/UDP
controls while Unix IPC remains available. This is not packet capture.

The final audit corrected late-overload Results after the save acknowledgement,
compact unsaved/identity visibility, current long-token review navigation,
NFC-expanded legal input retention, large practice target memory, independent
history classification filters, fixed-width live speed overflow, and explicit
4 Hz statistics/10 Hz pace presentation clocks. All use the original engine,
settings registry, and worker ownership boundaries.

## Stage gates and integration ownership

| ID | Evidence and remaining boundary |
|---|---|
| GATE-A | Immutable effective specs, 61 kernel acceptance/property fixtures, pinned deterministic generation, six allocation/bounds tests, and Unicode tests pass. The reducer has no terminal, file, database or real-clock imports; no-sleep replay tests inject receipt microseconds. |
| GATE-B | Source plus final first-key/no-jump/restart/cleanup PTY cases pass. The original pre-storage warm/latency/idle/stress measurements remain preserved separately. Required actual graphical-emulator validation is external. |
| GATE-C | All modes, searchable registry settings, presets/bindings, themes, rules, practice, current/historical review, exact-ratio PBs, filters, recovery and exports are integrated. Both architectures pass all25 product workflows; 20 settings and23 CLI fixtures use actual configuration/SQLite paths. |
| GATE-D | Five portable release binaries and installable packages, complete source, licensing, recovery/privacy/stress tests, native/translated PTY, real tmux/SSH and final integrated performance have local evidence. Actual archive/installer verification passes. Required native OS/emulator, genuine cold and controlled-reference checks remain external. |
| GATE-FINAL | All312 specification requirements were individually audited. All available local tests, target checks, measurements, dependency review and six-archive/installer verification pass; implementation and lockfile are committed at d9284e80f46836efc48365b7fe3a3d038d7e771f. Final collection sidecars record artifact byte verification. The35 outstanding external validation rows remain explicit; this is not whole-specification completion. |

## Pace and detailed review

| ID | Evidence and remaining boundary |
|---|---|
| PACE-001 | `LiveCache` samples immutable fixed/PB pace using floor(WPM×5×elapsed/60), independent of scoring. The live-cache fixture checks exact logical units at100ms checkpoints; product personal-best pace tests resolve only an indexed matching unpaced best and do not mutate a Running sample on a late reply. |
| PACE-002 | `pace_acceptance` verifies ghost position, user-caret overlap precedence, viewport-only visibility, no ghost-driven scrolling, and an unchanged result snapshot. Rapid-frame cache and real-buffer tests enforce at most10Hz pace and4Hz live statistics despite faster text frames. Assistance identity/PB exclusion have independent kernel/storage fixtures. |
| REVIEW-001 | Summary/text tabs expose WPM/raw series, attempt/error buckets, consistency, final positional counts, deletion/attempt totals, original tokens and entered differences. Product snapshots and workflow tests exercise both tabs; long current-token End/pan and verified historical-source scrolling reach the actual tail without disk persistence. |
| REVIEW-002 | Review labels attempted mistakes separately from final incorrect/extra/missed output. Repaired-cat, omitted-token and Zen fixtures assert these distinctions. History uses bounded sparklines; full chart/review content appears only after Details. |
| REVIEW-003 | Current review/repeat use the retained immutable engine/sample. Historical custom review verifies supplied source hash and distinguishes missing entered text from retained or completely replayable opt-in traces; partial traces cannot invent a diff. Tests cover default/private redaction, NFC-expanded originals and long source navigation. |

## Explicit acceptance assertions

| ID | Evidence and remaining boundary |
|---|---|
| AT-001 | Kernel first-wrong-character fixture and native/translated `first_key_focus_json`/`first_input_without_ready_wait`: first eligible input starts and appears exactly once. |
| AT-002 | First-key PTY and buffer assertions compare the Ready/Running text anchor, wrapping and cursor. Full-focus/mouse tests prove hidden controls do not return. |
| AT-003 | Published corrected-cat golden asserts5 attempts,4 correct attempts,1 deletion,4 retained,4 credited,80% accuracy and24 WPM/raw at2 seconds. |
| AT-004 | Early-space and separator fixtures preserve missed counts without invented typed output. Empty-space/Ready observer tests prove no skipped token, timer start or redraw. |
| AT-005 | Active-prefix fixtures verify entered-only credit for a wholly correct prefix and zero whole-token credit for a wrong/extra prefix, live and at the timed endpoint. |
| AT-006 | Final correct word completes without trailing Space; a wrong final word stays repairable until permitted explicit submission. Both pure engine and real completion workflows pass. |
| AT-007 | Exact/code Tab/newline and confirm-to-finish fixtures plus native/translated `exact_confirm` preserve literal units and final-tail correction. Early F5 is incomplete practice. |
| AT-008 | Injected29.999s/30.000s deadline test, ordered reader watermarks and delayed90s finalization prove exclusive receipt semantics and exactly30s duration. PTY deadline tests verify adapter wiring without claiming sleep-based receipt precision. |
| AT-009 | Both final lifecycle reports pass `restart_epoch` and `repeat_practice`: stale letters/releases/Enter cannot cross epochs; repeated sample has reset counters and visible practice identity. |
| AT-010 | Full/mistakes/current/none correction and arbitrary-edit properties reverse retained contributions once while preserving prior wrong attempts. |
| AT-011 | Independent Master/Expert/stop-rule fixtures and challenge-failed once-mode PTY cases pass; conflicting configurations fail before raw mode. |
| AT-012 | Composed/decomposed/reordered combining/ZWJ/RI/whole-associated-grapheme fixtures plus wide-cell/grapheme-deletion UI and transport tests pass. Actual font/IME appearance is external and not inferred. |
| AT-013 | Native/translated application and both transports reject paste atomically: Ready stays unstarted; active result gets practice integrity without pasted attempts. |
| AT-014 | Normal resize preserves logical input/time; unsafe resize and supported suspend paths interrupt and restore instead of pausing. Actual machine sleep and foreign native OS behavior remain external. |
| AT-015 | Actual SQLite FULL/read-only/locked/corrupt/transaction-rollback fixtures and app locked-save/retry/pending-recovery workflows preserve typing and honest unsaved counts without destructive repair. |
| AT-016 | Private session/preset/trace-lock, custom/Zen redaction, independent opt-ins, default exports and neutral diagnostics are asserted across storage, CLI and final product workflows. |
| AT-017 | Exact profile/category tests isolate active pack/rule/scoring/source/assistance changes while excluding cosmetic/inactive fields. PB comparison uses integer cross-products and waits for commit/indexed comparison. |
| AT-018 | Independent seeded generation and replay goldens pass on Rust1.88/1.95; fixed-source/restart replays pass in both macOS architecture workflows. Required native Linux/Windows replay remains external. |
| AT-019 | Both final macOS lifecycle suites exercise normal exit, error/panic, Ctrl-C, SIGINT/SIGTERM/SIGHUP and supported suspend cleanup, including cursor/protocol state. Rosetta is labeled translation; native other-OS/emulator behavior remains external. |
| AT-020 | Controlling-terminal keyboard/source separation, detached-terminal failure and once-JSON fixtures assert untouched/clean redirected streams and exactly one finite versioned result object. |
| AT-021 | Reader hard4096 capacity, sequence/epoch and closing-overload tests plus healthy/slow stress workloads require ordered input or explicit interruption. Final product test keeps the overload verdict visible after storage ACK. Final production stress measurements are tracked separately. |

## Local release evidence and remaining external gate

Implementation and local validation are distinct from the external release gate.
Native Linux/Windows execution, actual macOS Terminal/Windows Terminal/Kitty
appearance/input, system sleep, genuine cold-cache SSD startup and a dedicated
controlled reference machine are unavailable here. The prior Computer Use refusal
to access Terminal is retained in the terminal audit; it was not bypassed.

All final measurement reports are reconciled. The six-archive collection passes
payload and outer-checksum verification, complete source verification and actual
host installation. The independent archived-installer review passes 12 commands
per macOS architecture, including idempotence, uninstall and preservation of
modified/unregistered files and user history. Foreign installers refuse before
prefix writes; foreign binaries were not executed. The draft attestations are
preserved as `measurements/release-draft-{validation,provenance,independent-review}.json`.
Final collection sidecars record the final archive byte identities outside their
own payloads.

The complete implementation, lockfile and validation evidence are committed at
`d9284e80f46836efc48365b7fe3a3d038d7e771f`. The final ledger has 277 passes and
35 pending external validations, with no pending local requirements. Eight
implementation rows remain Partial specifically because they describe external
terminal/cold/controlled-machine evidence, not omitted application features.
`scripts/audit_coverage.py --require-local-complete` verifies the ledger structure
and individual audit coverage; the named tests and reports provide behavioral
evidence. The full user goal remains open while required external work remains.

The additional staged Git whitespace diagnostic reported one extra blank line at
the end of the frozen helper's Cargo.toml; this cosmetic TOML whitespace was
preserved with the measured input identity. Required Rust formatting and all
supported local Clippy configurations pass. The two historical documentation
links missing from the preserved upstream Crossterm snapshot are identified in
the independent archive report; required licenses and application documentation
are present and their relative destinations resolve.

## Final local follow-up findings

The exact final packaged production benchmark passes all 22 workloads with
ordinary SQLite persistence, including exactly 1,000 saved completion/restart
results. The raw report and scoped validation record 169 application processes,
1,017 total saved results, matching finalized/storage counts, default privacy,
ordered input and restored terminals. The rapid-restart p95 of 10.634 ms is
explicitly separated from ordinary-input latency. The
independent Zen lifetime oracle injects26,207.375s and proves six complete
post-saturation cycles add no live/peak engine allocation, then release everything.
The separate application run completes 3610.014 real seconds, applies all 20480
units, reaches the 4096 editable and 3601 chart caps, saves exactly one result
without private text, and restores the terminal. Its initial burst is followed
by real elapsed chart accumulation; it is not an hour of continuous typing or
an extended post-chart RSS plateau. The original executable identity and the
unchanged-engine relationship to the final Windows correction are documented.
Blind and pace UI fixtures pass on current/MSRV and preserve scoring snapshots.

The initial local source/lockfile commit is36bea17. Windows file creation was found
to inherit potentially broad ACLs in explicit shared directories. The frozen
correction now supplies atomic protected current-user creation, validates
existing SQLite parent/database/sidecar ACLs, and keeps directory guards alive
through connection close. The application uses a safe API; the narrow Win32
adapter is the documented PRIV-005 requirement for an ARCH-004 unsafe exception.
The independent [Windows review](audit-windows-permissions-review.md) has no
unresolved finding. Stable Windows-target Clippy and MSRV compilation pass,
including native fixtures; native Windows ACL execution remains external.
Earlier reports retain their exact original source/executable hashes and do not
claim to test later changes.
