# Settings and presets

Configuration is typed, versioned TOML. `clack config path` prints the resolved
configuration and data locations; `clack config show --resolved` prints effective
settings, and `clack config validate` validates a file without entering terminal raw
mode. `--config PATH` and `--data-dir PATH` select explicit locations. The file
limit is 256 KiB. UTF-8 errors, unknown fields, unsupported values, and conflicting
rules produce a path/value diagnostic.

Initial precedence is built-in defaults, user configuration, an explicitly chosen
preset, then CLI flags. A later explicit setting edit changes the effective
session. Ordinary CLI overrides do not rewrite the file. A persisted setting edit
writes only its edited fields: launching with `--time 120`, then changing the theme,
does not silently save 120 seconds as the default. Applying a preset from settings
persists its explicitly named fields even when a CLI override already made the
effective value equal. Save current as default intentionally saves the effective
session.

The defaults are a 30-second English 200 timed test, normal difficulty, mistakes
backspace, no punctuation or numbers, timer-only status, terminal-inherited
colors, automatic focus, three text rows and an automatic width capped at 72 cells.
The required smaller-terminal limits still apply. Auto width and explicit 72 have
the same geometry at ordinary screen sizes; the former records the automatic
policy explicitly.

```toml
schema_version = 1

[test]
mode = "time"
seconds = 30
words = 50
language = "english_200"
punctuation = false
numbers = false
policy = "prose"
normalize_exact = false
completion = "confirm"
quote_length = "short"
# quote_id = "a-stable-local-id"
# file = "/path/to/private-passage.txt"

[appearance]
theme = "terminal"
focus = "auto"
width = "auto"
alignment = "center"
lines = 3
line_spacing = 0
caret = "bar"
tab_stop = 4
color = "auto"
error_underline = true
typed_bold = false
ascii_markers = false

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
# minimum_wpm = 40.0
# minimum_accuracy = 95.0

[practice]
pace = "off"
auto_indent = false
selection = "missed"

[privacy]
save_results = true
store_custom_text = false
store_event_trace = false
private_session = false

[workflow]
favorite_themes = []
favorite_packs = []
result_details = false

[workflow.bindings]
# Safe aliases retain the ordinary baseline shortcuts:
# new_sample = "ctrl+n"
# help = "f1"

[storage]
journal_mode = "auto"
pending_limit = 16
```

`enhanced_keyboard = false` is a root key. Set it before the first table header if
adding it to this example. Enabling enhancements is explicit; essential baseline
commands remain reachable.

The registry in `src/settings.rs` owns setting paths, groups, labels, descriptions,
value kinds, ordinary/advanced exposure, and score-impact metadata. It exposes the
canonical typed defaults to adapters, so defaults are not separately duplicated
in a palette schema. Configuration validation uses that same registry plus the
pure engine's cross-field rule validation. The acceptance suite checks that every
serialized leaf is registered and that CLI choice lists match registry choices.

| Group | Settings and supported values |
|---|---|
| Test | `mode`: time/words/quote/custom/code/zen; `seconds`:1–3600; `words`:1–10000; stable `language`; independent `punctuation`/`numbers`; `policy`:prose/exact; `normalize_exact`; `completion`:confirm/auto; `quote_length`:short/medium/long/extended; optional `quote_id`/`file`. |
| Display | `theme`:terminal/dark/light/warm/high_contrast; `focus`:auto/always/off; `width`:auto or40–120; `alignment`:center/top; `lines`:1–5; `line_spacing`:0/1; `caret`:bar/block/underline; `tab_stop`:1–16; `color`:auto/always/never; independent error underline, typed bold, and ASCII markers. |
| Status | Independent `progress`, `wpm`, `accuracy`; `speed_unit`:wpm/cpm. Full focus hides all secondary status. |
| Rules | `difficulty`:normal/expert/master; `backspace`:mistakes/current/full/none; `stop_on_error`:off/letter/word; `blind`; optional positive `minimum_wpm`≤1000 and `minimum_accuracy`≤100. |
| Practice | `pace`:off/personal_best or a string containing fixed1–1000WPM; `auto_indent` for exact input; `selection`:missed/slow. Practice selection does not change the specified25-word or slow-quartile algorithms. |
| Workflow | Named data presets; favorite themes/packs; validated command aliases; `result_details`. |
| Privacy | `save_results`, explicit custom-text/trace opt-ins, `private_session`. CLI `--private` also sets the private-session flag and suppresses save/text/trace settings. The application keeps the session private once activated. |
| Storage | `journal_mode`:auto/wal/delete; `pending_limit`:1–64. These are advanced settings. |
| Terminal | Root `enhanced_keyboard` boolean; defaultfalse. |

The six advanced generator settings live under `[test.generator_parameters]`:

```toml
[test.generator_parameters]
sentence_min_words = 4
sentence_max_words = 12
comma_percent = 10
number_percent = 10
number_min_digits = 1
number_max_digits = 4
```

Sentence bounds are1–64 with minimum≤maximum; probabilities are0–100; digit
bounds are1–4 with minimum≤maximum. All fields are validated even while their
modifier is disabled. Only effective generation conditions split score profiles.
Quotes and supplied passages are not rewritten by random-word modifiers.

Word-stop conflicts with both no-backspace and expert difficulty. Code requires
exact policy, while timed/word/quote tests require prose. Auto-indent requires
exact text. A multi-field edit is validated as a whole: changing mode to code and
policy to exact succeeds together without committing an invalid intermediate
configuration.

