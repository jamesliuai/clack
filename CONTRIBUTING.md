# Contributing to Clack

Bug reports, focused features, tests, and documentation are welcome. Please
follow the [Code of Conduct](CODE_OF_CONDUCT.md). Report security issues using
[SECURITY.md](SECURITY.md), without posting exploit details in public issues.

## Development setup

Install Rust 1.88+ and a C compiler for bundled SQLite. CI uses Rust 1.95.0 for
its main checks and separately verifies the minimum supported compiler.
Python 3.11+ runs the release, license, and PTY tooling.

```sh
git clone https://github.com/jamesliuai/clack.git
cd clack
cargo build --locked
cargo run --locked -- --help
```

Keep the entire `vendor/` tree. Versioned paths select the terminal forks and
Windows private-file adapter during development; registry packages select
published equivalents. Use `--config PATH`, `--data-dir PATH`, and `--private`
when manually testing to avoid modifying your normal configuration or history.

## Before a pull request

Explain the problem and the user-visible result. Include reproduction steps
for bugs and relevant terminal, operating system, and Clack versions. Discuss
large behavior changes in an issue first. Avoid unrelated dependency updates.

```sh
cargo +1.95.0 fmt -p clack-typing -p clack-private-fs -- --check
cargo +1.95.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.95.0 test --locked --all-targets --all-features
cargo +1.88.0 check --locked --all-targets --all-features
python3 scripts/licenses.py --check
python3 scripts/release_test.py
python3 scripts/benchmark.py --self-test
python3 scripts/audit_coverage.py --require-local-complete
```

For interactive changes, build with `--features test-hooks` and run the PTY
commands used in [CI](.github/workflows/ci.yml). Manually exercise the affected
controls, resize behavior, and terminal restoration. Do not present a PTY test
as evidence of native Windows or physical display behavior.

The pure engine, configuration, storage, and UI tests should cover changed
behavior. Keep scoring changes explicit. See [SPEC.md](SPEC.md), the
[coverage ledger](docs/coverage.md), and [benchmark methodology](docs/benchmark-methodology.md)
for the design and validation contracts.

## Dependencies and content

Terminal-fork changes must retain upstream licenses and update the fork's change
notes. The Windows adapter's unsafe boundary is documented in
[vendor/clack-private-fs/SAFETY.md](vendor/clack-private-fs/SAFETY.md).
Run support-crate tests when modifying those crates. Version bumps must update
both the package and all exact dependency requirements.

After changing lockfiles, run `cargo fetch --locked`, then
`python3 scripts/licenses.py` and review the regenerated inventory and notices.
The generator rejects unreviewed licenses and dependency sources.

Do not copy Monkeytype code, themes, logos, or text collections. New bundled
content needs documented provenance and compatible terms in [docs/content.md](docs/content.md).

## Documentation and releases

Keep README examples, CLI/settings documentation, and CHANGELOG.md aligned with
behavior. Preserve historical measurements as historical evidence. Follow
[docs/release.md](docs/release.md) for publishing order and archive verification.
