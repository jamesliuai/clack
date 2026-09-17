# Clack

**[Monkeytype](https://monkeytype.com/) for the terminal.**

[![crates.io](https://img.shields.io/crates/v/clack-typing.svg)](https://crates.io/crates/clack-typing)
[![CI](https://github.com/jamesliuai/clack/actions/workflows/ci.yml/badge.svg)](https://github.com/jamesliuai/clack/actions/workflows/ci.yml)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](Cargo.toml)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A focused typing test built in Rust. Run `clack`, start typing, and see your
results. Practice with timed tests, word counts, quotes, code, or your own text.
Customize the experience and track your progress locally—no account, telemetry,
or runtime network connection required.

[Install](#install) · [Usage](docs/usage.md) · [Settings](docs/settings.md) · [Contributing](CONTRIBUTING.md)

## Install

With Rust 1.88 or newer and a platform C compiler installed:

```sh
cargo install clack-typing --locked
clack
```

The package is **`clack-typing`**; the executable is **`clack`**. Make sure Cargo's
binary directory (usually `~/.cargo/bin`) is on your `PATH`. Clack supports macOS,
Linux, and Windows. SQLite is bundled during compilation.

**Upgrading from 0.1?** Version 1.0 has a new configuration schema and SQLite
history. Read the [upgrade notes](docs/upgrading.md) before reusing old data.

<details>
<summary>Build from source</summary>

```sh
git clone https://github.com/jamesliuai/clack.git
cd clack
cargo install --locked --path .
```

Keep the full checkout, including `vendor/`. For verified binary/source archive
construction and the optional installer, see [release instructions](docs/release.md).

</details>

## Start typing

The default is a 30-second English test. The first eligible key starts the clock.

```sh
clack --time 60                      # One minute
clack --words 50 --punctuation       # A fixed word count
clack --quote --length medium        # A bundled quote
clack --file passage.txt             # Your own prose
clack --code --file src/main.rs      # Exact code, tabs, and newlines
clack --zen --private                # Free typing without saving results
clack --once --json > result.json    # One test with machine-readable output
```

Use `clack --help` for all options. On Unix, pipe custom text with
`printf 'a short passage\n' | clack --stdin`.

## What you can do

- **Practice your way.** Timed words, fixed word counts, quotes, custom prose,
  exact/code passages, and target-free Zen.
- **Keep your focus.** A compact prompt, steady caret, configurable focus and
  status display, themes, and keyboard-driven settings.
- **Change a test quickly.** Choose time or words, common presets or custom
  lengths, source files, punctuation, and numbers from test setup.
- **Learn from results.** Local history, statistics, personal best comparisons,
  result review, mistake practice, and JSON/CSV export.
- **Type beyond ASCII.** Extended grapheme clusters, canonical prose accents,
  and literal tabs and newlines in exact text. See the
  [terminal matrix](docs/terminal-matrix.md) for composition and display limits.

## Keyboard controls

| Key | Action |
| --- | --- |
| `Ctrl+T` / `F6` | Open test setup |
| `Left` / `Right`, then `Enter` | Choose and apply a setup value |
| `w` / `t` in setup | Switch between words and time |
| `Tab` / `Up` / `Down` in setup | Move between rows |
| `Esc` in setup | Discard pending changes |
| `Esc` / `Ctrl+P` in the test | Open the command palette |
| `Ctrl+R` | Start a new test |
| `F5` | Confirm exact text or finish Zen |
| `Ctrl+C` | Quit and restore the terminal |

Opening setup during a test ends that run. The clock starts again only after
returning to the test and typing. Repeating the same sample counts as practice.
The command palette offers alternatives when a terminal intercepts a function key.

## Settings and local data

```sh
clack config path
clack config show --resolved
clack config validate
clack themes list
clack history --profile all --json
clack stats --profile current --json
clack export --format csv --output new-results.csv
```

Configuration is versioned TOML; history is versioned SQLite. Use the command
palette for settings and presets, or read the [settings reference](docs/settings.md).
`--config PATH` and `--data-dir PATH` select separate locations.

`--private` saves no result or text trace for the session. Saved custom/Zen
results contain metrics and hashes by default, rather than private passages or
entered text. Text export requires explicit inclusion and prior opt-in storage.
Nothing is uploaded. See [storage and privacy](docs/storage.md) for the full contract.

## Documentation and help

- [Usage guide](docs/usage.md) and [CLI reference](docs/cli.md)
- [Settings and presets](docs/settings.md)
- [Scoring specification](SPEC.md#7-scoring-specification)
- [Terminal compatibility and troubleshooting](docs/terminal-matrix.md)
- [Content provenance](docs/content.md) and [dependency review](docs/dependency-review.md)
- [Benchmark methodology](docs/benchmark-methodology.md) and [validation coverage](docs/coverage.md)
- [Changelog](CHANGELOG.md) and [release instructions](docs/release.md)

Run `clack doctor --json` for diagnostics. Report bugs and suggestions through
[GitHub issues](https://github.com/jamesliuai/clack/issues/new/choose). For
vulnerabilities, follow the [security policy](SECURITY.md).

## Contributing

Focused pull requests, bug reports, and documentation improvements are welcome.
See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and checks, and follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Historical measurements and validation
reports describe the revisions and platforms actually tested; they are not
claims that every subsequent release has been retested on every platform.

## Inspiration and license

[Monkeytype](https://monkeytype.com/) is the main inspiration for Clack's focused
typing experience. Clack is independent, unaffiliated with and not endorsed by
Monkeytype. It has its own scoring specification and includes no Monkeytype
implementation, logo, themes, or text collections.

Application code is [MIT licensed](LICENSE). See
[third-party notices](THIRD-PARTY-NOTICES.md) for dependency and content licenses.
