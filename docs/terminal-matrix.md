# Terminal validation matrix

This is the evidence ledger for SPEC §14–16. A successful compilation, reducer
test, in-memory screenshot, PTY run, or harness self-test does not establish actual
emulator compatibility. Pending means not executed; it is not a pass.

## Current evidence

Rows ending in `PERMISSIONS-FINAL` describe the latest source freeze
`31586c3bc81633d5d3f912b7e717800caf185ecfce5358f7feeb92b3aa8129e3`.
Earlier `FINAL` rows retain the preceding frozen binaries and reports.
[The current aggregate](measurements/terminal-permissions-final.json) verifies
124/124 case rows and exact report/build/executable hashes. Windows source review
is [recorded separately](audit-windows-permissions-review.md); its native runtime
validation remains external.

| ID | Surface and environment | Status | Actual evidence | Limits |
|---|---|---|---|---|
| PTY-CLACK-MAC-ARM64-PERMISSIONS-FINAL | Rebuilt native ARM64 hooks, macOS 26.6.2 / 25G83, synthetic outer PTY; actual tmux 3.7c and OpenSSH 10.3p1 loopback | Pass, 62/62 | `{pty,product,transports}-permissions-final.json`: 27 lifecycle, 25 product, 10 actual transport; SHA256 `9a4640265a6d863f3ac3107c8d16e5418cdb4a3a55953a292468716ca7e65600`. | No graphical terminal, native foreign OS, physical display or remote-latency claim. |
| PTY-CLACK-MAC-X64-ROSETTA-PERMISSIONS-FINAL | Rebuilt x86-64 hooks under Rosetta on the same ARM64 host and actual native transports | Pass, 62/62 | `{pty,product,transports}-permissions-final-rosetta-x86_64.json`: 27 lifecycle, 25 product, 10 actual transport; SHA256 `ddc715e73af7ebd84c313ab091a43bd902e39978348864ad9dc9c06687517a10`. | Translated macOS application execution; native Intel hardware and graphical terminal remain external. |
| NETWORK-DENIED-MAC-ARM64-PERMISSIONS-FINAL | Rebuilt native ARM64 production, process-local IP denial and synthetic PTY | Pass | `network-denied-permissions-final.json`, SHA256 `4e50a20121623d2634fbdbda9aa1bdf8c066cc2e6623f0b071c1e9d9125c5807`; fresh controls and startup/type/result/SQLite save/cleanup pass. | Denied-network operation, not packet capture; Unix IPC permitted and no system policy changed. |
| PTY-HARNESS-MAC | Python standard-library PTY helper on local macOS arm64 | Pass | `python3 scripts/pty_test.py --self-test`, exit 0. Verified controlling-terminal input distinct from piped stdin; stdout/stderr separation; split UTF-8 and wide-cell decoding; cursor/alternate/paste state tracking; original terminal configuration restored. | Artificial fixture, not clack or an emulator. |
| PTY-CLACK-MAC | Real clack executable with explicit test-hooks, synthetic xterm-256color PTY, macOS arm64 | Pass, 22/22 extended cases | `docs/measurements/pty-epoch-instrumented.json`, isolated `target/pty/clack-epoch-instrumented`, SHA256 `9e8c942c49be581f726ad768e603f7f08d8bb174a73d023977c71dccd2f909b0`. Includes closing-epoch overload after provisional completion, 11 pre-raw source launches, associated-text Ctrl-C, random-restart layout replay and suspend restoration/resume interruption. Initial failure and repaired 21-case reports remain retained. | Instrumented correctness, not production timing or actual-emulator compatibility. |
| PTY-PRODUCTION-MAC | Pre-storage production release, synthetic xterm-256color PTY, macOS arm64 | Measured, all 22 workloads without failures | `target/benchmarks/stage-b/full.json`: three geometries, warm startup, ASCII/mixed receipt-to-flush, 150WPM CPU, healthy/slow 1000-event/s delivery, 60-second Ready/Results idle, 1000 completed tests, bounded short Zen. | Private/no-storage development baseline; final storage-enabled release remeasurement, real emulators and controlled reference remain separate. |
| PTY-CLACK-LINUX-X64 | Real clack executable, Unix PTY, Linux x86-64 | Pending external | — | Current macOS run cannot establish Linux PTY behavior. |
| PTY-CLACK-LINUX-ARM64 | Real clack executable, Unix PTY, Linux arm64 | Pending external | — | Native execution required. |
| CONSOLE-CLACK-WINDOWS | Real clack executable and native Windows console channels, x86-64 | Pending external | — | Unix PTY script does not validate Windows CONIN$/CONOUT$ or backend behavior. |
| KERNEL-BOUNDARIES | Injected receipt-time reducer and Unicode fixtures, macOS arm64 | Pass | Coverage VA-004: 57 kernel acceptance/property fixtures, including exact deadline exclusion and late finalization. | Receipt semantics in pure reducer; reader and actual terminal evidence remain distinct. |
| PTY-PRODUCT-V3 | Preserved integrated test-hooks binary, macOS arm64 | Pass, 23/23 workflows and 25/25 lifecycle cases | `docs/measurements/product-integrated-v3.json` and `product-lifecycle-v3.json`; binary SHA256 `7a65d9006aaba376a377eef09d0d6ce9a5612f622d3be32a1bc2d96c7f2e5741`. | Historical integration evidence. The later overload-after-save rendering regression is retained separately in `product-overload-ui-before-fix.json`; the frozen native/Rosetta application reports below verify the repair. |
| TRANSPORT-TMUX-MAC | Actual tmux 3.7c, macOS 26.6.2 / 25G83 arm64, synthetic outer xterm-256color PTY | Pass, 5/5 | `docs/measurements/product-transports-audit-corrected.json`, preserved V3 hash above: completion, restart/resize, exact Unicode/Tab/newline, paste integrity, Ctrl-C; both inner and outer lifecycle checked. | Actual multiplexer execution; no graphical outer-emulator appearance claim. |
| TRANSPORT-SSH-MAC | Actual OpenSSH 10.3p1 encrypted/authenticated loopback session, same local OS and synthetic outer PTY | Pass, 5/5 | Same corrected transport report; remote exit code, independent JSON stream, literal Unicode input, restart/resize, paste and lifecycle checked. | Local loopback, not a different remote OS, remote network latency, or graphical emulator. |
| PTY-CLACK-MAC-ARM64-FINAL | Frozen native arm64 test-hooks application, macOS 26.6.2 / 25G83, synthetic PTY | Pass, 27/27 lifecycle and 25/25 product | `docs/measurements/pty-final.json`, `product-final.json`; SHA256 `08ba507053e733d6b8cc668c32572ad13572aa5192af861eaf242d0f9f535390`. All new History, live-state, ignored Ready and overload ACK checks pass. | Local native application evidence; graphical terminal and physical display remain external. |
| TRANSPORT-MAC-ARM64-FINAL | Same frozen native application through actual tmux 3.7c and OpenSSH 10.3p1 loopback | Pass, 10/10 | `docs/measurements/transports-final.json`; exact Unicode/Tab/newline, restart/resize, paste and independent inner/outer cleanup. | Synthetic outer terminal and same-host encrypted loopback; no remote OS or latency claim. |
| PTY-CLACK-MAC-X64-ROSETTA-FINAL | Frozen x86-64 test-hooks application translated by Rosetta on macOS 26.6.2 / 25G83 arm64 | Pass, 27/27 lifecycle and 25/25 product | `docs/measurements/pty-final-rosetta-x86_64.json`, `product-final-rosetta-x86_64.json`; SHA256 `c8d11b7b5b5b56fe64c558d7f1cc9c17123f9c404dc555ed77c6b54d7c09d7ba`. New checks cover History classifications, compact/result integrity, ignored Ready actions and visible overload after save ACK. | Translated application execution, not native Intel hardware or an actual graphical terminal. |
| TRANSPORT-MAC-X64-ROSETTA-FINAL | Same translated x86-64 application through actual native tmux 3.7c and OpenSSH 10.3p1 loopback | Pass, 10/10 | `docs/measurements/transports-final-rosetta-x86_64.json`; five workflows per transport, inner terminal attributes and outer modes checked. | Synthetic outer terminal; SSH peer runs on the same host. |
| NETWORK-DENIED-MAC-ARM64-FINAL | Frozen native arm64 production executable, process-local IP-denial sandbox, synthetic PTY | Pass | `docs/measurements/network-denied-final.json`; SHA256 `364c3b402cb9bd6a2c4d858ff28af881fc328ac7d9557ec52d343d93c3ae32b2`; startup/type/result/SQLite save/terminal cleanup pass. Positive IPv4/IPv6 loopback and negative TCP/bind/UDP controls verify the profile; Unix socketpair passes. | Operation while IP networking is denied, not packet capture or evidence about every attempted syscall. No system policy modified. |

