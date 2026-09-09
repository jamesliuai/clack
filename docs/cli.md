# Command-line interface

`clack` opens the default 30-second English 200 test immediately. Configuration,
source validation and controlling-terminal acquisition finish before raw mode.
The first eligible key starts the test. All commands below are local and need no
account or network.

```sh
clack --time 60 --language english_1000 --punctuation
clack --words 50 --numbers
clack --quote --length medium
clack --quote-id a-local-quote-id
clack --file passage.txt
clack --text "a short practice passage"
clack --file src/main.rs --exact
clack --code
clack --code --file src/main.rs
printf 'a synthetic passage\n' | clack --stdin
clack --zen
clack --preset focused
clack --time 30 --seed 42 --private
clack --once --time 30 --json > result.json
```

`--text` is a literal argument and may enter your shell history; `--file` or
`--stdin` is preferable for private passages. Their bytes supply source text,
never scored keyboard events. Keyboard input and interactive output use the
controlling terminal even when stdin/stdout are redirected. A missing controlling
terminal fails clearly before source stdin is consumed.

Time accepts whole seconds 1–3600; words accepts 1–10000. Their flags imply the mode
and are mutually exclusive. Quote lengths are short/medium/long/extended. Code
and custom exact text preserve tabs/newlines; `--tab-stop N` accepts 1–16 display
cells without changing logical units, for example `--tab-stop 8`. `--ascii-markers` enables
simple marker fallback. `--completion confirm` is the default for exact text;
F5 confirms after tail correction, and early F5 is incomplete practice.

Source selectors that describe different passages are rejected together. Code
may be paired with file/text/stdin to select exact treatment of that source.
`--quote --quote-id ID` is an accepted redundant explicit selector. Exact policy
cannot be applied to timed/random-word/quote modes. `--normalize-exact` requires
exact policy. Punctuation and number modifiers affect generated words, preserving
supplied quotes/custom passages.

Independent options include difficulty/backspace/stop-on-error, blind display,
minimum WPM/accuracy, fixed or matching-PB pace, auto-indent, theme/focus/width/
rows/alignment/caret/color, separate live status fields, and WPM/CPM units. Boolean
options that accept an explicit value use `--option=false` or `--option=true`.
`clack --help` is generated from the actual Clap definitions. Typed settings and
advanced generator/storage options are documented in [settings.md](settings.md).

## Results and exit status

`--once` exits after one completed or failed test. Interactive `--json` requires
`--once` and reserves stdout for exactly one version 1 result object. It includes
immutable effective conditions, source identity, raw counts, elapsed time,
metrics, samples, outcome, integrity flags and record eligibility. Unavailable
metrics are JSON null; output never uses NaN/Infinity or terminal escapes.

| Status | Meaning |
|---|---|
|0|Completed, challenge-failed, or explicitly incomplete practice result; inspect `outcome`. Successful noninteractive command.|
|1|Runtime I/O/input/output/storage failure or a safely handled internal error.|
|2|Invalid arguments, source, configuration, filter or imported data, before raw mode.|
|130|Explicit interruption such as Ctrl-C or a handled termination/suspend signal.|

If interrupted before any scored result exists, stdout remains empty and a
diagnostic explains that no result is available. After terminal restoration,
unacknowledged storage results are reported by their actual count on stderr.
They are not described as saved. The interactive recovery-export action can
preserve pending snapshots before leaving an unavailable-storage session.

`--private` marks the entire session private and suppresses result persistence,
custom text and diagnostic traces. `--no-save` suppresses result persistence.
Neither option changes scoring formulas. Diagnostic performance observations
require explicit `--benchmark-output NEW_PATH`, are bounded/numeric, and are
written after terminal restoration; that option applies only to interactive runs.

## History, statistics and export

These commands need no terminal and open history read-only. Missing history is
an empty result set; commands create no database, schema, migration or data
directory. Corrupt or unsupported databases are preserved and reported neutrally.

```sh
clack history --limit 20
clack history --limit 20 --offset 20 --json
clack history --profile all --mode time --language english_200 --outcome complete
clack history --profile all --classification practice
clack history --profile all --classification paste_attempted --outcome interrupted
clack history --profile all --from 2026-09-01 --to 2026-09-04
clack stats --profile current --json
clack stats --profile all --mode words --outcome complete
clack export --format csv
clack export --format jsonl
clack export --format json --output new-results.json
clack export --format jsonl --profile all --include-text
```

History and stats default to the exact current matching profile; export defaults
to all profiles. `--profile` accepts `current`, `all`, or a 64-digit profile key.
Mode/language/outcome/classification/date filters further narrow that scope; use `--profile all`
when comparing multiple modes or conditions. History limits are1–1000, with a
bounded offset and returned `next_offset` for pagination. JSON history wraps
`export_version`, `filter`, `results`, and `next_offset`. JSON stats wraps
`export_version`, `filter`, and `statistics`.

