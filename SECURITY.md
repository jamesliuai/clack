# Security policy

Security fixes target the latest 1.x release. Older 0.1 versions remain available
but are not maintained as a separate security branch.

## Private reporting

Use GitHub's **Security → Report a vulnerability** action if available. Include
the affected version, OS, terminal, impact, and minimal reproduction. If private
reporting is unavailable, open an issue requesting a private contact channel
without vulnerability details. Do not include credentials, private passages, or
other people's data in public issues. Maintainers coordinate fixes and disclosure
as capacity permits; there is no guaranteed response time.

## Scope

Clack runs locally with the invoking user's permissions. Inputs include terminal
events, CLI arguments, configuration, custom passages, bundled content, and local
SQLite history. It does not contact a server or fetch code/content at runtime.

See [storage](docs/storage.md) for persistence and export behavior, the
[terminal matrix](docs/terminal-matrix.md) for input and restoration boundaries,
and the [Windows adapter review](vendor/clack-private-fs/SAFETY.md) for private
file creation. These controls do not make Clack a sandbox.

The terminal forks are published as `clack-crossterm` and
`clack-ratatui-crossterm`; `clack-private-fs` supplies the Windows permission
adapter. Development uses their reviewed source under `vendor/`. See
[dependency review](docs/dependency-review.md) and [third-party notices](THIRD-PARTY-NOTICES.md).

Crashes, terminal escape injection, unintended file access, privacy regressions,
and reachable dependency vulnerabilities are appropriate reports. Editing one's
own scores and ordinary appearance preferences are not security vulnerabilities.
