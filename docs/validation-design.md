# Validation design

This document maps adversarial verification to specification 1.0. It is a test
design, not evidence that a platform or performance gate has passed. Execution
results belong in the coverage checklist, benchmark report, and terminal matrix.

## Pure kernel fixtures

Tests use received-event timestamps supplied by the caller. They do not sleep,
open terminals, inspect private engine fields, or derive expected values by
reimplementing the scoring algorithm.

| Requirement | Observable fixture |
| --- | --- |
| First input, §4.1/§6.4 | Start with an arbitrary timestamp; wrong printable input appears once and establishes the clock. Leading prose spaces and commands preserve Ready. Expected exact leading whitespace starts. |
| Formula golden vector, §7.2 | `c a x Backspace t Space` against nonfinal `cat` gives 5 attempts, 4 correct, 1 deletion, 4 retained, 4 credited, 80% accuracy; at 2 seconds, 24 WPM and raw WPM. |
| Early submission, §6.2 | `c Space` against `cat dog` retains only 2 units, records 2 missed target letters, and gives zero whole-token credit. Repeated empty Space does not skip `dog`. |
| Separator attempts, §6.2 | `c a x Space` is 3 correct attempts out of 4: the separator does not double-penalize an already complete but misspelled token. |
| Partial endpoint, §7.1 | `ca` contributes 2 credited units; `cx` contributes zero. Neither receives an untyped suffix or separator. |
| Final word, §5 | Correct final word completes on its last unit. An incorrect final word remains editable; normal Space may submit it. |
| Correction, §6.2 | Full backtracking through a submitted correct token removes its prior contribution once. `mistakes` reopens only an incorrect prior token; a correct prior token is a barrier. `current` cannot reopen; `none` ignores deletions. Ctrl-W obeys identical boundaries. Historical errors never vanish. |
| Exact content, §5/§6.2 | Tab and newline occupy one logical input unit each. A wrong space occupies one position instead of skipping a token. The final typo can be deleted and repaired before F5. Early F5 produces incomplete practice. |
| Timing, §6.4 | For a 30-second test, a receipt at 29.999 seconds counts and one at exactly 30 seconds does not. Finalizing late still yields exactly 30 seconds. |
| Difficulty, §6.3 | Master fails on the first definite mismatch. Expert allows a wrong character until token commit, including literal exact boundaries and the final F5 segment. Word-stop rejects early submission; letter-stop records wrong attempts without retaining them. |
| Challenge thresholds, §6.3 | Minimum WPM waits through the five-second grace interval and uses the unrounded value at the next complete boundary. Minimum accuracy waits until 20 scored text attempts. |
| Unicode, §7.4 | NFC-equivalent prose matches. Delivery as `e` then combining acute is one revised grapheme attempt against `é`; a canonical prefix cannot prematurely fail master. Deletion removes the whole cluster. Exact content preserves code-point distinctions. Include a multi-scalar ZWJ cluster and a double-width character. |
| Undefined metrics, §7.2 | No attempts means null accuracy; no positive duration means null speeds. Sub-second completed tests use their actual duration. Zen has raw output speed but null target WPM/accuracy. |
| Samples, §7.3 | Idle one-second buckets exist independently of render calls. A fractional tail retains its real duration. Consistency is null with fewer than five full buckets or a zero mean and includes idle buckets otherwise. |
| Paste, §6.4 | Reject the entire paste: no Ready start, no inserted units, and an active-run practice flag. |
| Record identity, §9.4 | Theme and inactive limits do not change identity. Effective rules, source/revision, algorithm/scoring versions, completion, and assistance do. Random seed does not split ordinary categories; explicit seed/repeat prevents standard eligibility. |
| Practice, §9.1 | Repaired mistakes remain candidates using original target spellings. Slow practice excludes the first token and corrected tokens and requires eight eligible tokens. |
| Determinism, §5 | One fixed spec/seed/generator/pack revision yields identical text and ordered replay counts. Snapshot a known output, not two calls that could share the same nondeterministic defect. |

Property tests generate arbitrary input and edit sequences and check independent
invariants after each action: nonnegative counters, correct attempts no greater
than all attempts, credited output no greater than retained output, finite or null
metrics, valid logical positions, bounded retained windows, and unchanged results
after finalized-state input. A replay with a shifted clock origin must give the
same scores and duration. A full-correction insertion/backspace pair restores
retained output while preserving the insertion attempt and deletion count.

## Event ordering and restart boundaries

