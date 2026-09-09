# Terminal, input, viewport, and interaction audit

Audit date: 2026-09-06 local (2026-09-07 UTC). This is an independent requirement audit against the repository `SPEC.md`, not a declaration that the complete 1.0 goal has passed. The audit separates source inspection, deterministic tests, PTY observations, actual transports, and unavailable native emulators. The central coverage ledger is owned by the primary agent.

The current verification host reports macOS 26.6.2 build 25G83, arm64, Rust 1.95.0. Historical reports retain their original host and binary identities. Current OpenSSH is 10.3p1; tmux is the checksum-verified local 3.7c build. A synthetic xterm-256color PTY cannot prove graphical-emulator appearance, fonts, IME behavior, another operating system, or key-to-photon latency.

After the reviewed Windows permissions correction, all 62 lifecycle/product/transport cases were repeated on each rebuilt macOS application: 124/124 pass, zero failures or skips. The current runtime source freeze is `31586c3bc81633d5d3f912b7e717800caf185ecfce5358f7feeb92b3aa8129e3`. Exact commands, report/log hashes, build manifests and executable identities are in [terminal-permissions-final.json](measurements/terminal-permissions-final.json). The [independent Windows source review](audit-windows-permissions-review.md) records resolved permission findings and leaves native Windows ACL execution external. Earlier final reports below remain preserved evidence for their original binaries.

## Evidence inventory

