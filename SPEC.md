# Rust Typing CLI — Product and Engineering Specification

**Document version:** 1.0  
**Research date:** September 4, 2026  
**Working executable:** `clack`
**Status:** Specification for a new implementation; performance figures are acceptance targets, not measured results.

## 1. Product definition

Build a local-first, keyboard-driven typing test with Monkeytype's immediate feedback, configurable tests, and repeatable practice, redesigned for a terminal rather than copied from a website.

The defining interaction is **launch → type → results → type again**. Running `clack` must display a usable test, not a homepage, menu, setup wizard, or empty loading shell. The first text input starts the clock and removes nonessential interface elements without moving the text. Configuration and analysis remain available, but live outside the typing surface.

Priority order: input correctness and responsiveness; visual clarity and stability; useful customization; trustworthy scoring and history; breadth of content. A new feature must not compromise the earlier priorities.

Ship a native Rust binary for macOS, Linux, and Windows. No account, network service, browser, daemon, telemetry, or recurring connection is required.

## 2. Research conclusions and their implications

| Finding | Product implication |
|---|---|
| Monkeytype's inspected defaults include a 30-second timed test, English, punctuation/numbers disabled, a small timer, configurable caret, and optional live metrics. Its configuration also supports error rules, themes, pacing, and practice-related options. [S1] | Preserve immediate typing and orthogonal settings. Do not reproduce the website's complete settings surface on the main screen. |
| Monkeytype distinguishes whole-word WPM, raw WPM, input accuracy, and consistency. [S2] | Use separate counters for final output and input attempts. Do not calculate WPM as raw WPM multiplied by accuracy. |
| The inspected difficulty logic distinguishes submitting an incorrect word from making an incorrect character attempt. [S3] | Implement expert and master as explicit engine rules, not UI labels. Specify their interaction with correction rules. |
| `ttyper` already demonstrates Rust/Ratatui typing tests, custom text, languages, and configurable styling. `tt` emphasizes shell composition. [S4–S5] | These capabilities are baseline functionality, not sufficient differentiation. Differentiate through focus behavior, polished defaults, reliable history, and measured latency. |
| Ratatui renders through intermediate buffers; applications still own input, state, and scheduling. [S6] | Start with Ratatui and a custom typing widget. Do not build a terminal renderer from scratch without profiling evidence. |
| Legacy terminal keyboard encodings contain ambiguities; enhanced protocols improve reporting but cannot be assumed everywhere. [S7–S8] | Essential controls must work with baseline terminal input. Do not promise physical-key analysis, universal IME behavior, or secure anti-cheat. |

The specification below makes deliberate product decisions. It is **Monkeytype-inspired, not a guarantee of score-for-score compatibility** with Monkeytype, especially for Unicode, partial words, assisted code, and consistency.

## 3. Release scope

### Required for 1.0

Timed and word-count tests; quotations; custom files and piped text; a code-text preset; free-writing zen mode; punctuation and numbers; importable language packs; themes and layout preferences; a searchable command palette; saved presets and keybindings; correction/difficulty rules; optional live metrics and pace caret; missed/slow-word practice; compact results plus detailed review; local history and personal bests; JSON/CSV export; resilient terminal cleanup; automated correctness and performance tests.

### Not in 1.0

Accounts, online leaderboards, multiplayer, cloud sync, social features, achievements, advertisements, plugins, AI-generated exercises, visual keyboard diagrams, physical finger/hand analysis, keypress sound packs, animated backgrounds, or a syntax editor. Do not build a web companion. Do not port Monkeytype's entire collection of novelty modes.

Full bidirectional-script support, complex script shaping, and composition-aware IME timing require separate validation; they are not implied by accepting UTF-8. Add them deliberately rather than advertising universal language support.

## 4. Interface and interaction

### 4.1 First launch

Default test: time mode, 30 seconds, English 200-word pack, normal difficulty, punctuation/numbers off. Backspace policy is `mistakes`; live WPM and accuracy are off; a small time counter remains visible. Show three lines in a 72-cell-wide text block, subject to terminal size.

No welcome screen, logo, mandatory configuration, tutorial modal, or start button. A subdued shortcut hint is sufficient onboarding.

Ready state, schematic:

```text
             time 30  ·  english 200

             the world can change when small things become
             part of the way we work and think about what
             comes next in a place we know so well

             start typing                 esc commands
```

The caret is at the first target character immediately. The first eligible printable text input starts the clock even when it is incorrect. Leading space, modifiers, navigation, and commands do not start a word-based test. In exact/code mode, expected leading whitespace is eligible input.

### 4.2 Active typing

On the first input, remove the mode label, hints, branding, and navigation. Preserve the block's coordinates; removing controls must not recenter or rewrap the text.

```text
             24s

             the world can change when small things become
             part of the way we work and think about what
             comes next in a place we know so well
```

An optional status line can show `24s · 87 wpm`. Never show an always-visible dashboard, history sidebar, toolbar, or boxed widgets during typing. Full-focus mode hides the status line as well. Mouse movement must not restore hidden controls.

Timer and text are left-aligned to the same block. Reserve stable field widths so changing numbers do not shift other elements. In word mode, optional progress is completed words / requested words, not words remaining in the currently generated buffer.

### 4.3 Layout rules

Default block width is `min(72, terminal_columns - 8)` cells. Center horizontally and place the block near 45% of terminal height, with clamping to keep all visible rows inside the screen. Offer centered and top-aligned layouts, width 40–120 cells or automatic, one to five text rows, and zero or one blank row between text rows.

Use logical text positions, not screen coordinates, as the source of truth. With three rows, retain one preceding row and one following row once scrolling begins. Advance by whole rows; do not implement pixel-like smooth scrolling. Never split a grapheme cluster. Break unusually long tokens across rows safely without adding scored characters.

At 40×10, use the compact one-row layout and hide secondary information. Below 40×10, do not start a test. If an active test becomes too small to display safely, end it as interrupted and explain why; do not silently pause a scored timer.

