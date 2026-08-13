<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Permissions

`Permission` in `crates/keystone-core/src/rbac.rs`. The ingest token is
**not** a permission and cannot grant any of these.

| Permission | Description |
|---|---|
| `nodes_view` | View node list, heartbeat, and host metrics |
| `docker_view` | List and inspect Docker objects on a node |
| `docker_manage` | Start/stop/remove containers, Compose, images, volumes, and networks |
| `docker_exec` | Execute a process inside a container (root-equivalent) |

`Permission::admin_all()` is every variant. This slice has a single local
admin; cookie auth means the signed-in user is treated as that role. Agent
`DockerOp::permission()` is the intended mapping for a future UI that hides
buttons. Enforcement that already exists: agent `manage` / `allow_exec`
flags, and “must be logged in” on Docker POST.

When you add a variant, give it `description()`, include it in `admin_all`
if the admin should have it, and add `` `snake_name` `` to this table.