The local task sandbox denies a child's `/dev/tty` open with EPERM. The successful
PTY helper self-test used the approved isolated-terminal escalation. This was a
tool sandbox restriction, not a clack compatibility result.

macOS revokes the PTY when its controlling-session leader exits. The helper uses
an external session keeper, then forks clack into its own foreground process group.
The keeper retains the terminal until post-exit cleanup assertions finish. The
reported child PID is clack's PID. `started_ns` is an external monotonic timestamp
immediately before the clack fork; Python keeper startup is setup work and is not
reported as clack startup. The app's terminal initialization remains inside the
measured interval.

Terminal-attribute checks compare all configuration fields and control characters,
masking only `PENDIN`. Darwin may set that kernel-maintained pending-line flag when
canonical mode is restored; it is not a user configuration preference. Echo,
canonical mode, signal handling, input/output flags, speed, and control characters
are still checked. The helper does not change the user's own shell terminal.

## Required actual-emulator matrix

Fill in exact OS build, CPU/architecture, terminal version, local/remote context,
geometry, color policy, input protocol, build identifier, date, and evidence path
for every executed row. The names below are planned test surfaces, not a claim
that these applications or machines are installed.

| ID | OS / architecture | Actual terminal or path | Input coverage | Status | Evidence |
|---|---|---|---|---|---|
| EMU-MAC-TERMINAL-ARM64 | macOS arm64 | macOS Terminal | Baseline keys, native caret, redirected channels | Pending external, tool policy unavailable | Primary agent attempted `cua.getApp("Terminal")`; Computer Use refused access to `com.apple.Terminal` for safety reasons. No bypass attempted. |
| EMU-MAC-TERMINAL-X64 | macOS x86-64 | macOS Terminal | Native build and ordinary baseline behavior | Native Intel hardware/graphical terminal pending external | Rosetta PTY execution is separately completed in `PTY-CLACK-MAC-X64-ROSETTA-FINAL`. |
| EMU-LINUX-X64 | Linux x86-64 | A common local emulator, exact choice/version to record | Baseline keys and readable color fallback | Pending external | — |
| EMU-LINUX-ARM64 | Linux arm64 | A common local emulator, exact choice/version to record | Native executable and baseline behavior | Pending external | — |
| EMU-WINDOWS-TERMINAL | Windows x86-64 | Windows Terminal | Native console, baseline keys, redirected stdin/stdout, cleanup | Pending external | — |
| EMU-KITTY-PROTOCOL | Record actual supported OS/architecture | Kitty or another verified Kitty-protocol emulator | Press/Repeat/Release/associated text; opt-in enhancements and restoration | Pending external | — |
| EMU-NO-ENHANCEMENTS | Record actual supported OS/architecture | Terminal with keyboard enhancements disabled or unavailable | Every essential action remains reachable | Pending external | May share an executed baseline emulator row, with explicit evidence. |
| EMU-TMUX | macOS arm64, tmux 3.7c; outer emulator pending | Actual emulator → tmux → clack | Escape/function-key behavior, resizing, paste, cleanup | Transport passed; graphical outer emulator pending external | `TRANSPORT-TMUX-MAC` executes actual tmux under a synthetic PTY. It does not validate an actual graphical emulator. |
| EMU-SSH | macOS arm64, OpenSSH 10.3p1; remote OS and outer emulator pending | Actual emulator → SSH → remote clack | Controlling TTY, resize, input, reconnect/error behavior | Loopback transport passed; remote/graphical path pending external | `TRANSPORT-SSH-MAC` executes actual authentication/encryption. SSH latency is environmental and excluded from local responsiveness budgets. |

