# Upgrading from Clack 0.1 to 1.0

Version 1.0 follows the newer implementation on the repository's main branch.
The Cargo package remains `clack-typing`, and the command remains `clack`.

This is a major release: controls, configuration, scoring, and storage have
changed. The old JSONL history is not a SQLite database, and the old
`config_version = 1` configuration is not the new `schema_version = 1` format.
There is no automatic importer for 0.1 history or configuration.

Back up the configuration and history reported by the old executable's
`clack config path` before upgrading. Keep old exports if you want to retain
access to their metrics. Start 1.0 with a separate configuration path and a new
data directory using `--config PATH` and `--data-dir PATH`; do not point it at
old files expecting a migration. Follow [settings](settings.md) to recreate
preferences. Scores from the two implementations should not be compared as
though their rules were identical.

```sh
cargo install clack-typing --version 1.0.0 --locked
```

The [README](../README.md#keyboard-controls) covers the new controls. In
particular, `Ctrl+T`/`F6` opens test setup and `Ctrl+R` starts a new test.
The old 0.1.0 crate remains available for users who need that implementation.