### 4.4 Visual language

Use whitespace, alignment, and restrained contrast instead of borders. No ASCII-art logo, progress ring, decorative separators, required Nerd Font, emoji controls, or bright green correct text by default.

Theme roles: background, pending text, correct text, incorrect text, extra text, muted UI, accent, caret, and pace caret. The default theme inherits terminal background and foreground; ship independently designed dark, light, warm-accent, and high-contrast alternatives. Explicit RGB themes must also have useful 256/16-color fallbacks.

Pending text is subdued but readable; correct text uses the primary foreground; errors use both an error color and a non-color cue such as underline. At an incorrect position, show the entered character; detailed review shows expected versus entered text. Display extra letters distinctly and reflow only the affected visible suffix when needed. Ordinary correct input must not relayout the document.

Default caret: steady native bar where supported, otherwise a clearly styled cell. Block and underline alternatives are configurable. Do not run an application animation loop just to blink a caret. Do not change the user's font, font size, terminal palette, window title, or terminal configuration.

Honor `NO_COLOR` unless explicitly overridden; monochrome mode must still distinguish errors and the active position. [S14] Reduced motion is effectively the default. Do not claim screen-reader accessibility without testing it separately.

### 4.5 Results

```text
             92 wpm       98.6% accuracy
             raw 96       30s       english 200

             personal best +3

             enter next   f2 repeat   f3 practice   f4 details
```

The personal-best line is conditional. Compact results show WPM, accuracy, raw WPM, duration, configuration identity, and any important status: failed, practice, interrupted, or unsaved. A larger dashboard is available only through Details.

Do not require a popup dismissal before another test. Enter starts a new sample with the same effective settings. F2 repeats the exact sample. Ctrl-R remains the immediate new-sample action. Printable letters do nothing on Results, preventing residual typing from accidentally starting another run. Ignore pre-transition queued input; additionally debounce unmodified Enter for 150 ms after completion. Explicit Ctrl-R is not delayed.

### 4.6 Commands and keybindings

| Action | Default | Behavior |
|---|---|---|
| Commands | Esc or Ctrl-P | Open searchable command palette. During a run, abort that run first; no paused scored state. |
| New sample | Ctrl-R | Abort if necessary, generate new text, enter Ready. |
| Repeat sample | F2 | Reuse exact text/configuration, reset timing and counters, label repeated practice. |
| Next sample | Enter on Results | New text with the same settings. |
| Practice | F3 on Results | Open missed/slow-word choices. |
| Details | F4 on Results | Open review without modifying the recorded result. |
| Finish | F5 | Finish zen; confirm a fully traversed exact-text test; otherwise finish early as incomplete practice. |
| Delete character | Backspace | Delete one permitted grapheme, subject to correction policy. |
| Delete word | Ctrl-W | Delete within the permitted correction range. |
| Quit | Ctrl-C | Exit cleanly; preserve acknowledged results and report any unsaved result. |

The palette replaces the text area rather than stacking permanent UI around it. It has a search field, at most seven visible matches, the selected value, and a one-line explanation. Actions include modes, duration/count, language, content modifiers, theme, focus, caret, difficulty, presets, history, configuration, and help. Arrow keys select; Enter applies; Esc cancels. Preview themes only while not running; cancel restores the previous theme.

All actions must be reachable through the palette, so intercepted function keys are not a dead end. During Ready/Running, printable characters cannot be rebound to commands. Tab and Enter belong to exact/code content when applicable. Reject ambiguous or conflicting bindings rather than silently shadowing input.

## 5. Test modes and content

| Mode | Required behavior |
|---|---|
| Time | Presets 15/30/60/120 seconds; custom integer 1–3,600 seconds. Start on first eligible input; finish at the exact deadline; include or exclude the final partial word according to §7, not by accident. |
| Words | Presets 10/25/50/100; custom 1–10,000. Generate the exact count. Complete automatically when the final word is correct, or submit it with Space under normal difficulty. Never require a trailing space after an already correct final word. |
| Quote | Local, attributed passages; short 1–30 words, medium 31–75, long 76–150, extended 151–300. Preserve punctuation/case, normalize prose whitespace, and avoid immediate repeats. Permit selection by stable quote ID. |
| Custom | UTF-8 file, explicit literal text, or standard input. Prose policy normalizes whitespace; exact policy preserves whitespace, indentation, tabs, and line breaks. Read/validate before entering Ready. |
| Code preset | Custom exact text, not random programming keywords masquerading as code. No parser or syntax highlighting is required. Tab is one logical input unit with configurable displayed tab stops. Enter supplies a newline; CRLF is normalized to LF. |
| Zen | Free writing without a reference. Show duration and output speed; accuracy, errors-against-target, and target-based personal bests are not applicable. F5 ends the session. Keep bounded scrollback and aggregate older content. |

For exact text, default completion is explicit confirmation: once all target units have been traversed, the user may correct the tail and press F5. Before that point, F5 records an incomplete practice result. This avoids ending a code test before the last typo can be repaired. An optional automatic-completion setting is a separate profile parameter.

Custom literal text supplied on a command line can enter shell history; documentation should recommend a file or stdin for private material. Do not echo private text in error messages or logs.

### Content generation

Embed the default English 200 pack and a small quote set. Ship English 1,000 and 10,000 packs and documented import support; add French, German, and Spanish packs through the same format once their provenance is verified. Do not bundle hundreds of dictionaries just to claim coverage.

Language pack metadata: stable ID, language tag, revision, source, license, content hash, text direction, supported input policy, and normalized unique tokens. Import one-word-per-line UTF-8 plus a metadata file. Validate offline; do not execute scripts or fetch dependencies.

Random generation samples uniformly from the selected pack and rejects immediate adjacent duplicates when at least two distinct tokens exist. Use a specified seeded PRNG and version the generation algorithm. The same seed, pack revision, settings, and generator version must produce identical text across supported platforms. Seed alone is not a sufficient replay identifier.