One reader owns all terminal `poll` and `read` calls and stamps each event when it
is received. Crossterm explicitly requires this single-thread ownership and
disallows mixing these functions with EventStream. [Crossterm event documentation](https://docs.rs/crossterm/0.29.0/crossterm/event/index.html)

The input queue carries monotonically sequenced, epoch-tagged events and ordered
deadline watermarks. The reader and engine share text-start eligibility. The
reader arms its own schedule from the first eligible event receipt, avoiding a
control-message race while the engine processes the identical timestamp.
The reader emits the watermark after all events it already received before the
deadline; events received at or after it are excluded. Rendering cannot finalize
a timed score by independently racing the keyboard queue.

At restart, advance through a reader acknowledgement barrier. Discard old-epoch
queued events, pending OS input and incomplete parser state before accepting events for the new
epoch. Release events never insert text. Results ignores printable text and
debounces unmodified Enter for 150 ms, while Ctrl-R remains immediate. Enhanced
associated text must replace the corresponding key-code insertion, not duplicate
it; the keyboard protocol distinguishes press, repeat, release, and associated
text. [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)

Dependency review found that upstream Crossterm 0.29's `KeyEvent` has no
associated-text field and its parser ignores the third CSI-u text component.
[Crossterm KeyEvent API](https://docs.rs/crossterm/0.29.0/crossterm/event/struct.KeyEvent.html)
The documented local patch in `vendor/crossterm/CLACK-PATCH.md` preserves associated
text and event kind in one event, propagates malformed input errors in order, and
exposes the existing backend wake primitive without enabling EventStream.
`tests/keyboard_protocol.rs` and the vendored parser fixtures verify this contract.
Actual-emulator protocol validation remains separate.

Adversarial scheduling tests should hold rendering while delivering events at
deadline-minus-one, deadline, and deadline-plus-one; deliver a delayed deadline
update; fill the 4,096-event queue; and race restart with text/release bursts. A
full queue must raise a separately wakeable sticky overload interruption before
any run can be classified as a valid score. Never rely on dropping the event or
blocking the reader indefinitely. Coalesce only resize events. The consumer
yields after 64 events or one millisecond so deadlines and controls remain live.

A completed engine result remains provisional until an ordered `EpochClosed`
verdict seals its input epoch. The reader captures sticky overload before reusing
the epoch slot, preserves closure packets across later barriers, and emits the
verdict before `EpochReady`. Results can render immediately, but persistence,
export and personal-best eligibility wait for this verdict. A bounded
`last_closed()` slot also supplies the verdict after reader shutdown/suspension.
Thus overflow arriving after the final reducer action cannot manufacture a valid
result. Full-queue tests exercise closure delivery as well as ordinary input.

## Suspend and terminal cleanup

Rust does not specify whether Instant includes system suspension. Compare
monotonic and wall elapsed intervals and conservatively invalidate an active run
on large divergence or an unexplained large scheduling gap. The detector may wake
without drawing; hidden metrics do not cancel deadline or integrity checks.
Regressed monotonic timestamps also invalidate the run instead of becoming an
invented zero-duration interval. [Rust Instant documentation](https://doc.rust-lang.org/std/time/struct.Instant.html)

Pseudo-terminal tests verify alternate-screen/raw-mode/paste/cursor restoration
after normal completion, explicit interruption, injected setup failure, panic,
and supported signals. The session guard tracks changed capabilities and possible
partial writes that still need compensating cleanup. Signal handlers notify a safe execution path; they do not perform I/O or
locking directly. Stop the input reader and any terminal-writing workers before
restoring state. Cursor style returns to the queried prior value when supported,
otherwise the terminal default. Baseline keys must remain usable with keyboard
enhancements disabled. No mouse capture is required.

Actual emulator, tmux, SSH, suspend/resume, Windows console, and physical-display
tests remain separate from in-memory rendering and pseudo-terminal checks. A
successful local build cannot establish those results.

The current executable checks include `cargo test --test keyboard_protocol`
(13 tests, including 16 isolated macOS PTY scenarios), seven reader unit tests,
57 independent kernel acceptance/property fixtures and the unchanged content
validation fixtures. The input suite exercises atomic text/paste admission,
control-binding eligibility, epoch closure under overflow, partial CSI/paste
discard, setup-time signals, exact deadlines, runtime enhancement toggling,
pipe separation and suspend/cleanup. It requires permission to open each child's
own `/dev/tty` where the tool sandbox denies that access by default.

`docs/measurements/pty-epoch-instrumented.json` records 22 passing full-application
PTY cases against an immutable instrumented executable with SHA-256
`9e8c942c49be581f726ad768e603f7f08d8bb174a73d023977c71dccd2f909b0`.
The `overload_after_completion` case waits for a test-only `completion_pending`
observation, fills the undrained application queue with paced input, then verifies
an interrupted exported result with unchanged final counts and no record
eligibility. Instrumentation parks only the engine/render owner at that boundary;
the input reader continues to receive and reports its actual sticky overflow.

## Performance evidence

Use release builds. Record hardware, OS/compiler, profile, terminal/version,
geometry, input/content dataset, storage size, and enabled options. Startup is
measured externally from process creation through the first usable frame. Report
warm-cache and genuinely cold experiments separately. A reducer benchmark cannot
establish input-to-flush latency or physical key-to-photon performance.

Measure 80×24, 120×40, and 200×60 with ASCII and mixed-width text; healthy and slow
PTY readers; and 1,000 events/second. Verify 1,000 successive runs and enough zen
output to cycle every configured bounded window. Report the actual measured
limits and any unavailable reference-machine or emulator work explicitly.
