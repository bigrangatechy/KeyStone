<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Documentation

Two audiences, two trees. Neither is generated from Rust types.

| Tree | Audience | Published | In the server binary |
|---|---|---|---|
| `docs/src/` | Operators | GitLab Pages (book root) | Yes — `/help` and `keystone docs` via `include_str!` |
| `docs/dev/src/` | Contributors | GitLab Pages `/dev/` | No |

`crates/keystone-server/src/help.rs` embeds operator chapters. Adding a
user-facing page means: write `docs/src/*.md`, add it to `docs/src/SUMMARY.md`,
and add a `HelpSection` in `help.rs` if it should appear in the UI.

Developer pages are mdBook only. Completeness tests (not generators) live
in `keystone-core`:

- every `catalog()` name appears as `` `name` `` in `docs/dev/src/metrics.md`
- every `DockerOp::as_str()` in `docs/dev/src/docker.md`
- every `Permission::as_str()` in `docs/dev/src/permissions.md`

If you add a metric and forget the developer table, `cargo test` fails.
You still write the prose yourself.

`/help` is the operator book **for this binary**. Pages can be newer than
an installed `.deb`. Do not teach operators to run a docs generator.

CLI: `keystone docs` and `keystone docs --section <slug>` print the same
markdown as `/help`. Slugs match the operator filenames without `.md`.
