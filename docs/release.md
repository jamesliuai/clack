# Release construction and validation

Application version 1.0.0 targets stable Rust with MSRV 1.88. The checked-in
lockfile is mandatory. Default release settings use thin LTO, one codegen unit,
symbol stripping and unwind panics so the terminal guard can restore state.
The core application forbids unsafe code. A narrow Windows-only private-file
adapter implements protected DACL creation for SPEC PRIV-005; its reviewed
exception and tests are documented in [SAFETY.md](../vendor/clack-private-fs/SAFETY.md).
The narrowly patched Crossterm backend
retains upstream low-level implementation code under its original MIT license.
No `target-cpu=native` package is supported.

## Rebuild and archive

Install the documented Rust toolchain and a platform C compiler, then fetch the
lockfile's crates once. Subsequent build and package commands work offline:

```sh
cargo +1.95.0 fetch --locked
cargo +1.95.0 build --locked --offline --release
python3 scripts/licenses.py --check
python3 scripts/release_test.py
python3 scripts/package.py --target aarch64-apple-darwin --toolchain 1.95.0
python3 scripts/package.py --source --output dist
```

Use the target that matches the build runner. The package builder independently
builds without test hooks, checks executable architecture and size, and for a
native build executes version and doctor metadata checks. It generates man and
all five shell-completion formats from that same executable. Each archive
contains a manifest of every file with SHA-256, size and mode; its sibling
`.sha256` protects the full archive. Existing archives are never overwritten.

After collecting the five required platform archives and the source archive in
one directory, run `python3 scripts/verify_release.py --directory dist`. This
checks every payload against its manifest, confirms that source identities
match, executes the binaries supported by the current host, and installs and
uninstalls those packages inside disposable directories. Foreign OS execution
is explicitly unperformed. The report stays beside the archives.

The source archive preserves the original Cargo manifest, lockfile, complete
application/tests/scripts, approved data, vendor fork and privacy adapter, documentation, notices,
CI configuration and source specification. It does not depend on Git metadata.
Source archives preserve versioned local dependency paths. The separate crates.io
package normalizes those paths to published support crates. Neither the existing
archive workflow nor a push to main publishes Rust crates automatically.

## Publishing to crates.io

Use a verified crates.io account and `cargo login`. Start from a clean release
commit, update the changelog and affected lockfiles, regenerate notices, and run
CI checks. Publish changed support crates in this order, running the same command
with `--dry-run` before each actual upload:

```sh
cargo publish --manifest-path vendor/crossterm/Cargo.toml
cargo publish --manifest-path vendor/ratatui-crossterm/Cargo.toml
cargo publish -p clack-private-fs
cargo publish -p clack-typing --dry-run
cargo publish -p clack-typing
```

Skip unchanged support versions already in the registry. A published version
cannot be overwritten: bump changed support versions and their exact dependency
requirements together. Version 1.0.0 uses `clack-crossterm` 0.30.0,
`clack-ratatui-crossterm` 0.2.0, and `clack-private-fs` 1.0.0.
Check `cargo package -p clack-typing --list` and verify a registry install in a
temporary directory with `cargo install clack-typing --version 1.0.0 --locked --root PATH`.

## Archive reproducibility

Archive entries have sorted paths, fixed permissions, zero owner/group IDs and
`SOURCE_DATE_EPOCH` timestamps (default zero; ZIP clamps dates to 1980). Repeating
the archive operation over identical input bytes yields identical bytes. A
reproducible archive is separate from a reproducible compiler output: compiler,
SDK, C compiler, paths and operating system can affect executable bytes. Keep
those inputs fixed and compare the manifest's binary and source hashes. Never
claim independent bit-identical cross-host compiler reproduction from archive
determinism alone.

## Historical local binary construction

The following measurements predate the crates.io packaging changes. They remain
evidence for the exact source identities recorded below, not this release build.

The frozen source identity for this collection is
`31586c3bc81633d5d3f912b7e717800caf185ecfce5358f7feeb92b3aa8129e3`.
All five production executables were rebuilt with Rust 1.95.0, thin LTO,
one codegen unit, optimization level 3, stripped symbols, unwind panics and
disabled test hooks/debug assertions. Bundled SQLite and the default data are
inside each executable. The byte counts exclude archive documentation/notices:

| Target | Executable bytes | Observed binary requirements |
|---|---:|---|
| macOS arm64 | 5,047,968 | macOS deployment 11.0; system dylibs |
| macOS x86-64 | 5,401,632 | macOS deployment 11.0; system dylibs |
| Linux arm64 GNU | 5,007,184 | GLIBC symbols no newer than 2.17 |
| Linux x86-64 GNU | 5,600,552 | GLIBC symbols no newer than 2.17 |
| Windows x86-64 GNU | 5,273,088 | Windows system DLLs and Universal CRT API sets |