| Evidence | Observed result | Scope |
|---|---|---|
| `docs/measurements/{pty,product,transports}-permissions-final.json` | 27/27 lifecycle, 25/25 product, 10/10 transport; zero skips | Rebuilt native ARM64 hooks SHA256 `9a4640265a6d863f3ac3107c8d16e5418cdb4a3a55953a292468716ca7e65600`, source freeze31586c3b; immutable manifest in `target/release-permissions/aarch64-apple-darwin-hooks.json`. |
| `docs/measurements/{pty,product,transports}-permissions-final-rosetta-x86_64.json` | 27/27 lifecycle, 25/25 product, 10/10 transport; zero skips | Rebuilt x86-64 hooks SHA256 `ddc715e73af7ebd84c313ab091a43bd902e39978348864ad9dc9c06687517a10`, same source freeze, executed through installed Rosetta; no native Intel claim. |
| `docs/measurements/network-denied-permissions-final.json` | Fresh controls and production workflow passed | Rebuilt native production SHA256 `4e50a20121623d2634fbdbda9aa1bdf8c066cc2e6623f0b071c1e9d9125c5807`; startup, typing, completed JSON, SQLite save and cleanup passed under the unchanged process-local IP-denial profile. IPv4/IPv6 controls and allowed Unix IPC were rerun; no packet capture claim. |
| `tests/kernel_acceptance.rs`, current audit rerun | 61 passed | Pure formulas, correction, immutable completion, exact deadline, difficulty, Unicode payload/cross-event boundaries, determinism properties. |
| `tests/kernel_performance.rs`, final Rust 1.88 all-target/all-feature suite | 6 passed | Zero warmed ASCII reducer allocations, bounded Zen correction, allocation release across runs, generator profile parameters, shared large practice metadata and lazy correction allocation. Log: `stage-d-msrv-all-tests.txt`. |
| `tests/zen_bounds_acceptance.rs`, Rust 1.95 and 1.88 | 1 passed on each compiler | Injected 26,207 seconds plus a 375 ms tail; six 3,701-second cycles after warm saturation. Both retain correct chart/history/text, report zero allocations and zero additional live/peak bytes in every cycle, and release all engine/snapshot allocations. A complete reference transcript is built outside the measured scope. Exact logs: `zen-bounds-current-final.log`, `zen-bounds-msrv-final.log`; source-linked report `zen-bounds-final.json`. |
| `tests/keyboard_protocol.rs`, current audit rerun | 15 passed | Includes 16 terminal lifecycle/reader scenarios and 10 explicit capability-probe scenarios in real isolated Unix PTYs. Helper tests returning without their trigger variable are not additional PTY scenarios. |
| `tests/ui_acceptance.rs`, current audit rerun | 10 passed | Required three geometries, 18 Ready/Running/Results snapshots, geometry bounds, Unicode cells, independent status controls, no focus jump, stable correct-input layout, current review reachability, effective interruption rendering. |
| `tests/ui_product_snapshots.rs`, frozen-source rerun | 16 passed | Six product overlay snapshot files cover palette/editor/review/history/configuration in color and monochrome. New fixtures verify compact interruption plus unsaved status, visible identities for different limits/rules, safe zero/unsafe overlay geometry, fallback color roles, stable live accuracy placement at 120,000 WPM, and 250 ms metric sampling across rapid input frames. Log: `terminal-product-snapshots-final.log`. |
| `docs/measurements/product-integrated-v3.json` | 23/23 passed | Historical integrated workflow suite, immutable V3 binary SHA256 `7a65d9006aaba376a377eef09d0d6ce9a5612f622d3be32a1bc2d96c7f2e5741`. |
| `docs/measurements/product-lifecycle-v3.json` | 25/25 passed | Historical application PTY lifecycle/keyboard/CLI boundaries; same preserved V3 executable. |
| `docs/measurements/product-transports-audit-corrected.json` | 10/10 passed | Actual tmux and authenticated encrypted loopback SSH, five workflows each; preserved V3 binary and current host. |
| `docs/measurements/product-overload-ui-before-fix.json` | Required new regression failed on V3 | Closing-epoch overload is stored as interrupted, but after save ACK the old Results renderer loses its interruption label. The repaired frozen native and Rosetta applications pass the new ACK visibility regression; the earlier failed evidence remains retained. |
| `docs/measurements/pty-final.json`, `product-final.json`, `transports-final.json` | 27/27 lifecycle, 25/25 product, 10/10 transport; zero skips | Prior frozen native arm64 test-hooks binary, SHA256 `08ba507053e733d6b8cc668c32572ad13572aa5192af861eaf242d0f9f535390`; all new UI/input regressions pass on that source revision. |
| `docs/measurements/pty-final-rosetta-x86_64.json` | 27/27 passed, no skips | Frozen x86-64 test-hooks application under Rosetta on arm64, SHA256 `c8d11b7b5b5b56fe64c558d7f1cc9c17123f9c404dc555ed77c6b54d7c09d7ba`; no native Intel hardware claim. |
| `docs/measurements/product-final-rosetta-x86_64.json` | 25/25 passed | Same frozen Rosetta application; includes History classifications, preservation of effective settings, and visible overload verdict after save ACK. |
| `docs/measurements/transports-final-rosetta-x86_64.json` | 10/10 passed | Same Rosetta application through actual native tmux 3.7c and OpenSSH 10.3p1 loopback, synthetic outer PTY. |
| `docs/measurements/network-denied-final.json` | Controls and production workflow passed | Native arm64 production SHA256 `364c3b402cb9bd6a2c4d858ff28af881fc328ac7d9557ec52d343d93c3ae32b2` starts, types, emits a complete JSON result, saves exactly once, and restores the terminal with process-local IP bind/inbound/outbound denial. IPv4/IPv6 TCP/bind/UDP controls return EPERM; Unix socketpair still works. This is not packet capture. |

The seven input-owner queue/clock/epoch unit tests also pass. Current vendored input-backend reruns pass 109 TTY and 108 MIO tests, each with the same 7 pre-existing interactive tests ignored. Minimal-feature, all-feature/all-target, and Windows-target checks finish without warnings; logs are `docs/measurements/terminal-vendor-*-final.log`.

Before the Windows permissions correction, both frozen macOS applications also passed all three complete suites: 27 lifecycle, 25 product and 10 actual transport cases per binary, zero failures or skips. That native arm64 hooks SHA256 is `08ba507053e733d6b8cc668c32572ad13572aa5192af861eaf242d0f9f535390`; manifest `target/benchmarks/stage-d-hooks/build-manifest.json`. Its Rosetta counterpart has manifest `target/release-final/x86_64-apple-darwin-hooks.json`. Fresh reports preserve failed evidence and historical measurements. Every suite checks executable SHA256 before and after its run and records an explicit native/translated execution label separately from the Python harness host architecture.

## Requirement reconciliation

