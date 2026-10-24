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

A fork based on Crossterm 0.29.0, packaged as `clack-crossterm` 0.30.1, is included as a narrow MIT-licensed fork in
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

The MIT-licensed Ratatui Crossterm backend is packaged as
`clack-ratatui-crossterm` 0.2.1 with its dependency selecting the same terminal
fork. Its implementation is unchanged from upstream 0.1.2. See
[vendor/ratatui-crossterm/CLACK_PATCH.md](vendor/ratatui-crossterm/CLACK_PATCH.md)
and its [license](vendor/ratatui-crossterm/LICENSE).

## Theme palettes

Sixteen shipped themes use color values published by the projects below. The
role assignments, blended tones and terminal fallbacks are Clack's own, and no
theme file, code, logo or other asset from these projects is included. Each
project is distributed under the MIT License with the copyright notice shown;
the license text follows the table. See [docs/themes.md](docs/themes.md) for the
themes themselves.

| Themes | Project | Copyright notice |
|---|---|---|
| `nord` | [Nord](https://www.nordtheme.com) | Copyright (c) 2016-present Sven Greb |
| `dracula` | [Dracula](https://draculatheme.com) | Copyright (c) 2023 Dracula Theme |
| `gruvbox_dark`, `gruvbox_light` | [Gruvbox](https://github.com/morhetz/gruvbox) | Copyright (c) 2018 Pavel Pertsev |
| `solarized_dark`, `solarized_light` | [Solarized](https://ethanschoonover.com/solarized/) | Copyright (c) 2011 Ethan Schoonover |
| `catppuccin_mocha`, `catppuccin_latte` | [Catppuccin](https://catppuccin.com) | Copyright (c) 2021 Catppuccin |
| `tokyo_night` | [Tokyo Night](https://github.com/enkia/tokyo-night-vscode-theme) | Copyright (c) 2018-present Enkia |
| `rose_pine`, `rose_pine_dawn` | [Rosé Pine](https://rosepinetheme.com) | Copyright (c) 2023 Rosé Pine |
| `everforest` | [Everforest](https://github.com/sainnhe/everforest) | Copyright (c) 2019 sainnhe |
| `kanagawa` | [Kanagawa](https://github.com/rebelot/kanagawa.nvim) | Copyright (c) 2021 Tommaso Laurenzi |
| `one_dark` | [One Dark](https://github.com/atom/atom) (Atom) | Copyright (c) 2011-2022 GitHub Inc. |
| `synthwave` | [SynthWave '84](https://github.com/robb0wen/synthwave-vscode) | Copyright (c) 2019 Robb Owen |
| `night_owl` | [Night Owl](https://github.com/sdras/night-owl-vscode-theme) | Copyright (c) 2018 Sarah Drasner |

```text
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