Generate timed content in deterministic chunks, retaining at least two screens of lookahead. Generation must never depend on the timing of keypresses. Prepare pack indexes and Unicode metadata outside the input reducer.

Punctuation and numbers are independent modifiers. Initial punctuation algorithm: sentence lengths 4–12 words, first-word capitalization, terminal period, and a 10% interior comma probability. Initial numbers algorithm: replace 10% of selected tokens with a 1–4 digit decimal number, without leading zeroes. Apply number replacement before punctuation. Store algorithm version and effective parameters. Do not alter quote/custom content unless the user explicitly chooses a transformation.

Limits: custom input ≤1 MiB; reject invalid UTF-8 and an empty normalized target. Limit imported pack tokens to 128 graphemes and 8 KiB each, and any single input grapheme cluster to 32 Unicode scalars; reject oversized/pathological content during validation with a precise diagnostic. Long zen sessions retain only a bounded text window; earlier text is no longer editable, while cumulative counters remain valid. Full-history backspace applies only to retained content in zen.

All bundled text requires source/license metadata. Monkeytype's repository is GPL-3.0; use independent branding and make any code/data reuse an explicit licensing decision. [S15] Do not assume repository licensing automatically establishes rights to every quoted passage.

## 6. Engine behavior

### 6.1 State machine

```text
Ready -> Running -> Results
  |          |
  |          +-> Aborted/Interrupted -> Palette or Ready
  +-> Palette/Settings/History -> Ready
Results -> Review/Practice/Ready
```

Only Running accepts scored input. Palette search text can never enter a test buffer. Focus presentation is derived from engine state and preferences, not implemented as a second test state.

A test has an immutable specification: content/source identity, seed and generator version, input policy, scoring version, duration/count, rules, and assistance flags. Changing scoring-relevant settings starts a new test. Cosmetic configuration is independent of scoring identity.

### 6.2 Matching and correction

Word/prose policy compares graphemes positionally inside each token. Space submits a nonempty token. Early submission marks its unentered suffix as missed; extra input remains visibly extra. Empty leading/duplicate spaces are ignored in this policy and do not skip whole words. A separator attempt is incorrect when it skips unentered target units; existing letter errors alone do not make the separator another error.

Exact policy uses the literal target sequence. A mistyped space occupies the expected position; it does not skip the rest of a token. Whitespace and newlines are real scored units. Backspace reverses retained output, not historical attempts. This is a typing test, not an editor: arbitrary cursor navigation and insert-mode editing are not part of scored input.

Correction policies:

- `mistakes` (default): edit the current token; at its empty boundary, reopen the immediately preceding incorrect token. A correct submitted token is a barrier.
- `current`: edit only the current token.
- `full`: backtrack through previously submitted tokens in retained content.
- `none`: disable deletion.

Ctrl-W applies the same permission boundaries as Backspace. Reopening or deleting a token must reverse its previous output-score contribution exactly once. Implement this with per-token contributions and an edit stack, not a full-transcript scan.

### 6.3 Difficulty and blocking

Normal accepts errors. Expert fails when a nonempty incorrect token is submitted. In exact policy, crossing a target token boundary submits it; F5 confirmation submits the final segment. Master fails on the first definite incorrect text attempt. These correspond to distinct behaviors in the inspected Monkeytype implementation. [S3]

Stop-on-error is separate: `off`, `letter`, or `word`. Letter mode records an incorrect attempt but does not advance/retain the wrong character. Word mode prevents submission until the token is correct. Validate combinations: word-stop plus deletion-disabled is invalid; expert plus word-stop should be rejected as conflicting intents rather than silently making one ineffective.

Minimum-WPM and minimum-accuracy rules are optional. WPM checks use unrounded cumulative WPM at one-second boundaries after a five-second grace period. Accuracy checks begin after 20 scored text attempts. Show the exact failure reason. These challenge results have their own profile identity.

Blind mode hides error styling until Results; it does not modify matching, accuracy, or failure rules. The code preset may optionally insert expected indentation after newline, but inserted units are marked assisted, never credited as manually typed, and excluded from standard records.

### 6.4 Timing and terminal events

Use `Instant` for elapsed scoring time, never frame counts or wall-clock subtraction. Store wall-clock UTC only for history timestamps and interruption diagnostics. Rust does not guarantee uniform suspend behavior for `Instant` on every platform; detect suspend/large clock divergence conservatively and mark the run interrupted instead of manufacturing a normal score. [S9]

Timestamp events when the input reader receives them. The start timestamp is that of the first eligible text event. Accept timed-test events only when their receipt timestamp is strictly before the deadline. An event exactly at the deadline is excluded. Final timed duration is the configured duration even when rendering or finalization happens late.

Never pause a scored timer on focus loss, menu opening, ordinary resize, or a slow terminal. Focus loss may set a metadata flag when reported. Opening an overlay aborts; resizing normally preserves the test. Sleep/suspend and unrecoverable display/input interruptions invalidate standard-record eligibility.

Process text Press and Repeat events; never insert on Release. Treat an enhanced event's associated text as the text, not as an additional insertion beside its key code. Do not infer keyboard layouts or left/right Shift from text input. Baseline terminal key-repeat events cannot be reliably distinguished from repeated typing on every terminal. [S7–S8]

Enable bracketed-paste handling. Reject a paste event as a unit during testing; do not feed its characters into scoring. Before a run, it does not start the timer. During a run, record `paste_attempted` and classify the result as practice-only. Legacy unbracketed paste and external macros are not reliably preventable; this is a local practice tool, not a secure competition client.

## 7. Scoring specification

Use an explicitly versioned metric implementation, `scoring_version = 1`. Numeric display rounding is never used for calculations or personal-best comparisons. Store counts and integer elapsed time so scores can be reproduced.

### 7.1 Distinct counters