`--classification` accepts `standard`, `practice`, `paste_attempted` and
`assisted_code`. Standard means a saved result eligible for standard personal
bests. Practice includes explicit seeds, repeats/named practice, pacing,
auto-indent, paste attempts, Zen and incomplete submissions. Paste-attempted
and assisted-code selections are overlapping subsets of practice. Classification
is independent of outcome: an ordinary failed, aborted or interrupted attempt
does not become practice merely because it was unsuccessful. A zero-duration
unassisted completion can match neither standard nor practice and remains
available in unfiltered history. These filters apply consistently to history,
statistics and every export format.

Dates use UTC `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS[.sss]Z`. A date-only `--to` includes
that whole day by using the following midnight as the exclusive boundary.
Timestamp `--to` is exclusive. Invalid Gregorian dates and reversed ranges fail
with exit2 before database access. Statistics expose result/sample counts, raw
weighted denominators, aggregate WPM/raw WPM and attempt accuracy; they do not
average differently sized run speeds without weighting.

Current profile preparation includes actual pack ID/revision/hash, generator
version and effective rules. Root test options precede the command:

```sh
clack --time 60 --language english_1000 stats --profile current --json
clack --file passage.txt history --profile current --json
clack --quote-id a-local-quote-id history --profile current --json
```

A new command process cannot infer which randomly selected quote was on a prior
screen, so quote `current` requires a stable quote ID or an explicit profile key.
`--profile all` remains available without choosing a quote. Matching-personal-best
pace resolves only the indexed best for the prepared unpaced profile; if none
exists, choose a fixed pace or explicit profile key. Noninteractive commands
reject `--stdin` without reading it; use file/literal source identity when needed.

Export streams one consistent read transaction. JSON is a versioned envelope,
JSONL has one versioned record per line, and CSV has a versioned header and safe
text escaping. The encoding is selected by `--format`; the global `--json` switch
does not override it. A file export uses a new0600 file on Unix and refuses an
existing destination. It leaves stdout empty, reports the exported count on
stderr, and removes an incomplete newly created file if exporting fails.

Every export passes through the storage privacy filter. Full source text, entered
strings and traces require `--include-text` **and** previous explicit persistence
of those fields. The command never invents text discarded by privacy defaults.
Custom/zen/source paths are absent from ordinary history/export data. Stored
partial Zen or prepared timed content carries an explicit text-scope label.

## Configuration, packs and diagnostics

```sh
clack config path
clack config show
clack --time 60 config show --resolved --json
clack config validate
clack languages list --json
clack languages import ./my-pack --json
clack themes list
clack doctor --json
clack doctor --probe --json
```

`config show` displays saved/default values, while `--resolved` includes invoked
preset and CLI overrides. It does not write them. Explicit invalid configs fail
before raw mode and remain untouched. Help/version, config-path discovery,
completions and man output remain available even when a configuration is broken.
`--config PATH` and `--data-dir PATH` are global options available on subcommands.

Import directories contain `words.txt` and `metadata.json` using the format in
[content.md](content.md). Validation completes before creating the destination.
Only normalized word data and validated metadata are copied; scripts or other
files are never executed or installed. The complete temporary directory is
published together. New directories use0700 and files0600 on Unix.

Bundled IDs cannot be shadowed. Reimporting identical metadata/content is an
idempotent success; an existing ID with different content or revision is preserved
and rejected. Choose a new ID for a distinct pack revision. Interrupted temporary
imports have hidden names, are not listed as installed packs, and cannot shadow
a valid installed pack. Pack IDs are validated before constructing destination
paths.

Doctor reports OS-observed terminal availability/size, configured input/color
policy, detected environment color depth, resolved data paths, selected journal
mode and compiled backend capabilities. Protocol support that has not been
observed is JSON null; requested enhancements are not presented as negotiated
support. Ordinary doctor emits no raw-mode changes or startup cursor-position
queries. Explicit `--probe` owns the controlling terminal for at most 400 ms,
asks for keyboard-enhancement and primary-device responses, and restores raw
state before reporting. It never draws an alternate screen or writes terminal
escapes to stdout. No response means unknown support, not a fabricated negative.
The native Windows console backend reports that this protocol probe is
unsupported. Interrupted probes return 130 after their report. Compiled build
metadata records architecture, OS, debug assertions and test-hook presence.
Actual-emulator input/color/IME behavior is documented separately in
[terminal-matrix.md](terminal-matrix.md).

## Generated documentation and validation

```sh
clack completions bash > clack.bash
clack completions zsh > _clack
clack completions fish > clack.fish
clack man > clack.1
```

Completions and man/help derive from the same CLI definitions. Generating them
does not parse source stdin, access history or create configuration directories.

`cargo test --offline --test cli_acceptance` passed21 executable CLI tests after
adding missing-source all-profile queries and handled closed-output paths;
the Rust1.88 rerun also passed all21. Settings and sample suites separately
passed20 fixtures each. Fixtures
use the real executable, engine snapshots and SQLite transactions. They cover
no-terminal empty/read-only commands, filtering/pagination/weighted stats, private
exports, import validation/publication/idempotence/permissions, neutral corruption
errors, source/policy argument checks, defaults precedence, and generated help.
Unix terminal-ordering and interactive JSON/cleanup behavior have separate PTY
evidence. Native Windows/Linux console and filesystem checks remain external
until actually executed.
