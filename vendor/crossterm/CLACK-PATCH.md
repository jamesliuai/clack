# Clack terminal compatibility fork

Published as `clack-crossterm` 0.30.0 for Clack 1.0.0. This is a different
input API from the earlier Clack 0.1 fork (published as 0.29.0).


Base: crates.io `crossterm` 0.29.0. The upstream MIT license remains in `LICENSE`.
This is a documented local compatibility fork, not an upstream release or an
upstream compatibility claim. Its MSRV is 1.70; clack's application MSRV is higher.
The original upstream unsafe backend operations remain; this patch adds no unsafe
code, runtime, dependencies, renderer, or second keyboard reader.

## Associated text and bounded parsing

The optional `associated-text` feature preserves the third Kitty CSI-u field as
`Event::KeyWithText { key: KeyEvent, text: String }`. The existing `KeyEvent: Copy`
API and Press/Repeat/Release metadata remain intact. Helpers recognize both event
forms. `REPORT_ASSOCIATED_TEXT` exposes protocol flag 16. An explicitly empty text
field remains empty; it never falls back to inserting the physical key code.
Malformed fields, invalid scalars and decoded payloads over 16 KiB produce ordered
`InvalidData` errors, including through both Unix buffering layers.

Incomplete encoded escape sequences are bounded to 128 KiB (the Vec capacity can
reach 256 KiB). An oversized CSI produces one error and discards its remaining
sequence, preventing leaked payload characters. A huge bracketed paste is drained
as one empty-payload paste event; clack rejects the entire paste atomically. The
terminator may cross the buffer limit or any read boundary. Long incomplete byte
streams yield from polling after a one-millisecond service slice so the sole input
owner can handle controls and deadlines.

The application additionally admits only its shared conservative Unicode scalar
repertoire and caps associated events at 128 graphemes and 32 scalars per
grapheme. Combining marks, emoji joiners/tags, and variation selectors can continue
a prior event. Complete targets retain stricter grapheme validation.