`attempts_total` counts scored text insertion attempts, including wrong attempts later deleted and wrong attempts rejected by letter-stop. `attempts_correct` counts those attempts matching the expected input position. Deletion and commands count separately and do not inflate either value.

`retained_units` counts manually entered graphemes still present in the retained output, including wrong and extra units and typed separators. Units discarded from zen scrollback retain their aggregate contribution. Deleted text does not contribute to this output counter.

`credited_units` counts manually entered units in correctly completed target tokens, including their typed separators. A submitted incorrect token contributes zero, even if some letters were correct. Leading exact-text whitespace can form its own segment. Auto-inserted indentation contributes zero. Separately expose final positional correct/incorrect/extra/missed character counts; those are not synonymous with whole-token credited units.

For a live or timed endpoint inside the active token, count its entered prefix only if the entire entered prefix matches the target and has no extras. Do not credit untyped suffix characters or a separator that was never typed. For an incorrect active prefix, whole-token credit is zero. Apply this same rule live and at completion.

### 7.2 Formulas

For elapsed seconds `t > 0`:

```text
WPM      = 12 * credited_units / t
Raw WPM  = 12 * retained_units / t
Accuracy = 100 * attempts_correct / attempts_total
CPM      = 5 * WPM
```

WPM is five-character-equivalent speed, not the number of dictionary words typed. Monkeytype documents the same five-character convention and distinguishes output speed from input accuracy. [S2] These exact endpoint and Unicode rules are this product's decisions, not a claim of identical Monkeytype scores.

Before enough time has elapsed, display `—` rather than infinity. Live speed stays `—` for the first second; finalized sub-second tests use their actual positive elapsed duration. No-input runs have no valid score. A zero-duration completion, such as a one-character custom target entered on the starting event, has null speed and is not personal-best eligible; never invent a denominator to produce a number. Accuracy with zero attempts is `null`, not 100%. Zen has output speed, but target-based WPM and accuracy are `null`.

Example: for a nonfinal target token `cat `, input `c a x Backspace t Space` yields five text attempts, four correct attempts, one deletion, four retained units, and four credited units. Accuracy is 80%; correcting the typo does not erase it. At a measured two-second endpoint, WPM and raw WPM are both 24.

### 7.3 Charts and consistency

Record one-second samples, independently of render frequency: cumulative WPM, cumulative raw WPM, attempt accuracy, insertion-attempt rate, and error count. Include zero-input intervals. Record a fractional tail for the chart without pretending it is a full second. Cap retained chart samples and downsample only for display.

Optional consistency is a documented product-specific statistic: for at least five complete one-second buckets, take insertion-attempt rates, their population mean `mu` and standard deviation `sigma`, and compute `100 / (1 + sigma / mu)`. Include idle buckets; omit the fractional tail. If there are fewer than five buckets or `mu = 0`, use `null`. Do not call this exact Monkeytype consistency.

### 7.4 Unicode

Keep byte offsets, Unicode scalars, grapheme positions, and terminal cells distinct. Use grapheme-aware segmentation and deletion, NFC-normalized comparison for prose, and display-width calculation for layout. Exact code mode preserves original code points unless normalization is explicitly selected. Unicode segmentation, normalization, and displayed width solve different problems. [S10–S12]

Text delivery can arrive as several scalars for one grapheme. Maintain a provisional trailing grapheme; finalize its scored attempt on a new grapheme boundary, deletion, submission, or finish. Extensions of that same pending grapheme revise that attempt rather than create extra attempts. A canonical prefix of an expected composed grapheme must not trigger a false master-mode failure. Count a definite mismatch or finalized incomplete cluster as wrong. The ordinary ASCII path remains immediate.

Typing-unit WPM uses graphemes, not UTF-8 bytes or display cells. Unicode accuracy refers to committed text-unit attempts, not physical keypresses. Keep language/input/scoring profiles separate. Validate composed/decomposed accents, combining sequences, double-width characters, and zero-width joiners; do not infer full script support from these tests alone.

## 8. Customization without visual clutter

Every supported setting must be available in a typed configuration file; ordinary settings also belong in the palette. Keep independent concerns independent rather than creating dozens of near-duplicate modes.

| Group | Settings |
|---|---|
| Test | Mode, time/count, pack/source, punctuation, numbers, exact/prose policy, completion behavior. |
| Display | Theme, focus behavior, width, alignment, text rows, line spacing, typed/error appearance, caret. |
| Status | Time/progress, cumulative WPM, accuracy; each independently visible or hidden. Speed unit WPM/CPM. |
| Rules | Difficulty, backspace policy, stop-on-error, blind mode, minimum WPM/accuracy. |
| Practice | Pace disabled/fixed WPM/personal-best pace; code auto-indent; missed/slow selection. |
| Workflow | Named presets, favorite themes/packs, bindings, result detail defaults. |
| Privacy | Save results, save custom text, save diagnostic event trace, private-session mode. |

Default to timer-only status, not every metric enabled. Users should be able to create a completely text-only active view, an informative training view, or a high-contrast accessibility-oriented view without recompiling.

Example canonical configuration; the executable and schema are proposed APIs, not existing commands:

```toml
schema_version = 1

[test]
mode = "time"
seconds = 30
words = 50
language = "english_200"
punctuation = false
numbers = false

[appearance]
theme = "terminal"
focus = "auto"                  # auto | always | off
width = 72
alignment = "center"
lines = 3
line_spacing = 0
caret = "bar"

[status]
progress = true
wpm = false
accuracy = false
speed_unit = "wpm"

[rules]
difficulty = "normal"
backspace = "mistakes"
stop_on_error = "off"
blind = false

[practice]
pace = "off"
auto_indent = false

[privacy]
save_results = true
store_custom_text = false
store_event_trace = false
```

