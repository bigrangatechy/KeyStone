<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Developer documentation

This book is for people changing KeyStone. The product goal is a **homelab
replacement for Portainer and Netdata**: live host metrics and per-node
Docker in one UI, unlimited nodes, NAT-friendly agents, no remote
`docker.sock`. Features should serve that, not grow into Kubernetes, a
SaaS cloud, or a generic enterprise NMS.

Operator install, Settings, and dashboards live in `docs/src/` (mdBook at
the repo `docs/` root, `/help` in the running server, `keystone docs`). Do
not put crate-level how-tos in the operator book: homelab users should not
need `WidgetKind` or `define_metric!`.

CI publishes this tree next to the operator book (`/dev/` on GitLab Pages).

## When you change behaviour

Update the **operator** chapter if an admin would notice (a new Settings
field, a new Docker checkbox, a new default card). Update **this** book if
the types, protocol, or extension steps changed. Tests in `keystone-core`
fail if the catalog, `DockerOp`, or `Permission` lists in these pages miss a
variant. Packaging scripts are similarly tested so they cannot `chown -R`
or depend on Docker Engine.

Commits need `git commit -s` (DCO). There is no CLA. Org-wide rules live
in [Ranga/community](https://git.bigrangatech.com/Ranga/community). See
CONTRIBUTING.md at the repository root.
