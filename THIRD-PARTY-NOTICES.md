# Third-party notices

Application code is licensed under [MIT](LICENSE). Dependencies and bundled
typing material retain their own terms; the application license does not grant
rights to somebody else's passages or dictionaries.

[third-party/inventory.json](third-party/inventory.json) records every locked
dependency, its declared SPDX expression, the selected license alternative,
registry checksum, repository and membership in the five release OS/architecture
configurations, including both MSVC and GNU Windows dependency graphs.
Those graphs include build tools and are not a claim that every listed crate is
linked into every executable. Full supplied license/notice files are retained in
[third-party/licenses](third-party/licenses). `scripts/licenses.py --check`
reproduces the inventory from the locked sources and rejects unreviewed license
expressions, sources or missing notices for native dependencies.

The unchanged `option-ext` 0.2.0 crate is MPL-2.0. Its complete covered source and
license are included at [third-party/source/option-ext-0.2.0](third-party/source/option-ext-0.2.0)
in source and binary packages. These files may be obtained and modified under
their MPL terms. No proprietary restriction is imposed on that source.

A fork based on Crossterm 0.29.0, published as `clack-crossterm` 0.30.0, is included as a narrow MIT-licensed fork in
[vendor/crossterm](vendor/crossterm), with its original
[license](vendor/crossterm/LICENSE). Changes expose associated text, permit a
wakeable dedicated reader and safe signal ownership, preserve epoch/partial
input boundaries, and support bounded explicit diagnostic queries. See
[docs/dependency-review.md](docs/dependency-review.md). Upstream source is not
represented as unchanged, and the fork is not automatically fetched at runtime.

The first-party MIT-licensed Windows private creation adapter is included at
[vendor/clack-private-fs](vendor/clack-private-fs) and identified in the locked
inventory for transparency. Its [safety review](vendor/clack-private-fs/SAFETY.md)
documents the narrow Win32 boundary. It uses the existing `windows-sys` dependency
and carries no additional registry or runtime service dependency.

Bundled language material, literature excerpts, the original code passage and
the SplitMix64 algorithm are separately documented in [docs/content.md](docs/content.md),
[data/source-manifest.json](data/source-manifest.json), each pack/passage's
metadata and [data/licenses](data/licenses). The literature selections are based
on the original public-domain works, not an assumption about a hosting site's
repository license. No Monkeytype implementation, logo, theme, text collection
or other project asset is bundled.

The MIT-licensed Ratatui Crossterm backend is published as
`clack-ratatui-crossterm` 0.2.0 with its dependency selecting the same terminal
fork. Its implementation is unchanged from upstream 0.1.2. See
[vendor/ratatui-crossterm/CLACK_PATCH.md](vendor/ratatui-crossterm/CLACK_PATCH.md)
and its [license](vendor/ratatui-crossterm/LICENSE).
