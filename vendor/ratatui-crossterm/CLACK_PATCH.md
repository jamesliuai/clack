# Clack's Ratatui Crossterm backend

`clack-ratatui-crossterm` is an MIT-licensed fork of `ratatui-crossterm` 0.1.2.
Upstream: https://github.com/ratatui/ratatui
Base crates.io archive SHA-256: `567584a3b0e6a8203c23de40b4861497266725eb5363dbfd18a1edd603cca9f0`.

The backend implementation is unchanged. Package metadata and the Crossterm
selection are changed to use `clack-crossterm` 0.30.0 exclusively; support for
upstream Crossterm 0.28 is removed. The library retains the `ratatui_crossterm`
name. See LICENSE for the original copyright and permission notice.

Clack disables Ratatui's built-in Crossterm backend and uses this crate directly.
This keeps cursor queries, raw-mode state, and input parsing in the same patched
Crossterm instance, including during terminal initialization and clearing.

Version 0.2.0 selects the newer terminal fork used by Clack 1.0.0.