Local pass below means the named code path and automated evidence cover the behavior on this host. It is not native-emulator or cross-platform runtime approval. A source-only conclusion is explicitly labeled; all release/platform gates remain with the primary audit.

The explicit scope contains 100 requirement IDs. `docs/measurements/terminal-audit-coverage-final.json` confirms every scoped ID has one audit row and exists in the central ledger; it checks row coverage, not implementation correctness.

| ID | Inspected implementation and direct evidence | Audit conclusion |
|---|---|---|
| UI-001 | Canonical configuration, shared samples, default Ready rendering; settings tests and actual default startup reports. | Implemented; local evidence exists. |
| UI-002 | Defaults are mistakes correction, timer-only status, three rows, auto width capped at72; geometry/snapshot tests. | Implemented; local evidence exists. |
| UI-003 | Ready directly renders target/hint without a setup modal; first-input-without-Ready-wait PTY case. | Local pass. |
| UI-004 | Reader/engine share eligible start predicate; first wrong event receives start time and appears once; first-key PTY and kernel fixtures. | Local pass. |
| UI-005 | `eligible_text_start`, shared Bindings resolution, keyboard atomic-paste/commands and exact whitespace fixtures. | Local pass. |
| UI-006 | Ready/Running use unchanged Geometry; only configured labels disappear; first-input snapshots and actual target-anchor equality. | Local pass. |
| UI-007 | Default active view has no panels; overlays replace it; full-focus test verifies every outside cell blank; mouse/focus-gained normalized away. | Local pass; actual appearance external. |
| UI-008 | Status and target share `Geometry.text.x`; live speed uses a fixed five-cell overflow marker. Real-buffer fixtures compare low speed with a legal 10,000-unit/one-second result at 120,000 WPM. | Local pass: accuracy starts in the identical column and overflow reads `9999+ wpm`. |
| UI-009 | Word progress uses current completed token/requested spec.words, never generated lookahead count. | Source inspected. |
| UI-010 | Geometry clamps auto72/columns-minus8, centering and near45% placement; zero/small/large geometry cases. | Local pass. |
| UI-011 | Typed preferences and registry validate width40–120/auto, center/top, rows1–5, spacing0/1; geometry tests. | Local pass for accepted settings and geometry. |
| UI-012 | Viewport has logical token/unit positions and whole-row anchor; three rows retain one prior/next row; restart/resize PTY replay checks. | Local pass. |
| UI-013 | One cell descriptor per whole grapheme with cached widths; oversized tokens wrap without inserting scored units; Unicode geometry fixtures. | Local pass for validated repertoire; font/emulator width remains external. |
| UI-014 | Geometry selects one row at40x10 and hides secondary controls; required compact snapshots. | Local pass. |
| UI-015 | Small Ready is disarmed; active unsafe resize interrupts with reason; small_ready_focus_mouse now explicitly verifies ignored unsafe input, safe resize, focus metadata and no mouse restoration (`pty-small-ready-focus-mouse.json`). | Local pass. |
| UI-016 | Typing widget uses whitespace/direct cells, no borders/logo/rings/fonts; default correct role inherits foreground. | Source inspected, snapshots inspected. |
| UI-017 | Theme has background/pending/correct/incorrect/extra/muted/accent/caret/pace roles. | Local fallback-role test passed. |
| UI-018 | Terminal/dark/light/warm/high_contrast themes are distinct source palettes; terminal resets foreground/background. | Implemented/tested roles; actual readability external. |
| UI-019 | TrueColor/Ansi256/Ansi16/Monochrome select compatible values for every role. | New exhaustive fallback-role fixture passed. |
| UI-020 | Pending dim/subdued; correct primary; errors underline or reverse plus error color. | Local semantic/style pass; subjective glyph readability external. |
| UI-021 | Entered wrong/extra characters are drawn from retained input, review compares target and input; monochrome and review fixtures. | Local pass. |
| UI-022 | Token layout revision changes only footprint; sync rebuilds from affected token; ordinary correct input reuses layout. | Local pass. |
| UI-023 | Session applies steady native bar/block/underline, viewport always adds fallback cell styling; no blink loop. | PTY command/fallback pass; native emulator rendering external. |
| UI-024 | No font/palette/title/size/persistent-configuration command is emitted; only reversible session capabilities. | Source audit. |
| UI-025 | NO_COLOR auto detection and explicit color policy; monochrome error/extra/caret roles tested. | Local implementation/style pass; actual terminal appearance external. |
| UI-026 | No smooth scrolling/caret animation; no screen-reader accessibility claim in documentation. | Source/documentation audit. |
| UI-027 | Results render metrics, duration/source and outcome; effective-verdict and compact-warning fixtures cover interruption after save ACK. | Compact unsaved and mode/limit/profile identity fixtures pass; frozen native and Rosetta integration preserves interruption after ACK. |
| UI-028 | Storage ACK alone produces PB/tie notice and only for current ticket/epoch; larger charts are Details-only. | Product PB and locked-save fixtures pass. Both frozen macOS applications pass the closing-epoch ACK regression. |
| UI-029 | Shared bindings route Enter/F2/Ctrl-R; restart clones settings, repeat exact spec/text/generator. | PTY repeat/new-sample/replay pass. |
| UI-030 | Results letters ignored, epoch barrier drains old input, 150ms unmodified Enter guard, Ctrl-R exempt. | Kernel immutability and restart_epoch PTY pass. |
| KEY-001 | Palette creation aborts active test before disarm; search text cannot score. | product palette_compact_abort pass. |
| KEY-002 | Ctrl-R aborts/reset/new sample immediately through shared binding. | restart_epoch pass. |
| KEY-003 | F2 clones exact sample and marks repeated; counters/timing reset. | repeat_practice pass. |
| KEY-004 | F3 missed/slow choices; F4 immutable current/historical review. | Product practice/review/default-choice tests pass. |
| KEY-005 | Shared Finish action maps F5/palette to engine mode-dependent Finish. | Exact/code, incomplete practice and Zen kernel/PTY fixtures pass. |
| KEY-006 | Backspace/Ctrl-W use pure engine permission boundaries; grapheme deletion fixtures. | Kernel correction/property pass. |
| KEY-007 | Ctrl-C handled before associated text; reader stops, bounded storage flush, terminal restore, then unsaved diagnostic. | Lifecycle, pending-across-results and acknowledged-result preservation product pass. |
| KEY-008 | Palette replaces target; bounded at most7 matches and current selected value/help; compact scrolling tested. | Product snapshots/selection tests pass. |
| KEY-009 | Palette built from REGISTRY and CommandAction::ALL plus themes/packs/presets. | Exhaustive registry plus product action workflows; no separate scoring implementation. |
| KEY-010 | Arrows/Enter/Esc and theme preview outside Running; failed edits remain editable and disk unchanged. | Theme preview, missing-source, invalid-setting/config, presets product pass. |
| KEY-011 | Shared Bindings rejects printable/Tab/Enter and ambiguous Ctrl aliases; reader and app use same Arc. | Keyboard/configuration tests and dynamic_binding_reader_timer pass. |
| TIME-001 | Reader receipt uses origin.elapsed Instant; engine receives integer time; wall clock is diagnostic only. | Source and deterministic clock-origin properties. |
| TIME-002 | Reader interrupts backward time, >3s active observation gap or >1s wall divergence; SIGTSTP restoration/CONT reactivation. | Clock fixture and SIGTSTP PTY pass; actual system sleep external. |
| TIME-003 | One reader stamps after read, sets first eligible receipt once; ordered watermark precedes late event. | Reader source/ordering fixture and kernel exact-boundary pass. |
| TIME-004 | Exactly-at30s input excluded; delayed90s tick still produces30s result. | deadline_is_exclusive_and_late_finalization_keeps_configured_duration pass. |
| TIME-005 | No paused score state; focus metadata, normal resize, overlay abort, slow output retain/invalidate honestly. | Resize, palette abort and slow-consumer evidence; source inspected. |
| TIME-006 | Error/signal/clock paths interrupt and exclude eligibility; recoverable panic restores session. | Local lifecycle/clock/invalid input pass; machine sleep/other OS external. |
| INPUT-001 | Press/Repeat once, Release ignored, associated text replaces code; bounded scalar payload validation. | Keyboard API/parser/PTY tests pass. |
| INPUT-002 | No layout/physical repeat/left-right Shift inference; baseline limitations documented in terminal matrix. | Source/documentation audit. |
| INPUT-003 | Paste event normalized as a marker, no payload/scored characters, Ready untouched/active practice. | Kernel and paste_atomic PTY pass; both transport paste cases pass. |
| INPUT-004 | Unbracketed paste/macros and non-secure competition limits documented. | Documentation complete. |
| UNI-001 | Unit graphemes separate from UTF8/codepoint/cell positions; cached widths, NFC prose and literal exact. | Kernel and UI combining/wide fixtures pass. |
| UNI-002 | Exact normalization is explicit effective spec/profile field. | Kernel policy/profile fixtures pass. |
| UNI-003 | Pending cluster spans events; extension revises one attempt, finalized on boundary/delete/finish. | Composed/decomposed and incomplete-cluster tests pass. |
| UNI-004 | Whole associated grapheme known before auto-completion/difficulty; scalar prefixes remain provisional. | New whole-payload and regional-indicator cross-event fixtures pass. |
| UNI-005 | Metrics use grapheme counts, not byte/cell/physical key counts; profile includes policy/language/scoring. | Kernel formula and Unicode fixtures; content docs. |
| UNI-006 | Accent/combining/double-width/ZWJ/RI fixtures exist without claiming universal scripts. | Automated pass; real font/IME/emulator appearance external. |
| ARCH-002 | Ratatui custom TypingWidget/Viewport; no custom terminal escape renderer. | Source audit and real app PTY output. |
| ARCH-005 | Pure reducer has no IO/SQL/clock; injected timestamp replay needs no sleeps. | Kernel/property test pass. |
| ARCH-006 | UI borrows &Engine/&ResultSnapshot, mutates presentation buffer/cache only. | Source audit; immutable snapshot renderer fixture. |
| ARCH-007 | Target/entered Units, token contributions and stable logical cursor; colors are styles, not engine strings. | Source audit and edit/layout fixtures. |
| ARCH-008 | One Reader worker, app owns engine/render, lazy storage starts after first usable frame. | Source audited; product integration and separate storage audit. |
| ARCH-009 | Only InputOwner calls Crossterm read/poll during app; explicit doctor probe runs without a Reader. | Source search and exclusive-owner PTY scenarios pass. |
| ARCH-010 | Envelope epoch/sequence/receipt plus bounded queue; storage only receives immutable records and wake channel. | Source audit, queue/order tests; storage audit owns write-boundary completion. |
| ARCH-011 | Barrier clears stale text/parser bytes and retains global signal/resize/closure events. | Reader unit and restart_epoch PTY pass. |
| ARCH-012 | Ordered deadline watermarks; results provisional until closure overload verdict. | Reader queue/watermark/closure tests, overload_after_completion PTY and ACK rendering regression pass on both frozen macOS applications. |
| ARCH-013 | INPUT_CAPACITY4096, sticky overload, lifecycle packets preserved with explicit invalidation. | Exact-capacity unit and post-completion overload PTY pass. |
| ARCH-014 | Only resize (and OS-coalescing identical signals) coalesced; text never merged; bounded command/check loop. | Resize-order fixture and healthy/slow input stress evidence. |
| ARCH-015 | Main parks with unpark token; no game loop; workers wake only due work. | Source plus60s Ready/Results idle reports; final performance owner remeasures release. |
| ARCH-016 | Engine batch at most64/1ms; event timestamps/sequence remain authoritative. | Source audit and stress report. |
| ARCH-017 | Dirty frame cap8334us, input applied before draw throttle, coherent Terminal.draw. | Source and latency/frame reports; final performance audit separate. |
| ARCH-018 | 1Hz progress,4Hz metrics,10Hz pace, no default Ready/Results periodic redraw; independent reader deadline. | Source interval tests, real-buffer 250 ms metric cache across rapid input frames, hidden/full-focus fixtures and idle reports. Ignored Space/Backspace/F5 in Ready produce no frames or terminal bytes on both frozen macOS applications. |
| ARCH-019 | Full Ratatui frame, cell diff, no per-key clear; suspend resizes viewport without DSR/global stdout. | First-key/redirected/suspend PTY and layout fixture pass. |
| ARCH-020 | Cached Unit width/token footprint/line cells, visible-window placement and affected-suffix rebuild. | Source and correct-input cache-reuse tests. |
| ARCH-021 | Synchronized output not used or required. | Source audit. |
| ARCH-022 | Terminal normalization/reducer do no disk/SQL/network/full transcript diff; expensive setting/storage/preparation work deferred outside bounded ordinary batch. | Source audit; primary owns final integration/performance audit. |
| ARCH-023 | Ordinary warmed ASCII path zero allocations; incremental metrics. | Allocation fixture pass; release timing evidence owned by performance audit. |
| ARCH-024 | Finite review bounds, Zen 4096-unit editable window, bounded opt-in trace and samples. | New independent lifetime fixture cycles both editable text and 3,601 samples six times after saturation, compares against a full-transcript oracle, verifies extra deletions stop at the immutable prefix, and measures flat live allocation. Real full-application long-run evidence remains separately recorded by the primary audit. |
| TERM-001 | Five native-format binaries/packaging handled by release owner. | Cross-build artifacts cannot prove native runtime; external execution remains. |
| TERM-002 | Common actual emulator per Linux/macOS/Windows. | Pending external; no substitution from PTY. |
| TERM-003 | Matrix includes macOS Terminal, Windows Terminal, Kitty, baseline, tmux and SSH. | Actual tmux/loopback SSH passed; graphical/native matrix pending external. |
| TERM-004 | SSH latency excluded from local responsiveness budget in matrix/transport report. | Documentation complete. |
| TERM-005 | Session tracks raw/alt/cursor/style/paste/focus/enhancements, including attempted partial writes. | Lifecycle modes and enhancement mutation PTY checks pass. |
| TERM-006 | Guards restore all attempted capabilities on normal/error/panic/signals; suspend restores before SIGSTOP. | macOS PTY pass; actual emulators/native systems external. |
| TERM-007 | Signal-hook self-pipe prepared before raw; handler performs no application cleanup. | Source audit, signals and setup protection PTY scenarios. |
| TERM-008 | Catch-unwind prints after guards; prior style not queried on ordinary startup, reset user default. | Panic/error cleanup and cursor protocol PTY pass. |
| TERM-009 | Reader joined before terminal restored; storage worker never UI-writes; bounded shutdown. | Source audit and lifecycle output assertions. |
| TERM-010 | SIGKILL/emulator failure/power-loss limitation documented. | Documentation complete. |
| TERM-011 | No mouse capture; baseline keys, fallback roles, styled caret and ASCII markers. | Local code/style tests; actual capability/glyph fallback external. |
| TERM-012 | Geometry checked before draw/start, safe small buffers, explicit unsafe-resize interruption. |0x0–200x60 UI bounds and small-resize PTY pass. |
| TEST-001 |61 pure reducer/golden/property tests are terminal independent. | Current rerun passed. |
| TEST-002 | Arbitrary edits verify counts, retained reversibility, bounds, finite metrics and injected-clock replay. | Current properties passed. |
| TEST-003 |18 base snapshots and6 product overlay matrices at all3 sizes in color/mono. | All matrices and new compact result warning/identity fixtures pass; six Results goldens refreshed and inspected, log terminal-results-goldens-final.log. |
| TEST-004 | 27 lifecycle app cases, 25 product workflows, 26 internal keyboard/probe PTY scenarios, and 10 actual transport cases execute real binaries. | Both frozen macOS applications pass all suites without skips; x86-64 runs under Rosetta and arm64 runs natively. |
| TEST-005 | Manual actual emulator appearance/input verification. | Pending external. |
| SHIP-006 | Explicit evidence/limitations and manual protocol in terminal-matrix.md. | Artifact complete; native emulator evidence still external. |
| DOD-001 | First key counts and no focus/restart/resize layout jump. | Automated local pass; pleasant real-terminal experience external. |
| DOD-003 | Registry-searchable settings and mostly text/whitespace default active view. | Automated source/snapshot/product pass; real appearance assessment external. |
| REC-005 | History editor exposes arrow-selectable standard/practice/paste-attempted/assisted-code examples and renders active classifications on reserved row 3. | Frozen native and Rosetta product fixtures each create five actual saved results, check each classification and overlapping outcome filters, verify aggregate counts and empty state, and prove navigation adds no typing scores or database rows. Storage audit owns the complete classification/index review. |
| PERF-011 | Production app executes a complete persisted run with process-local IP networking denied while required Unix IPC is permitted. | Local denied-network operation passed with positive/negative controls; static dependency/source audit remains the evidence about absence of runtime network clients. No packet-capture or syscall-trace claim. |