## Automated Unix PTY suite

The script uses only the Python standard library. It provides a bounded VT screen
decoder for the cursor, erase, and mode sequences exercised by this application;
it is not a replacement terminal emulator. Capture is bounded to 8 MiB per stream,
observation metadata to 4,096 records, and the screen to the selected geometry.
Monotonic byte counters remain valid when benchmark consumers clear captures.

```sh
python3 scripts/pty_test.py --self-test
cargo build --offline --locked
python3 scripts/pty_test.py --binary target/debug/clack --report docs/measurements/pty-production.json

cargo build --offline --locked --features test-hooks
mkdir -p target/pty
cp target/debug/clack target/pty/clack-instrumented
python3 scripts/pty_test.py --binary target/pty/clack-instrumented --hooks --require-all --report docs/measurements/pty-instrumented.json
```

The production and instrumented runs are separate evidence. `--hooks` must be
explicit and must correspond to a binary built with the `test-hooks` feature.
Hook-requiring cases are reported **skipped** when hooks were not requested;
`--require-all` makes such skips a failing suite outcome. To exercise completed
integration incrementally, select named cases with repeated `--case` arguments.
`--list` lists all cases. `--self-test` never claims to test clack.

Copy instrumented executables before a long suite: Cargo tests with different
features can replace `target/debug/clack`. One intermediate attempt suffered that
setup collision and could not observe hooks; it is preserved separately as an
invalid build-selection attempt. Both the successful 21-case run and subsequent 22-case closing-epoch run used isolated copies.