Initial precedence: built-in defaults < user configuration < explicitly invoked preset < CLI flags. An explicit later UI change overrides the running value. CLI overrides alone never rewrite defaults. Palette/settings changes persist their edited fields; applying a preset through the palette is an explicit settings change. A “Save current as default” action persists an effective session intentionally.

Use one settings registry for validation, defaults, labels, help, command exposure, and serialization metadata. Preserve comments when editing TOML. Write configuration atomically. Unknown fields and invalid combinations produce a useful path/value diagnostic; do not silently reinterpret them. Automatic launch may fall back to safe defaults after reporting a broken config, but must preserve the broken file; an explicitly supplied invalid config exits with an error before entering raw mode.

Named presets are data, not executable plugins. Keep at least three: default, focused (status hidden), and code (exact input, confirm-to-finish). Presets never smuggle in shell commands.

## 9. Practice, review, and history

### 9.1 Immediate practice

“Practice missed” includes words that had an incorrect attempt, even if repaired, plus submitted words with mistakes or omissions. Use original target tokens, not the user's incorrect spellings. De-duplicate the selection and build a 25-word practice test by shuffling/repeating it. Do not reapply punctuation to already punctuated tokens. If there are no candidates, disable the action with a clear explanation.

“Practice slow” uses the slowest quartile of correctly completed tokens from the last test, requiring at least eight eligible tokens. Measure from the previous token boundary to this token's submission and normalize by expected text units; exclude the first token and tokens containing corrections. This is a local practice heuristic, not a physical-key diagnostic.

Practice temporarily uses a named practice configuration and restores the original test configuration afterward. These sessions are labeled practice and excluded from standard records. Do not add a machine-learning recommendation system to accomplish simple word selection.

### 9.2 Pace caret

Optional fixed-WPM or matching-personal-best pace. Advance its logical target position by `floor(target_wpm * 5 * elapsed_seconds / 60)`. Render it only when it falls inside the user's current viewport; never scroll away from the user to follow it. The user's caret wins if positions overlap. Update at no more than 10 Hz. Pacing is an assistance flag in result identity.

### 9.3 Detailed review

On demand, show WPM/raw history, error buckets, consistency, final character counts, total attempts/deletions, missed tokens, and the expected/entered text differences. Separate “mistakes during typing” from “errors left in the final output.” A sparkline is sufficient in compact history; full plots require an explicit Details action.

Current-test review and repeat must work without disk persistence. With default privacy settings, historical custom-text review requires the user to provide the original file with the same hash; do not pretend the application retained private text it intentionally discarded.

### 9.4 Comparable records

Construct a canonical profile key from mode/limit, language pack ID and content revision, generator version, punctuation/numbers parameters, difficulty, correction/stop rules, text policy and normalization, scoring version, completion policy, and assistance flags. Fixed passages additionally include content identity. Cosmetic changes never split records; materially different test conditions do. Include only effective parameters: an inactive word-count preference must not split timed-test records.

Random seeds distinguish runs but do not split ordinary random-test record categories. An explicitly supplied seed or repeated sample is practice; it does not enter the normal random-test record table. Quote/custom records are per content identity, not comparable with random-word records. Only successfully completed eligible runs can become standard personal bests. Practice, aborted, failed, interrupted, paste-attempted, assisted-code, and incomplete runs are excluded, but remain available under their own history filters.

Personal-best attribution waits for the indexed historical comparison; do not announce a best merely because history has not loaded, and do not delay the rest of Results while checking. Store raw counts and elapsed time; compare exact ratios before display rounding. Label these local personal bests, not verified competition records. History defaults to the current matching profile and supports date, mode, language, and outcome filters. Show sample count with averages. Aggregate accuracy as total correct attempts / total attempts; aggregate output speed as total credited units / total elapsed time, not an unweighted average across differently sized tests. Median run speed can be a separate explicitly named metric.

## 10. Architecture

### 10.1 Libraries and boundaries

Use stable Rust with a committed lockfile and documented minimum supported Rust version. Select mutually compatible Ratatui/Crossterm versions at implementation time rather than independently taking arbitrary newest versions.

Recommended building blocks: Ratatui and Crossterm for terminal UI/input; Clap for CLI parsing; Serde and TOML/TOML-editing support for typed config; a versioned seeded generator; Unicode segmentation/width/normalization crates; Rusqlite with bundled SQLite; platform-appropriate directory discovery. Test tooling should cover unit/property tests, UI snapshots, pseudo-terminal tests, and benchmarks. Do not add Tokio, an ECS, a dependency-injection framework, a database server, or custom unsafe code without a demonstrated requirement.

Keep the repository small. One core library and one binary are sufficient; modules can be organized as:

```text
src/
  engine/       # state transitions, matching, edits, scoring, clock interface
  content/      # packs, generation, passage loading, validation
  ui/           # typing viewport, palette, results, review, history
  terminal/     # input normalization, capability policy, session guard
  storage/      # result records, SQLite migrations, exports
  config.rs     # settings registry, validation, presets
  cli.rs        # flags/subcommands; no typing logic
  main.rs       # composition and scheduling
```

The core has no terminal, filesystem, SQL, or real-time clock dependency. Its boundary is conceptually `apply(action, elapsed_time) -> state_changes`. Inject a clock; replay the same ordered actions without sleeping in tests. Rendering reads a view model and does not mutate scoring.

Represent target content as normalized text plus stable grapheme/token spans. Represent actual input separately. Retain token contributions, correction permissions, aggregate counters, and a logical viewport anchor. Do not use a string of colored terminal escape sequences as application state.

### 10.2 Thread ownership and event flow

Use one dedicated input reader, one engine/render thread, and one lazily started storage worker. The reader exclusively owns Crossterm `read`/`poll`; Crossterm documents that these must be used on the same thread and not mixed with `EventStream`. [S8]

The input reader stamps and sequences events, then sends them through a bounded queue. The engine/render thread exclusively owns test state and terminal output. The storage worker receives immutable result snapshots and never reads the keyboard or writes terminal UI.

