# clack

`clack` is a local terminal typing test. Open it, type, see the result, and start
again. The first eligible key starts the clock. The default test lasts 30 seconds;
the active screen keeps the target text, a steady caret and a small timer.

Version 1.0.0 uses stable Rust, with Rust **1.88** as its minimum compiler version.
It needs no account, service, browser, telemetry or network connection at runtime.
The application has its own scoring specification and branding; it does not
claim score-for-score compatibility with another typing product.

```sh
clack
clack --words 50 --punctuation
clack --quote --length medium
clack --file passage.txt
clack --code --file src/main.rs
printf 'a short passage\n' | clack --stdin
clack --zen --private
clack --once --json > result.json
```

Press **Ctrl-T** (or **F6**) for test setup. The current length is selected:
use **Left/Right**, then **Enter** to apply. Press **w** for Words or **t** for
Time; each mode remembers its last applied length. The screen shows common
presets, accepts custom durations and word counts, and includes quote lengths,
source files, punctuation and numbers. **Tab/Up/Down** moves between rows;
**Esc** discards pending changes. Opening setup during a test ends that run.
The clock starts only when you return to the test and begin typing.

Use `Esc` or `Ctrl-P` for commands, `Ctrl-R` for a new test, `F5` to confirm exact
text or finish Zen, and `Ctrl-C` to quit. The command palette exposes settings,
presets, history, result review, mistake practice and alternate actions when a
terminal intercepts a function key. Repeating the same sample is practice.

Modes include timed words, fixed word counts, quotes, custom prose, exact/code
passages, and target-free Zen. Display settings are separate from scoring rules.
The engine counts extended grapheme clusters, handles canonical prose accents,
and preserves literal exact-text tabs and newlines. Mistyped attempts remain in
accuracy history after correction. Details are in [scoring](SPEC.md#7-scoring-specification),
[settings](docs/settings.md), and [the command-line guide](docs/cli.md).

## Install

Choose the archive for your operating system and architecture from `dist/`.
Verify its sibling `.sha256` file with `shasum -a 256 -c FILE.sha256` on macOS,
`sha256sum -c FILE.sha256` on Linux, or `Get-FileHash -Algorithm SHA256 FILE.zip`
on PowerShell. Extract the archive into a new directory. Its `manifest.json`
records every file's size and checksum, the build command, source identity and
which executable checks actually ran.

The standalone executable is `bin/clack` (`bin/clack.exe` on Windows). Copy it to a
directory on your PATH, or use the optional Python 3.11+ installer:

```sh
python3 install.py --prefix "$HOME/.local"
python3 install.py --prefix "$HOME/.local" --uninstall
```

The installer verifies all package checksums, refuses existing binaries, and
keeps its documentation/man/completion/license bundle under `share/clack`. It never
edits PATH, configuration or history. Uninstall removes only its registered,
unchanged files. Keep the extracted installer to run uninstall. Windows users
can run the same helper with `py -3 install.py --prefix "$env:LOCALAPPDATA\clack"`
in PowerShell, or copy the executable manually. An upgrade first requires an
explicit uninstall or a different prefix.

Build from the complete source archive or this checkout with a C compiler and
Rust 1.88 or newer:

```sh
cargo build --locked --release
./target/release/clack
```

Use the original `Cargo.toml` and included `vendor/crossterm` fork together.
`cargo install --path . --locked` is supported; publishing this source as a
normalized crates.io package is disabled because that would discard the root
dependency patch. Build tools may fetch the locked crates once; the executable
does not fetch content or code. See [release instructions](docs/release.md) for
offline builds, deterministic archives, the five-target CI matrix and checks.

## Local data and privacy

`clack config path` prints the platform-specific paths. Configuration is versioned
TOML and history is versioned SQLite. The storage worker saves after a result
without blocking typing; an unsaved result remains visibly unsaved and can be
retried or exported. Broken databases are preserved rather than silently repaired.
Use `--config PATH` and `--data-dir PATH` to isolate an installation.

The renamed app uses `clack` for its default configuration and data directories.
To reuse data from an older installation, pass its existing configuration file
with `--config PATH` and its data directory with `--data-dir PATH`.
Historical validation records in `docs/measurements` retain their original names.

`--private` writes no result or text trace for the entire session. Default saved
custom/Zen results contain metrics and hashes, not private passages or entered
text. Export requires both explicit text inclusion and previous opt-in storage
before any private text can appear. No result data is uploaded.

On Windows, new private files use protected user-only ACLs before content is
written. SQLite also requires a protected private data directory and private
existing database/sidecar files. If an existing shared or inherited directory is
refused, choose a new, nonexistent `--data-dir` that clack can create privately.
Existing data and unrelated directory permissions are preserved. The exact
Windows permission policy is documented in the [privacy adapter review](vendor/clack-private-fs/SAFETY.md).

```sh
clack history --profile all --json
clack stats --profile current --json
clack export --format csv --output new-results.csv
clack doctor --json
clack doctor --probe --json
```

The optional doctor probe spends at most 400 ms waiting for capability responses
on the controlling terminal, restores raw mode before returning, and does not
run during ordinary startup. Terminal protocol limits, unsupported composition
and bidirectional text behavior, and cleanup boundaries are documented in
[the terminal matrix](docs/terminal-matrix.md).

## Verification and release status

Automated reducer/property, Unicode, configuration, storage, CLI, snapshots and
PTY suites are included. [The coverage ledger](docs/coverage.md) tracks every
specification requirement separately from its evidence. [Measurements](docs/measurements)
contain actual observed results and limits; early Stage B measurements are
explicitly identified as preceding storage integration. Cross-compilation and
PTY checks do not establish native Windows/Linux runtime or actual-emulator
appearance. Outstanding external checks remain explicit in the ledger and
terminal matrix.

Application code is [MIT licensed](LICENSE). Dependency notices, the MPL-covered
source, content provenance and Unicode terms are included in
[third-party notices](THIRD-PARTY-NOTICES.md). The complete implementation request
is preserved as [SPEC.md](SPEC.md).