The final harnesses check SHA256 before and after execution. Their
`--execution-label` distinguishes native arm64 from x86-64 under Rosetta; the
`environment.architecture` field describes the Python host. `/usr/bin/file`
identifies the frozen binaries as thin arm64 and x86-64 Mach-O executables.
`/usr/bin/arch -x86_64 /usr/sbin/sysctl -in sysctl.proc_translated` returns `1` on
this host (`docs/measurements/rosetta-translation-control.txt`). The translated
application suite does not establish native Intel hardware behavior.

Every application case creates a temporary schema version 1 configuration and
data directory, passes `--config`, `--data-dir`, and `--private`, and removes its
temporary data afterward. It never repurposes HOME or CODEX_HOME. Inputs are
synthetic fixture text. Stdout is captured separately from the controlling TTY,
so ANSI/status pollution of `--once --json` cannot be hidden by a terminal decoder.

| Case | Required assertion | Coverage IDs |
|---|---|---|
| `ctrl_c_cleanup` | Raw Ctrl-C exits 130; redirected stdout stays empty; terminal configuration/cursor/alternate/paste/focus/keyboard modes restored. | KEY-007, TERM-005–009, AT-019 |
| `invalid_source_before_raw` | Eleven launches: literal ESC; file/stdin each invalid UTF-8, ESC/control, bidi,33-scalar grapheme,1MiB+1. Every invalid source exits2 before setup, avoids its private sentinel, and leaves stdout empty. | DATA-013/014, PRIV-006, CLI-002 |
| `invalid_config_before_raw`, `invalid_arguments_before_raw` | Invalid configuration and arguments fail before raw/alternate setup, with clear errors, clean stdout and no private-input disclosure. | CLI-002/008, CFG validation, TERM-005 |
| `no_controlling_terminal` | A detached session fails clearly with exit 1, emits no stdout, and leaves the retained stdin pipe unread. | CLI-006/008 |
| `first_key_focus_json` | Wrong first character appears once at the original target cell; no focus jump; correction/remainder produces exact counts and one JSON object. | UI-004/006, SCORE-002–005, AT-001/002/020 |
| `first_input_without_ready_wait` | Input sent promptly after the app fork, without waiting for Ready, survives initial setup and remains editable; exact counts detect launch-time input flushing. | UI-004, AT-001, DOD-001 |
| `exact_confirm` | Tab and Enter are literal logical units; final typo remains repairable; F5 confirms. | MODE-008/011, AT-007 |
| `stdin_keyboard_separation` | Piped bytes provide source only; controlling-PTY keystrokes produce the score; JSON stdout stays clean. | CLI-004/006, AT-020 |
| `paste_atomic` | Ready paste does not start or type; active paste does not insert, flags practice, and does not prevent further typing. | INPUT-003, AT-013 |
| `deadline_duration` | Well-before-deadline input accepted; 1-second score remains exactly 1,000,000 µs despite terminal scheduling. | TIME-004, AT-008 |
| `challenge_failure_once` | Master/Expert failure produces one failed result with the triggering input counted and no personal-best eligibility. | KEY-005, SCORE difficulty, AT-011 |
| `resize_preserves_run` | Ordinary resize preserves epoch, logical counts, and the running timer. | UI-012, TIME-005, AT-014 |
| `too_small_interrupt` | Active 39×9 geometry interrupts with a reason, exit 130, and no record eligibility. | UI-015, TERM-012, AT-014 |
| `small_ready_focus_mouse` | Unsafe Ready ignores input; safe resize starts normally; focus loss records metadata without pausing; focus gain/mouse reports never restore hidden controls. | UI-005/007/015, TIME-005, TERM-011/012 |
| `ignored_ready_no_redraw` | Repeated leading Space, Backspace and F5 create zero frames/bytes/attempts or timer start; the next eligible character still starts/counts once. | UI-005, ARCH-015/018, PERF idle work |
| `restart_epoch` | Residual completion bytes/Enter do not restart; old-epoch bytes/releases do not enter a Ctrl-R restart; fresh input counts once. Restarted random12-word sample must render identically to a fresh process replaying its exported seed, detecting stale cache positions. | UI-012/029/030, ARCH-011/020, AT-009 |
| `overload_after_completion` | A completion is provisional until the reader closes its epoch. A subsequent overload verdict changes the exported outcome to interrupted, preserves the three scored attempts, sets input_overload and excludes PB eligibility. | ARCH-012/013, AT-021 |
| `repeat_practice` | F2 returns the same fixed sample and visibly labels its repeated result practice. | KEY-003, UI-027, AT-009 |
| `associated_text` | Associated text replaces physical key code; Press/Repeat insert once and Release never inserts. Associated text cannot shadow the essential Ctrl-C command or enter that command into scoring. | INPUT-001, KEY-007 |
| `malformed_associated_text` | Invalid CSI-u Unicode scalar produces explicit input failure/interruption; no leaked characters or eligible record. | INPUT-001, ARCH-013, TIME-006 |
| `sigint_cleanup`, `sigterm_cleanup`, `sighup_cleanup` | Supported signals take the safe exit 130 path and restore shell state. | TERM-006/007, AT-019 |
| `sigtstp_cleanup` | A supported suspend request either exits safely or restores before stopping; resumed once-mode cannot continue the old score. | TIME-002/006, TERM-006, AT-014/019 |
| `panic_cleanup`, `error_cleanup` | Fault after first usable frame exits 1 and restores all changed terminal state; diagnostics stay off stdout. | TERM-006/008/009, AT-019 |