Tag events and commands with a test epoch so old input cannot leak across restart. Deliver an ordered deadline watermark after all events received before that deadline; finalize from that ordered stream, not from a race between a drawing tick and the keyboard queue. Updating a test's deadline must not restart its clock.

Bound the event queue to 4,096 small events. Never silently drop input: if the queue cannot keep up, surface an input-overload interruption and invalidate standard scoring rather than claiming a valid run with missing characters. Coalesce resize notifications, not text events. Keep control/deadline handling responsive under a sustained input burst.

### 10.3 Scheduling and rendering

This is an event-driven application, not a game loop.

1. Sleep until input, a worker response, the next visible status change, or an engine deadline.
2. Apply input immediately in timestamp order. Drain at most 64 events or one millisecond of work before yielding to other due work.
3. Mark the view dirty only when visible content/state changes.
4. Render when dirty, subject to a 120 Hz ceiling; never delay engine updates to satisfy the visual limit.
5. Flush one coherent frame. Return to waiting.

The time label normally changes at 1 Hz; live speed/accuracy at 4 Hz; optional pacing at ≤10 Hz. Ready and Results have no application-driven periodic redraw by default. The deadline remains scheduled even with every visible metric disabled.

Ratatui expects the full visible frame to be described on each scheduled draw and handles intermediate-buffer comparison. [S6] Render a complete frame; do not incorrectly draw only “dirty widgets” into a freshly cleared frame. Terminal output should nevertheless be limited to changed cells. Do not clear the terminal on each keypress.

Implement the typing area as a custom widget using prepared spans and direct cell placement. Cache grapheme boundaries, widths, target line breaks, and visible styles. Invalidate layout for width changes or affected text overflow, not for every normal character. Use synchronized output only as an optional supported capability, never as a requirement for correct rendering.

### 10.4 Work forbidden on the input path

No disk writes, SQL queries, network access, file opening, pack parsing, global transcript diffing, full-history scoring, unbounded logging, or random theme selection per keypress. Calculate metrics incrementally. Preallocate common buffers. Do not promise every library call allocates nothing: the zero-allocation goal applies specifically to the warmed, ordinary ASCII engine reducer and must be measured.

For finite tests, retain the bounded state needed for correction/review. For long zen sessions, aggregate discarded text and keep only a bounded editable window. Keep optional diagnostic traces bounded and disabled by default; never accumulate a permanent event log merely to compute live WPM.

## 11. Performance acceptance targets

These are engineering gates for the default 30-second English test, not statements that Rust or a library automatically meets them.

| Metric | Initial target |
|---|---|
| Warm launch to first input-ready rendered frame | p95 ≤50 ms |
| Cold launch on local SSD | Target ≤200 ms; publish cold methodology separately |
| Received input to completed output flush | p95 ≤10 ms, p99 ≤20 ms at 120×40 |
| Warm ASCII reducer work, excluding terminal I/O | p99 ≤50 microseconds; no steady-state heap allocation |
| Ready/Results idle CPU | <0.1% of one logical core over a 60-second measurement |
| Active CPU at a replayed 150 WPM | <3% of one logical core at 120×40 |
| Default resident memory | ≤30 MiB; no growth with repeated ordinary tests |
| Long-session state | Bounded; demonstrate no linear growth after configured windows fill |
| Release binary with default data | Target ≤15 MiB stripped; report exact packaging assumptions |
| History query over 100,000 seeded results | p95 ≤100 ms for first page/current-profile summary |
| Network traffic | None during normal operation |

Record exact CPU, memory, OS, Rust/compiler profile, terminal/version, terminal geometry, local versus remote execution, dataset, history size, and enabled options. Establish dedicated local reference machines; do not turn noisy shared-CI absolute timing into a misleading pass/fail score.

Measure startup externally from process creation to the first complete usable frame; an internal timestamp inside `main` is not total startup. Report warm-cache and genuinely cold experiments separately. Exclude build time, not terminal initialization. Do not hide an unfinished content load behind a fast placeholder frame.

Measure reducer cost with deterministic traces; rendering CPU with an in-memory backend; terminal bytes/flushes with a pseudo-terminal; user-facing behavior in actual emulators. A test backend does not establish real terminal latency. “Input received to flush” is an application metric, not physical key-to-photon latency. Real key-to-photon testing is separate and includes keyboard, OS, terminal, and display delay.

Exercise 80×24, 120×40, and 200×60; ASCII and mixed-width text; healthy and slow terminal consumers; and a 1,000-event/second stress stream. Stress acceptance is ordered input or an explicit overload interruption, not a fabricated WPM benchmark. Test 1,000 successive runs for leaks and run long zen sessions until bounded buffers cycle.

Use release builds and measure LTO/codegen choices rather than setting `opt-level = "z"` because a smaller binary sounds faster. Keep portable release CPU targets; do not publish `target-cpu=native` binaries as generally compatible builds. Establish regression baselines; investigate repeatable ≥10% regressions on controlled hardware.

## 12. Persistence and privacy

Use SQLite for history, not a JSON file rewritten after every keystroke. A proposed logical schema:

```text
results
  id, created_at_utc, app_version, scoring_version
  profile_key, effective_test_spec, source_identity, seed, generator_version
  outcome, practice_reason, assistance_flags, integrity_flags
  elapsed_us, credited_units, retained_units
  attempts_total, attempts_correct, deletion_count
  final_correct, final_incorrect, final_extra, final_missed

samples
  result_id, bucket_index, duration_ms
  credited_units, retained_units, attempts_total, attempts_correct, errors

word_summaries                         # approved/bundled content only by default
  result_id, token_identity, attempts, errors, elapsed_us

schema_migrations
  version, applied_at_utc
```

Index history by `(profile_key, created_at_utc)` and index the fields used for profile-specific bests. Paginate results; do not load an entire lifetime of history into the UI. Version JSON exports separately from SQL schema.

