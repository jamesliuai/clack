> Packaging update: the application now selects `clack-crossterm` 0.30.0 and
> `clack-ratatui-crossterm` 0.2.0 using versioned paths for source builds and
> registry dependencies for Cargo installs. The terminal implementation remains
> the newer main-branch fork described below. Prior measurement hashes remain
> historical evidence and are not regenerated to imply a new audit.

# Locked dependency and source review

`Cargo.lock` identifies 188 packages: the application, the local Crossterm fork,
the Windows privacy adapter, and 185 registry packages. The release uses one core library and one executable,
Ratatui 0.30.2 with its explicit `crossterm_0_29` backend, Crossterm 0.29.0,
Clap-generated interfaces, Serde/TOML, bundled SQLite through Rusqlite, and pinned
Unicode segmentation/width/normalization. There is no Tokio, ECS, dependency
injection framework, database server or runtime network client in the application
graph. `#![forbid(unsafe_code)]` and Cargo's unsafe lint cover application source;
upstream dependency internals remain separate audited dependencies. The local
Windows-only adapter is the demonstrated PRIV-005 exception described below.

The engine reducer accepts ordered actions and injected receipt microseconds.
Its imports contain no terminal, filesystem, SQL or live-clock API. Rendering
borrows an immutable engine and mutates presentation buffers only. One reader
owns terminal event decoding, one thread owns the engine/output, and a lazily
created storage worker owns immutable snapshots and SQL. A diagnostic probe runs
only from the separate explicit doctor command and never competes with a live
input reader. See [the architecture ledger](coverage.md) for individual gates.

## Licenses and integrity

`scripts/licenses.py` reproduces the full dependency inventory and supplied
license/notice files from the locked local sources. It explicitly selects among
reviewed SPDX alternatives, rejects unknown expressions/sources, and checks
normal/build dependency membership for the five OS/architecture configurations,
including both Windows GNU and MSVC graphs. It includes build/development
dependencies for transparency, without claiming each is linked into every
binary. Every native graph dependency has supplied notices in the package.

MIT, Apache-2.0, Zlib, MPL-2.0 and Unicode-3.0 terms occur in the selected graph.
Where multiple alternatives are offered, the inventory records the chosen
alternative; an `AND Unicode-3.0` obligation is preserved. Full original supplied
license files are retained, including additional alternatives. `option-ext`
0.2.0 is the MPL-covered dependency: its complete unchanged crate source is
distributed with every binary and source package at
`third-party/source/option-ext-0.2.0`. This follows the source-availability
obligation described in the [Mozilla MPL FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/).

The independent archive check in
[dependency-integrity-permissions.json](measurements/dependency-integrity-permissions.json) verified all
185 cached registry archive SHA-256 values against the lockfile. Notices and
MPL source are reproducible with `python3 scripts/licenses.py --check`.
Data licenses are independently recorded in `data/source-manifest.json`, pack
metadata, `data/unicode-case-manifest.json` and `data/licenses`. Generated Unicode
case mappings are pinned, so changing the Rust compiler does not silently alter
seeded case conversion.

## Security checks and maintenance

`deny.toml` and CI reject unknown registries/Git sources, unreviewed licenses,
yanked dependencies and known security advisories; no advisory ignore list is
configured. Multiple upstream dependency versions remain visible in the lockfile
inventory. `cargo-deny` 0.20.2 license/bans/source/advisory checks passed locally.
The independent [cargo-audit report](measurements/cargo-audit-permissions-metadata.json) used
`cargo-audit` 0.22.2 and RustSec database commit
`5a0ebedfe8bdd2e295b171f4162f8c977bcad9a5`: no known vulnerability or warning was
reported for the current 188-package lockfile, including the separately reviewed
local privacy adapter. The report includes exact time, tool,
database and lockfile identities. These tools check published advisories; they
do not prove the absence of undiscovered defects or review a local patch's logic.

Dependency updates require rerunning compatible-backend, MSRV, Unicode replay,
native-platform, warning-free, license and advisory checks. The CLI is generated
from the same definitions as its help/man/completions. The release source archive
preserves the lockfile and root patch; publishing a normalized crate is disabled.

## Crossterm compatibility fork

The fork is based on crates.io 0.29.0, upstream commit
`36d95b26a26e64b0f8c12edfe11f410a6d56a812`, archive SHA-256
`d8b9f2e4c67f833b660cdb0a3523065869fb35570177239812ed4c905aeff87b`.
The original MIT license is byte-for-byte unchanged.
[crossterm-fork.json](measurements/crossterm-fork.json) identifies every changed
source file and its upstream/local hash; build caches are excluded.

The changes preserve associated CSI-u text and reject malformed/oversized input;
expose a wakeable reader without EventStream; reset pending decoder/OS input at
epoch boundaries; route signals through the existing safe wake pipe; preserve
Windows console modes, repeat counts and surrogate metadata; and expose parsed
responses to the bounded explicit doctor probe. The full-decimal keyboard flag
response fix prevents an ASCII digit from being mistaken for a numeric bitmask.
No application thread introduces a second keyboard read or an unsafe signal
cleanup handler. The fork's [change notes](../vendor/crossterm/CLACK-PATCH.md) state
the exact optional features, bounded parser behavior and backend tests.

Both selected Unix TTY and alternative MIO paths have actual local test evidence.
Cross checks cover Windows conditional source, while native Windows console
behavior remains an external gate. The complete fork is in the source release,
and its original license is also in every binary's dependency notices. A future
upstream upgrade must retain these behaviors or remove the patch only after the
corresponding application and decoder regressions pass.

## Windows private creation adapter

The MIT-licensed `clack-private-fs` workspace member uses the already locked
`windows-sys` 0.61.2 to create private files with a protected current-user DACL
before payload writing. Stable standard-library file creation cannot supply that
descriptor. Relying on inherited defaults would violate PRIV-005 in shared
directories. The application itself still forbids unsafe code; the narrow adapter
isolates token, descriptor and handle operations behind safe functions with RAII.
The full rationale, bounds proof, current-user/administrator trust boundary and
native Windows tests are in its [safety review](../vendor/clack-private-fs/SAFETY.md).

Private SQLite parents are protected and pinned through connection cleanup;
existing database and sidecar ACLs are checked individually. Unsafe existing
parents/children are refused without changing unrelated ACLs. New configuration,
exports, recovery and imported language content also use atomic private creation.
The adapter has no Unix implementation and adds no registry package. Native
Windows standard/elevated-token and filesystem validation remains separately
identified from the local Windows cross-compilation checks.