PTY writes have harness timestamps, not input-reader receipt timestamps. Do not
claim the exact 29.999/30.000-second boundary from sleep-based writes. Deterministic
reader/reducer tests must establish ordering, watermark exclusion, delayed
deadline updates, and the 4,096-event overload boundary. PTY duration checks then
establish that the real application connects that engine to a terminal correctly.
Stress and slow-consumer measurement are owned by the benchmark harness and must
report ordered input or explicit overload, never silently accepted missing input.

## Optional instrumented-build contract

The production default has no test observer or injection behavior. In a build
explicitly using `--features test-hooks`, the harness passes an inherited writable
file descriptor through `CLACK_TEST_OBSERVER_FD`. The app writes newline-delimited
JSON only after a complete coherent frame has flushed:

```json
{"event":"frame","state":"running","epoch":2,"cursor":[24,10],"counts":{"attempts_total":1,"attempts_correct":0},"elapsed_us":0}
```

`state` uses lowercase engine/presentation state, `epoch` is the input epoch,
`cursor` contains zero-based terminal cells, and `counts`/`elapsed_us` are numeric
metadata. No source text, entered text, absolute private source paths, or per-key
trace is necessary. The observer is not stdout, stderr, keyboard input, or the
terminal output stream. No per-key observer write is required or desired.