Run migrations and commits in the storage worker after the first usable frame. Use WAL on supported local storage, a single application writer, bounded busy retries, and one transaction per finished result plus samples. SQLite documents concurrent-reader benefits and WAL's local/shared-memory constraints; do not assume a network home directory is a safe WAL deployment. [S13] Provide a non-WAL fallback or local data-path override.

Use durable transactions for acknowledged saves. A result is “saved” only after commit acknowledgement. If the database is busy, read-only, full, or corrupt, keep typing functional, retain a bounded number of pending snapshots in memory, and display an unsaved warning outside active typing. Never delete or overwrite a corrupt database automatically. Offer result export for recovery.

Normal exit waits for a bounded flush; interruption must not hang indefinitely on storage. If a pending result cannot be committed, restore the terminal and report that result as unsaved. Forced termination or power loss can still lose unacknowledged in-memory work; do not claim otherwise.

Default privacy: save summaries and one-second samples, not per-key traces or private custom/zen text. Custom history stores a content hash and a neutral label, not an absolute source path or text body. Built-in data can use token IDs and known content identities. Store full custom text or event traces only after explicit opt-in. `--private` suppresses result persistence and diagnostic text capture for the whole session. No telemetry, automatic update checks, cloud calls, or clipboard access.

Use platform-appropriate config/data directories and provide a command that prints resolved paths. Where supported, create private configuration/data files with user-only access. Treat imported text as untrusted display data: reject ESC/C0/C1 terminal controls except explicitly supported Tab/newline normalization; never pass raw input bytes through as terminal commands. Detect bidirectional-control characters and unsupported shaping requirements rather than silently rendering misleading content.

## 13. Command-line contract

Illustrative final interface:

```sh
clack

clack --time 60 --language english_1000 --punctuation
clack --words 50 --numbers
clack --quote --length medium
clack --file passage.txt
clack --text "a short practice passage"
clack --file src/main.rs --exact
a_command_that_outputs_text | clack --stdin

clack --preset focused
clack --time 30 --seed 42 --private
clack --once --time 30 --json

clack history --limit 20
clack stats --profile current --json
clack export --format csv
clack export --format jsonl

clack config path
clack config show --resolved
clack config validate
clack languages list
clack languages import ./my-pack
clack themes list
clack doctor
```

`--time` and `--words` imply their respective modes and are mutually exclusive. Reject incompatible source flags, invalid lengths, unsupported policies, and out-of-range values with exit code 2 before entering raw mode.

`--once` exits after one completed/failed test. `--json` requires `--once` for interactive tests and reserves stdout for exactly one versioned result object; no ANSI escapes, status messages, or banner may enter it. JSON `null` represents unavailable metrics; never emit NaN/Infinity. Normal completion and challenge failure both exit 0 and differ in the result outcome; runtime failure exits 1; explicit interruption exits 130.

For interactive runs, acquire a controlling-terminal input/output channel independently of redirected streams. `--stdin` consumes bounded UTF-8 source text, not subsequent keystrokes. On Unix use the controlling TTY; implement the equivalent console handling on Windows and verify it against the chosen backend. If no suitable interactive terminal exists, fail clearly and leave streams untouched. Noninteractive history/stats/export/help must work without a terminal.

`doctor` reports terminal size, effective input mode, color policy, data paths, and detected capabilities. Capability probing is explicit here; blocking terminal queries must not sit on the normal startup path. The primary workflow works with baseline input; enhanced keyboard reporting is optional, with documented configuration and complete restoration.

Provide shell completions and a man/help page generated from the same CLI definitions. Export files never include custom text unless explicitly requested and actually available.

## 14. Terminal reliability and supported behavior

Support modern local terminals on Linux, macOS, and Windows, with native builds for Linux x86-64/arm64, macOS x86-64/arm64, and Windows x86-64. Test actual terminal behavior rather than treating successful compilation as compatibility.

The minimum validation matrix includes one common emulator on each OS, macOS Terminal, Windows Terminal, a Kitty-protocol terminal, a terminal without keyboard enhancements, tmux, and an SSH session. SSH latency is environmental and is not covered by the local responsiveness budget.

Own terminal setup with a session guard. Track which capabilities were successfully changed: raw mode, alternate screen, cursor visibility/style, bracketed paste, focus reporting, and keyboard enhancements. Restore changed state on normal exit, error, recoverable panic, and supported termination/suspend paths. Do not perform unsafe cleanup operations directly inside an asynchronous signal handler; route supported signals through a safe shutdown path.

Use a panic hook and guard carefully; verify cleanup order. Restore the prior cursor style when queryable, otherwise reset to terminal default. Stop workers without letting them write after restoration. SIGKILL, terminal emulator failure, and machine power loss cannot be made cleanup-safe by ordinary application code.

No mouse capture by default. On unsupported capability/color/glyph cases, degrade to a readable baseline. Invalid terminal geometry must never panic, index outside a buffer, or corrupt the user's shell screen.

## 15. Acceptance tests

