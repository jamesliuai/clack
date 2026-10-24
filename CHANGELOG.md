# Changelog

## Unreleased

## 1.1.1 — 2026-10-24

- Show Clack's typing screen, light and dark themes, live theme previews,
  result review, and exact code practice in high-resolution README screenshots.
- Keep local screenshot review artifacts out of source control and Cargo packages.

## 1.1.0 — 2026-10-23

- Add 16 themes adapted from classic palettes: Nord, Dracula, Gruvbox Dark and
  Light, Solarized Dark and Light, Catppuccin Mocha and Latte, Tokyo Night, Rosé
  Pine and Rosé Pine Dawn, Everforest, Kanagawa, One Dark, SynthWave '84, and
  Night Owl. See [themes](docs/themes.md).
- Give every theme ten color roles. Wrong and extra letters, the caret, the pace
  caret and selected rows now follow the theme instead of fixed terminal colors.
  The live timer, result figures and the review speed chart use its accent.
  Designed palettes no longer dim text still to be typed in 24-bit or 256-color
  output.
- Choose 256-color fallbacks by perceptual distance, and test every theme's
  contrast at 24-bit and 256-color depth.
- Show swatches, descriptions and a preview hint for themes in commands. Add
  `clack themes show` and shell completion for `--theme`.
- Detect 24-bit color in Windows Terminal and in terminals whose `TERM` always
  supports it.
- Keep release source fingerprints identical across Windows and Unix path ordering.

## 1.0.1 — 2026-10-13

- Reduce memory use for untyped words, long passages, and repeated source preparation;
  keep rendering incremental around the caret and cache review navigation.
- Defer result validation to the storage worker, accelerate history queries, and
  bound SQLite records and exported file writes.
- Preserve input after unsupported terminal sequences, keep incomplete reads
  interruptible, and restore the terminal after input-worker failures.
- Reject nonregular source files and exclude local paths and measurements from
  distributed archives.
- Use `clack-crossterm` 0.30.1 and `clack-ratatui-crossterm` 0.2.1,
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
