<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Host system admin

Host apt and IPv4 are **not** `DockerOp`. Cookie-authed
`POST /nodes/{id}/sys/{op}` and `GET /api/v1/nodes/{id}/sys/updates` become
gRPC `Command`s with `SysOp::as_str()` (`status`, `updates_list`,
`updates_apply`, `net_set`).

The packaged agent stays `NoNewPrivileges=true` / `ProtectSystem=strict`.
It talks to `/run/keystone/sys.sock` (`0660 root:keystone`) only if the
operator enabled `keystone-sys.socket`. The agent unit keeps
`RuntimeDirectory=keystone` and `ReadWritePaths=/run/keystone` (the
directory). `-/run/keystone/sys.sock` is wrong: ignore-if-missing leaves
`/run` read-only when the helper is enabled after the agent started. The
helper binary is `/usr/lib/keystone/keystone-sys` (root, no `sh -c`, no
setuid).

`NodeSettings.sys_enabled` / `sys_manage` are pushed in `set_runtime`.
The agent refuses mutating `SysOp` unless manage is on. Tests must not
run live `apt-get`.

| Operation | Mutating | Permission | Description |
|---|---|---|---|
| `status` | no | `sys_view` | Host snapshot (addresses, reboot-needed, helper) |
| `updates_list` | no | `sys_view` | List pending apt upgrades |
| `updates_apply` | yes | `sys_manage` | Apply apt upgrades (streamed) |
| `net_set` | yes | `sys_manage` | Set IPv4 DHCP or static on one interface |