| Test | Required assertion |
|---|---|
| First input | First printable eligible character both starts timing and appears exactly once, including when wrong. No countdown or focus click. |
| Focus transition | Text coordinates before/after first input are unchanged; only configured UI disappears. |
| Correction | `c a x Backspace t Space` against `cat ` produces 80% attempt accuracy and four retained/credited units. |
| Submission | Early Space creates missed target units, not imaginary typed characters; duplicate empty Space never skips a whole token. |
| Partial endpoint | Correct active prefix contributes exactly its entered units; incorrect active prefix contributes no whole-token credit. |
| Final word | A correct final word completes without trailing Space. An incorrect final word remains repairable until explicit allowed submission. |
| Exact text | Tabs/newlines follow literal policy; the final error can be corrected before F5 confirmation; early F5 is incomplete practice. |
| Deadline | With a 30-second limit, event receipt at 29.999 seconds counts; receipt at 30.000 does not. Duration remains 30 seconds under delayed rendering. |
| Restart boundary | Queued letters/release events from an old test cannot enter the new test; repeated sample is labeled practice. |
| Backtracking | Reopening a token reverses exactly its old contribution; historical wrong attempts never disappear. |
| Difficulty | Master fails on definite incorrect input; expert on incorrect token commit; invalid stop/backspace combinations are rejected. |
| Unicode | Composed/decomposed prose accents compare appropriately; combining input is not double-counted; Backspace deletes a grapheme; wide characters occupy correct cells. |
| Paste | Bracketed paste is rejected atomically; does not start Ready; marks an active result practice-only. |
| Resize/suspend | Resize preserves logical position and timing; too-small/suspended runs are interrupted, not paused records. |
| Storage failure | Disk-full/read-only/locked/corrupt database never blocks typing; unsaved state is honest; no destructive “repair.” |
| Privacy | Private runs write no result/text trace; custom/zen text does not appear in default history, logs, or exports. |
| Personal best | Different packs/rules/scoring versions cannot share a record category; theme changes do not split one. |
| Determinism | Fixed test spec, generator, seed, and pack revision yield identical text and replayed scores across platforms. |
| Terminal lifecycle | Normal exit, panic/error injection, and supported signal paths restore shell usability and cursor state. |
| Shell composition | Piped content remains distinct from keyboard input; `--once --json` emits one clean parseable object on stdout. |
| Stress/bounds | Sustained input preserves order or explicitly interrupts; buffers remain bounded; no silent event loss. |

Unit-test the pure reducer and formula fixtures. Property-test arbitrary input/edit sequences for cursor bounds, nonnegative counters, reversibility of retained-output deltas, finite metrics, and terminal-independent scoring. Snapshot all states at 40×10, 80×24, and 120×40 using color and monochrome themes. Use pseudo-terminal integration tests for escape output and lifecycle; manually verify actual emulators for appearance and input behavior.

Required CI gates: formatting, warning-free linting for supported configurations, unit/property tests, snapshot/integration tests, release builds, and dependency/license review. Performance reports must be checked against controlled-machine baselines rather than invented from microbenchmarks alone.

## 16. Build order and definition of done

**Stage A — Typing kernel.** Implement immutable test specs, matching, timing, edits, scoring, deterministic content, and golden test vectors. No polished UI yet. Gate: all score/boundary fixtures and property tests pass without terminal dependencies.

**Stage B — Minimal vertical slice.** Add terminal lifecycle, input reader, event-driven scheduling, the custom typing viewport, default timed test, results, and immediate restart. Gate: first-keystroke behavior, no text jump, clean exit, and initial latency/startup benchmarks on real terminals.

**Stage C — Full usable product.** Add remaining modes, palette/settings/presets, themes, correction/challenge rules, practice, review, history, and exports. Gate: these use the same engine/setting registry; no duplicate scoring implementations or persistent typing-screen clutter.

**Stage D — Release hardening.** Complete Unicode/terminal/platform matrix, storage recovery, privacy tests, stress tests, licensing manifests, packaging, help/completions, and performance regression baselines. Gate: documented 1.0 behaviors pass; known unsupported cases are explicit.

Deliver the complete source repository, locked dependencies, versioned configuration/export formats, approved content manifests, automated tests and benchmark harness, benchmark report with actual measurements, terminal test matrix, and installable release binaries.

The product is done when it is pleasant enough to repeatedly launch without thinking about its interface: the first key always counts, the text never moves unexpectedly, the app never pauses to save a score, useful settings are easy to find, and the default active view remains mostly text and empty space.

## 17. Research source register

Primary sources reviewed for this specification. Source URLs are retained as code-form references for an implementation agent. The Monkeytype code snapshot inspected is `91bd24bb8513785c7364cbea29296ff7adafac41`; it is a research anchor, not a promise that a moving branch remains unchanged.

- **S1 — Monkeytype defaults:** `https://github.com/monkeytypegame/monkeytype/blob/91bd24bb8513785c7364cbea29296ff7adafac41/frontend/src/ts/constants/default-config.ts`
- **S2 — Monkeytype metric explanations and event statistics:** `https://monkeytype.com/about` and `https://github.com/monkeytypegame/monkeytype/blob/91bd24bb8513785c7364cbea29296ff7adafac41/frontend/src/ts/test/events/stats.ts`
- **S3 — Monkeytype failure/completion logic:** `https://github.com/monkeytypegame/monkeytype/blob/91bd24bb8513785c7364cbea29296ff7adafac41/frontend/src/ts/input/helpers/fail-or-finish.ts`
- **S4 — ttyper primary README:** `https://github.com/max-niederman/ttyper/blob/main/README.md`
- **S5 — tt primary repository:** `https://github.com/lemnos/tt`
- **S6 — Ratatui rendering and terminal documentation:** `https://ratatui.rs/concepts/rendering/` and `https://docs.rs/ratatui/latest/ratatui/struct.Terminal.html`
- **S7 — Kitty keyboard protocol, including legacy ambiguities:** `https://sw.kovidgoyal.net/kitty/keyboard-protocol/`
- **S8 — Crossterm event documentation:** `https://docs.rs/crossterm/latest/crossterm/event/index.html`
- **S9 — Rust Instant documentation:** `https://doc.rust-lang.org/std/time/struct.Instant.html`
- **S10 — Unicode segmentation crate documentation:** `https://docs.rs/unicode-segmentation/latest/unicode_segmentation/`
- **S11 — Unicode width crate documentation:** `https://docs.rs/unicode-width/latest/unicode_width/`
- **S12 — Unicode normalization crate documentation:** `https://docs.rs/unicode-normalization/latest/unicode_normalization/`
- **S13 — SQLite WAL documentation:** `https://www.sqlite.org/wal.html`
- **S14 — NO_COLOR convention:** `https://no-color.org/`
- **S15 — Monkeytype repository/license:** `https://github.com/monkeytypegame/monkeytype`