Linux and Windows cross builds used cargo-zigbuild 0.23.4 and Zig 0.15.2.
The two Linux link targets explicitly select glibc 2.17; ELF headers confirm
PIE, RELRO and nonexecuting stacks, with only libc/libm/libpthread/libdl imports.
Windows PE headers confirm ASLR, DEP and high-entropy ASLR, with no third-party
runtime DLL import. Rust documents Windows 10+/Server 2016+ for this target;
the Universal CRT is an operating-system component on Windows 10 and later.
See [Rust's platform requirements](https://doc.rust-lang.org/rustc/platform-support.html)
and [Microsoft's UCRT documentation](https://learn.microsoft.com/en-us/cpp/porting/upgrade-your-code-to-the-universal-crt?view=msvc-170).

Both macOS binaries record SDK 26.2 and deployment minimum 11.0. The arm64
executable runs locally; the x86-64 executable can run here through Rosetta.
Those observations do not establish native Intel, older macOS, foreign-OS or
actual-emulator compatibility. Exact commands, binary hashes, headers and
dependency lists are in `docs/measurements/release-permissions-builds.json` and its
referenced logs. The native CI matrix below deliberately also builds Windows
MSVC; that separate ABI has not been executed in this local environment.

The complete six-archive draft and actual host install/uninstall checks passed
against these exact binaries; [draft validation](measurements/release-draft-validation.json)
records full payload/outer hashes and the source archive check. Final distribution
archives are rebuilt after committing final audit documents and are verified
again in `dist/release-validation.json`. Draft and final archive hashes can
differ because the final documents contain the completed audit; their frozen
application executable hashes remain the same.

## Native CI matrix

`.github/workflows/ci.yml` defines these native jobs:

| Target | Runner |
|---|---|
| Linux x86-64 GNU | Ubuntu 22.04 x86-64 |
| Linux arm64 GNU | Ubuntu 22.04 arm64 |
| macOS x86-64 | macOS 15 Intel |
| macOS arm64 | macOS 14 arm64 |
| Windows x86-64 MSVC | Windows Server 2022 |

The quality gate runs format, warning-free linting across features, license and
advisory review, package/installer tests, benchmark-harness self-tests and the
312-ID coverage-ledger structural gate. The ledger gate preserves external
validation rows without treating them as behavioral passes. The
MSRV gate compiles every target kind and runs unit/integration tests on 1.88.
Each native matrix job runs warning-free checks and all tests, then a production
release build, package checksum verification and isolated package installation
and removal. Workspace defaults include the Windows privacy adapter, and the
Windows runner additionally compiles all target kinds on Rust 1.88. A separate
job verifies the complete source archive. Unix jobs
additionally run real controlling-PTY
lifecycle and product workflows. GitHub-hosted runner names follow the
[official runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
Actions are pinned to full commit IDs, credentials are not retained, and workflow
permissions are read-only. Dependency updates are proposed separately.

The workflow is a reviewable validation definition; it is not evidence that a
remote job ran. No tag, remote repository, uploaded release or publication is
created by this work. Actual local build and test outcomes are recorded in
`dist/release-validation.json` when produced. This attestation is outside the
archives so their hashes cannot become self-referential. A locally produced
foreign executable is identified as cross-compiled; its header/checksum do not
substitute for running it on its supported OS.

## Installation and rollback

After verifying the archive checksum, extract into a fresh directory and inspect
its manifest. The optional `install.py` checks the operating system and processor
architecture (with explicit macOS ARM64-to-x86-64 Rosetta compatibility), then
validates the complete package before
placing any binary. It publishes an executable only after preparing the whole
documentation/license bundle, refuses unknown existing destinations, and records
the hashes it owns. Reinstalling the exact package is idempotent. Installing a
different version requires explicit uninstall or another prefix.

Installer path checks reject symbolic links and Windows reparse points; uninstall
directory cleanup does not descend into junctions. A native Windows junction
fixture is included in CI and remains external validation on this macOS host.

Uninstall verifies the registered binary and removes only unchanged registered
files. User-added/modified bundle files remain. User configuration and result
databases are outside the ownership record and are always preserved. Power-loss
durability and filesystem semantics need native validation; the helper does not
claim an OS package-manager transaction or modify shell profiles.

## External release gates

Run native binaries on each OS and architecture, including actual local emulator
input/appearance, macOS Terminal, Windows Terminal, baseline and Kitty-protocol
terminals, tmux and SSH. Run filesystem permission/atomic-write/locking checks on
each OS. Verify release startup and latency against the controlled reference
machine, with recorded host/terminal/build/warmness/geometry/sample-count details.
The local Apple Silicon PTY evidence and cross builds retain their own scope;
unavailable emulator, hardware and controlled-baseline checks remain pending.