Opt-in enhancement requests flags 31. Associated-text flag 16 requires the
all-keys flag 8 according to the [Kitty protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/#report-associated-text).
The application ignores Release and non-text modifier events while preserving
baseline input when enhancements are disabled.

Unsupported CSI/SS3 commands are consumed through their sequence boundary and
ignored without terminating keyboard input. Keypad Begin/Enter and xterm
modifyOtherKeys are decoded as key events. Malformed associated text still
produces ordered errors. Clack disables ambiguous legacy X10 mouse parsing with
`event::set_legacy_mouse_input(false)` because it never captures mouse input;
the fork retains enabled parsing by default for other consumers. A stray private
reply prefix followed by a nonnumeric key no longer consumes that key.

These changes are prepared as 0.30.1. Consumers using `set_legacy_mouse_input`
must select that version; the published 0.30.0 does not expose this API.

Unmatched terminal replies are coalesced to their latest state by reply type,
while ordinary input keeps its order. Reply floods yield after a one-millisecond
service slice so keyboard-owner controls and deadlines remain responsive.
Terminal coordinate decoding uses checked subtraction instead of debug-build
underflows. The application input-owner scope guard closes and notifies its
queue even during unwinding, allowing the render loop to restore the terminal.

## Explicit diagnostic probe

The application uses `event::poll_keyboard_probe` only from `doctor --probe`.
It exposes parsed keyboard-status/device-attribute responses to the sole input
owner, without writing queries, accessing stdout, or changing terminal modes.
The application supplies a nonblocking controlling-terminal writer, a raw-state
guard, a 400 ms overall response deadline, and a 4096-event limit. The filter
accepts every event, so unrelated keys cannot accumulate in a skipped queue.
No normal startup code calls this API. The terminal query protocols provide no
cancellation acknowledgement: a response delayed beyond the bounded probe can
arrive after control returns to the shell. Increasing a drain timeout cannot
eliminate that protocol limit. Missing replies remain unknown; a
device-attribute reply without a keyboard reply indicates protocol absence.

The upstream flags-response decoder treated the first ASCII digit as a raw
bitmask. It now parses the complete decimal integer, validates its u8 bound,
and preserves unknown bits when reporting active flags. It does not assert that
all protocol features are supported merely because a status reply exists.

## Poll control, epochs and signals

The separate `poll-waker` feature exposes `event::poll_waker()` and a cloneable
`PollWaker::wake()`. It reuses existing Unix/Windows wake primitives without
EventStream or an asynchronous runtime. The input owner initializes its handle
before blocking. Controller wakeup never takes the keyboard-reader lock.

`event::discard_pending_input()` is called only by that input owner at an epoch
barrier. It clears pending OS input and incomplete parser/surrogate/repeat state,
preserving process signals and geometry. Unix uses safe rustix `tcflush` on the
controlling terminal; Windows drains a bounded console-input snapshot through the
existing safe backend API. The `use-dev-tty` zero-timeout poll now inspects both
buffered events and ready input; previously it returned before either check.
Both Unix sources report input EOF instead of spinning. The selected TTY source
polls before every terminal read, including after full buffers, because its
terminal descriptor may be blocking. Incomplete sequences cannot block a second
read while signals, wakeups, or a deadline are pending.

The optional `managed-signals` feature routes SIGINT, SIGTERM, SIGHUP, SIGTSTP and
SIGCONT as `Event::Signal`; SIGWINCH stays Resize. It extends the existing signal
self-pipe, with no signal worker and no I/O or locking in an application signal
handler. `event::prepare_signals()` prepares that signal-only pipe before raw-mode
setup; the sole input source later adopts it. This closes the setup interval
without opening or reading the keyboard on the render thread.

## Windows preservation and housekeeping

Windows input polling keeps an independently acquired CONIN$ handle, preserves
console repeat counts, and carries release metadata through surrogate delivery.
Raw-mode setup saves exact input and output console modes; cleanup restores them
instead of blindly enabling input bits, and restores VT output-processing flags.
A later session reenables VT processing when needed despite cached capability
state. The native console still requires actual Windows runtime validation.

Two redundant parentheses in Unix geometry closures were removed for warning-free
builds with both rustix and libc configurations. Upstream line endings are
preserved. The added transport features are optional; EOF, zero-poll, Windows
mode-preservation and warning fixes also correct the ordinary backend paths.

## Validation actually executed

On local macOS arm64:

```sh
cargo test --manifest-path vendor/crossterm/Cargo.toml --lib --offline --features associated-text,poll-waker,managed-signals,use-dev-tty
cargo test --manifest-path vendor/crossterm/Cargo.toml --lib --offline --features associated-text,poll-waker,managed-signals
cargo check --manifest-path vendor/crossterm/Cargo.toml --offline --no-default-features --features events
cargo check --manifest-path vendor/crossterm/Cargo.toml --offline --all-features --all-targets
cargo check --manifest-path vendor/crossterm/Cargo.toml --offline --target x86_64-pc-windows-gnu --features associated-text,poll-waker,managed-signals,use-dev-tty
```

The current security/performance regression run passes 117 tests for the TTY
source and 115 for the alternative MIO source. Each leaves seven pre-existing
cursor/terminal-dependent upstream tests ignored. Minimal events-only and
all-feature/all-target compatibility checks also pass. The application keyboard
suite passes all 15 tests, including 21 isolated lifecycle/input scenarios and
11 explicit probe scenarios; the reader-unwind queue-closure test also passes.

The earlier release audit recorded 109 TTY tests and 108 MIO tests. Its
final local audit logs are `docs/measurements/terminal-vendor-*-final.log`
in the application repository, including minimal/all-features and Windows cross checks.
The checks cover configuration compilation, including cross-compilation of the
Windows backend, rather than native Windows or emulator compatibility. The root
`tests/keyboard_protocol.rs` separately verifies normalization, lifecycle,
controlling-stream separation, epoch barriers and overload through isolated PTYs.
Actual terminal/emulator, tmux, SSH and physical-display results are tracked in
`docs/terminal-matrix.md` and must not be inferred from these checks.