Supported test-only `CLACK_TEST_FAULT` values are `panic_after_first_frame` and
`error_after_first_frame`. Both trigger after successful setup and a complete
usable frame so that cleanup is actually exercised. The expected runtime failure
status is 1. `wait_overload_after_completion` emits a completion-pending marker
and parks the application owner for a bounded injection window; it verifies that
the closing input epoch can invalidate a provisional completion and that a later
save acknowledgment cannot erase the visible interruption. Passing the variables
to an ordinary production build must not enable the hooks. These hooks do not
provide valid production latency measurements.

## Manual emulator protocol

For each actual-emulator row, record the artifact/build and environment first.
Use a production release binary and synthetic/private input.

1. Launch the default test repeatedly. Verify first key counts, no start screen or
   focus click, stable text coordinates, timer alignment, native/fallback caret,
   and default focus behavior. Finish, restart immediately, repeat, and use the
   palette alternative for intercepted function keys.
2. Exercise 40×10 compact mode, 80×24, 120×40, and 200×60; resize while typing and below
   the safe minimum. Inspect long-token wrapping, accents, combining delivery,
   double-width text, and supported emoji clusters. Confirm logical position
   remains stable and no score is silently paused.
3. Check terminal-inherited colors, shipped RGB/fallback themes, NO_COLOR, visible
   non-color error cues, wide-cell placement, and bar/block/underline carets.
   Record font/emoji variability; do not infer screen-reader or universal IME
   accessibility from visual inspection.
4. Exercise exact Tab/newline, final correction/F5, atomic bracketed paste,
   Press/Repeat/Release under optional enhanced reporting, and baseline commands.
5. Capture pre/post shell usability and terminal attributes after normal results,
   Ctrl-C, error, panic-instrumented build, supported signals, and actual system
   sleep/resume where available. Check cursor style, echo/canonical mode, alternate
   screen, bracketed paste, focus reporting, and keyboard mode restoration.
6. Verify piped source remains distinct from keyboard and `--once --json` produces
   a single parseable object on redirected stdout. Use only synthetic text in
   shareable captures. Document any failed or unsupported behavior explicitly.

SIGKILL, terminal-emulator failure, and power loss cannot be made cleanup-safe by
ordinary application code. Actual system sleep behavior, Windows console input,
all target architectures, remote SSH hosts, and graphical outer emulators remain
separate evidence until executed. No unexecuted row may be relabeled pass because
a different row succeeds.

## Supported behavior and limits

Ordinary startup issues no keyboard capability query. Essential actions use
baseline escape sequences: Esc/Ctrl-P commands, Ctrl-R new sample, Ctrl-W permitted
word deletion, Ctrl-C quit, and the function-key/Results shortcuts documented in
the command palette. Every action also has a palette entry. Printable characters,
Tab, and Enter cannot be commandeered from Ready/Running content by custom command
aliases. Function-key interception by an outer terminal is a reason to use the
palette alternative, not a reason to reinterpret typed text as commands.

Optional keyboard enhancements request flags 31, including all-keys-as-escape
reporting required for associated text. The adapter processes Press and Repeat,
ignores Release, and inserts associated text once instead of also inserting a
physical key-code character. Essential commands take priority over associated
text, while AltGr text remains text. Baseline input does not reveal physical keys,
left/right Shift, keyboard layout, or a reliable distinction between hardware
repeat and repeated typing. No such analysis is claimed.

Bracketed paste is rejected atomically: it cannot start Ready and marks an active
result as practice. Legacy unbracketed paste, external macros, and synthetic input
cannot be reliably prevented. This is a local practice tool, not a secure
competition client. Mouse capture is never enabled, and mouse/focus-gained events
cannot restore hidden typing controls.

