# Changelog

## Unreleased

- Reduce memory use for untyped words, long passages, and repeated source preparation;
  keep rendering incremental around the caret and cache review navigation.
- Defer result validation to the storage worker, accelerate history queries, and
  bound SQLite records and exported file writes.
- Preserve input after unsupported terminal sequences, keep incomplete reads
  interruptible, and restore the terminal after input-worker failures.
- Reject nonregular source files and exclude local paths and measurements from
  distributed archives.
- Prepare 1.0.1 with `clack-crossterm` 0.30.1 and `clack-ratatui-crossterm` 0.2.1,
  and verify the staged Cargo packages together before publication.

## 1.0.0 — 2026-09-16

- Publish the newer main-branch implementation under the existing `clack-typing`
  package name, with the `clack` executable.
- Include quick test setup, exact/code and target-free Zen modes, versioned
  settings, SQLite history, statistics, and the newer terminal input features.
- Publish the matching terminal forks and Windows private-file adapter so Cargo
  installs use the same implementations as source builds.
- Refresh the README and public contributor, security, and release guidance.

This release changes configuration, storage, controls, and scoring from 0.1.
See [upgrade notes](docs/upgrading.md). Historical validation records belong to
the source revisions they identify.

## 0.1.0

Initial crates.io release of the earlier implementation, with JSONL history,
word and timed tests, quotes, themes, and bounded terminal input.