## Quick test setup

Open **Ctrl-T**, **F6**, or **Test setup** in the command palette. The ready and
results screens show the shortcut. Settings are staged together; **Enter** saves
only the changed fields and prepares a ready test, and **Esc** discards the draft.
Opening setup during a running test aborts that run, as opening commands does.
Cancel then returns a fresh test with the original settings.

- **15s → 30s:** Ctrl-T, Right, Enter.
- **Time → Words:** Ctrl-T, w, Enter. Use Left/Right first to choose a word count.
- **Custom length:** type the number on the Time or Words row, then Enter.
- **Rows:** Tab/Shift-Tab or Up/Down; **choices:** Left/Right. Space toggles modifiers.
- **Mode shortcuts:** t = Time, w = Words, q = Quote, c = Custom, d = Code, z = Zen.
  In the File row, these letters are ordinary path input; move to Mode to use shortcuts.

Timed presets are 15/30/60/120 seconds; word presets are 10/25/50/100 words.
Other supported values remain visible and editable. Time and Words retain their
own last applied lengths. Quotes show short/medium/long/extended; changing the
category clears an explicitly pinned quote ID. Custom and Code expose a source
file field; empty uses the initial source, or bundled content for Code. Invalid
values, unavailable sources, and failed saves leave setup open for correction.
Punctuation and numbers appear only for generated Time/Words tests.

The full command palette still provides language packs, scoring rules, appearance,
presets, and advanced settings. `workflow.bindings.test_setup` can add a safe alias.

## Data presets

Three names are built in and reserved:

- `default` restores canonical settings and retains saved preset definitions.
- `focused` selects full focus and hides each status field.
- `code` selects code mode, exact policy and explicit completion confirmation.

Custom presets are typed patches:

```toml
[presets.training.test]
mode = "words"
policy = "prose"
words = 50

[presets.training.status]
wpm = true
accuracy = true

[presets.training.appearance]
theme = "high_contrast"
```

`clack --preset training` applies that data before CLI overrides. Names contain1–64
ASCII letters, digits, underscores or hyphens. At most64 custom presets are
accepted. Unknown keys and nested preset definitions are rejected. There is no
shell command, script, executable, network fetch, or plugin mechanism. A preset
with an invalid effective combination is rejected without changing the session.
Saving a named preset preserves other definitions and their comments.

## Command aliases

`workflow.bindings` maps a command name to one additional chord. Baseline
shortcuts are retained. Safe chords are `esc`, `f1`–`f24`, modified function keys,
or an unambiguous `ctrl+letter`. Printable characters, Tab and Enter cannot be
commandeered from typing. Ctrl-H/I/J/M are rejected because baseline terminals
can report them identically to Backspace/Tab/Enter. Duplicate modifiers, unknown
commands, and collisions with other baseline/custom actions are rejected.

Command names are `palette`, `test_setup`, `new_sample`, `repeat_sample`, `next_sample`,
`finish`, `delete_word`, `quit`, `details`, `practice`, `practice_missed`,
`practice_slow`, `history`, `help`, `config`, `save_default`, `export`, and
`retry_save`. Every command can also be exposed by the palette, which avoids
depending on terminals delivering a particular function key.

Results-only commands do not consume content during Ready/Running. Release
events never execute actions. Ctrl-C/R/P/W retain optional Shift/uppercase
compatibility; Ctrl-Alt/AltGr text is not mistaken for those essential commands.
The application and reader share the same validated `Bindings` object so command
recognition cannot accidentally start the timer before the app handles a chord.

## File editing guarantees

An edit first validates both the effective session and the saved baseline with
only the requested patch. If CLI overrides make an edit valid only for the
session, the operation explains the conflict and commits neither version. It
never quietly saves unrelated CLI values to make the disk configuration valid.

TOML editing preserves untouched fields, table comments, inline-table structure,
and comments on replaced scalar values. Writes use a unique sibling temporary
file, flush its bytes, then atomically replace the destination. Configuration
files are created0600 and new containing directories0700 on Unix. The editor
checks for a changed destination before replacement. Temporary files are removed
on a failed write. The rename is the commit point; containing-directory sync is
best effort on filesystems that support it.

An existing malformed, oversized, non-UTF-8, or nonregular config is preserved;
editing and Save current as default refuse to overwrite it automatically. A
symbolic-link config may be read, but mutation asks that its actual target path
be selected explicitly. Automatic launch may report a broken default config and
use safe runtime defaults; an explicitly supplied invalid config fails before
raw mode. No in-memory setting changes are published on a failed persistence
operation.

## Adapter API and validation

`Setting::{value,default_value,parse,choices,next_value}` provides setting display
and editing metadata. `Edit::parse(path, raw)` accepts one typed value; optional
threshold/source fields support `unset`. `Config::edited` is a no-I/O transaction.
`persist_edits`, `persist_preset`, `save_preset`, and `save_defaults` are explicit
file mutations. `preset_names` and `preset_edits` expose preset actions without
duplicating their definitions in UI code.

`cargo test --offline --test settings_acceptance` passed20 fixtures on macOS
arm64: registry/CLI consistency, canonical defaults, advanced bounds, precedence,
transactional cross-field edits, comment and inline-table preservation, CLI
override isolation, malformed-file preservation, data presets, bindings,
private-session fields, size bounds, Unix permissions, and symlink preservation.
Native Windows/Linux file behavior and final integrated palette interactions
remain separate validation requirements; these unit/integration results do not
claim those unexecuted surfaces.