The default theme inherits the terminal background and foreground. The shipped
explicit themes select RGB, indexed 256-color, or ANSI 16-color roles according to
the chosen color policy and environment. `NO_COLOR` suppresses color unless the
user explicitly chooses `--color always`; underline/reverse/bold cues retain
errors and active position without color. `appearance.ascii_markers` substitutes
simple markers when whitespace/continuation glyphs are unavailable. The app does
not set fonts, font sizes, terminal palette entries, titles, or persistent terminal
configuration. Cursor shape is a steady native bar/block/underline with a styled
cell fallback; no application blink or animation timer exists. Synchronized
output is not required or used.

Unicode acceptance is bounded and distinct from font, IME, bidi, or complex
shaping support. The engine deletes and scores graphemes, while the viewport uses
cell widths. Composed/decomposed Latin accents, combining delivery, CJK width,
regional indicators, and bounded emoji ZWJ sequences have automated fixtures.
See [content limits](content.md) for the conservative accepted repertoire. Actual
glyph widths depend on terminal/font behavior; no universal composition-aware
IME or screen-reader accessibility claim is made. Reduced motion is the default.

The reader compares monotonic progression with wall-clock progression only to
detect suspicious clock/suspend discontinuities; wall time never determines a
score. Backward receipt time, gaps over three seconds during an active reader,
or wall/monotonic divergence over one second interrupt instead of manufacturing
a normal result. Operating systems vary in whether `Instant` advances during
sleep. SIGTSTP/SIGCONT handling is tested; actual machine sleep is a separate
external check. Focus loss records metadata and ordinary resize keeps the same
clock; opening any overlay aborts a running test. Shrinking below 40×10 interrupts.

## Isolated transport reproduction

```sh
python3 scripts/transport_pty.py --binary target/pty/clack-instrumented \
  --tmux target/transport/tmux-3.7c/tmux \
  --report docs/measurements/product-transports-final.json
```

The harness launches an isolated tmux server on a private socket and an isolated
loopback-only SSH server. Host/client keys and configuration are temporary; user
SSH files, system services, and existing tmux sessions are untouched. `StrictModes`
remains enabled. Credentials live in a private workspace temporary directory
because OpenSSH correctly refuses an authorized-keys path beneath world-writable
`/private/tmp`; the tmux socket alone uses the short temporary path needed by
Darwin. The initial combined transport report's SSH failures occurred before clack
launched and are preserved as failed harness evidence. The corrected rerun passes
all ten actual transport cases.

tmux restores its outer cursor using the recorded xterm-256color terminfo `Se`
capability (`ESC[2 q`), while clack resets its inner cursor to the user's default
(`ESC[0 q`). The harness checks both layers independently; its earlier assumption
that the outer reset must be zero is preserved in the failed tmux report. SSH's
unprivileged local server may report that optional BSM audit session setup is not
permitted; those messages belong to the server log, and clack's separate stderr is
asserted empty. No transport timing is substituted for the local response budget.

## Production operation with IP networking denied

`scripts/network_denied_pty.py` launches only its child under the following
process-local macOS profile; no system policy or service is changed:

```scheme
(version 1)
(allow default)
(deny network-bind (local ip))
(deny network-inbound (local ip))
(deny network-outbound (remote ip))
```

The positive controls establish ordinary IPv4/IPv6 loopback connectivity. Under
the profile, TCP connection, local IP binding and UDP sends each fail with
permission errors; a Unix socketpair still exchanges data, retaining the IPC
mechanism needed by the input poll waker. The frozen native production binary
then starts in a PTY, accepts seven attempts, emits one completed JSON result,
commits exactly one SQLite result, passes `quick_check`, and restores terminal
attributes and modes. The exact profile and controls are retained in
`docs/measurements/network-denied-final.json`.

```sh
python3 scripts/network_denied_pty.py \
  --binary target/benchmarks/stage-d/clack \
  --execution-label native_macOS_arm64_production \
  --report docs/measurements/network-denied-final.json
```

This verifies operation while IP networking is denied. It is not packet capture,
a syscall trace, or proof that every possible execution path attempts no network
operation; the independent source/dependency audit supports the absence of a
runtime network client.