## Explicit acceptance-test crosswalk

AT-001/002: first-key/no-Ready-wait and focus-anchor PTY fixtures. AT-003–007: published correction/submission/prefix/final/exact kernel golden cases, plus exact_confirm actual PTY. AT-008: deterministic29.999s/30.000s boundary and delayed finalization fixture, ordered reader ticks, actual fixed-duration PTY (the latter is not a receipt-boundary measurement). AT-009: restart_epoch and repeat_practice PTY. AT-010/011: reversible backtracking and Master/Expert/stop-policy kernel fixtures. AT-012: composed/decomposed/whole-payload/RI/ZWJ/grapheme deletion plus width/caret UI and transport fixtures; real glyph/IME appearance remains external. AT-013: paste_atomic and both transport paste cases. AT-014: normal and unsafe resize plus SIGTSTP/CONT PTY; actual system sleep external. AT-015/016: product locked save/retry/pending set/private/historical redaction cases are local evidence; storage owner's full failure/privacy audit remains authoritative. AT-017: profile version/cosmetic/PB fixtures and paced eligibility; storage audit covers exact indexed ratios. AT-018: deterministic content/kernel goldens pass locally, cross-platform native replay is external. AT-019: normal/error/panic/SIGINT/SIGTERM/SIGHUP/SIGTSTP lifecycle tested in macOS PTY; native emulators external. AT-020: independent controlling keyboard/stdin source and exactly-one JSON stdout assertions. AT-021: hard4096 queue, explicit closure overload, stress bounds and order; the new visible-verdict-after-ACK regression passes on both frozen macOS binaries.

