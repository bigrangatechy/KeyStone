<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Developer documentation

This book is for people changing KeyStone. Operator install, Settings, and
dashboards live in `docs/src/` (mdBook at the repo `docs/` root, `/help` in
the running server, `keystone docs`). Do not put crate-level how-tos in the
operator book: homelab users should not need `WidgetKind` or `define_metric!`.

CI publishes this tree next to the operator book (`/dev/` on GitLab Pages).

## When you change behaviour

Update the **operator** chapter if an admin would notice (a new Settings
field, a new Docker checkbox, a new default card). Update **this** book if
the types, protocol, or extension steps changed. Tests in `keystone-core`
fail if the catalog, `DockerOp`, or `Permission` lists in these pages miss a
variant.

Commits need `git commit -s` (DCO). There is no CLA. See CONTRIBUTING.md
at the repository root.