## Findings and closure

1. **Overload result presentation, fixed:** an engine result is immutable after completion, but an input-epoch closure can still declare overload. V3 correctly exports/stores the effective interruption and then incorrectly redraws the raw completed engine result after save ACK. The new product case preserves that failed evidence; both frozen macOS applications now keep the effective interruption visible after ACK and in Details.
2. **Compact unsaved status, fixed:** the old 40x10 Result line clipped a long interruption reason before `unsaved`. The new two-row alert layout passes explicit outcome+unsaved assertions at all sizes. Mode/limit and profile identity are visible and distinguish effective rule/limit changes. All six Results goldens were refreshed and inspected.
3. **Transient lint warning, fixed:** the old review path left unused `visible_text`; the current product snapshot and Results-golden builds emit no warnings. Final release lint remains the primary agent's separate gate.
4. **Transport harness corrections:** initial SSH authorization beneath world-writable `/private/tmp` failed StrictModes before clack; fixed by private workspace credentials. tmux's outer terminfo reset2 differs from clack's inner reset0; both independently checked. Corrected10/10 report retained, earlier failures not erased.
5. **Ignored Ready redraws, fixed:** the observer fixture reproduces unnecessary frames on V3 after leading Space, Backspace and F5 (`pty-ignored-ready-before-fix.json`). Both frozen macOS applications pass the new zero-frame/zero-byte/zero-score assertion and still count the following eligible key once.
6. **External proof remains unavailable:** no native Linux/Windows execution, native Intel hardware, graphical Terminal/Windows Terminal/Kitty appearance, actual system sleep, remote OS/SSH latency, or screen-reader/IME assessment is inferred from this audit. Rosetta x86-64 application execution is completed evidence and explicitly labeled translation. The earlier explicit CUA refusal to access macOS Terminal must not be bypassed.

The final audit found no remaining local implementation defect in the terminal/input/UI requirements above. Source-only and external conclusions remain explicitly scoped. Actual graphical emulators, native Linux/Windows and Intel hardware, and real system sleep are still release-validation gaps; synthetic PTY and Rosetta passes do not close them.
