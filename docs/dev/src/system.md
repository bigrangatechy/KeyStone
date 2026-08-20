<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Host system admin

Host apt and IPv4 are **not** `DockerOp`. Cookie-authed
`POST /nodes/{id}/sys/{op}` and `GET /api/v1/nodes/{id}/sys/updates` become
gRPC `Command`s with `SysOp::as_str()` (`status`, `updates_list`,
`updates_apply`, `net_set`, `gitlab_backup`, `reboot`, `journal`).

The packaged agent stays `NoNewPrivileges=true` / `ProtectSystem=strict`.
It talks to `/run/keystone/sys.sock` (`0660 root:keystone`) only if the
operator enabled `keystone-sys.socket`. The agent unit keeps
`RuntimeDirectory=keystone` and `ReadWritePaths=/run/keystone` (the
directory). `-/run/keystone/sys.sock` is wrong: ignore-if-missing leaves
`/run` read-only when the helper is enabled after the agent started. The
helper binary is `/usr/lib/keystone/keystone-sys` (root, no `sh -c`, no
setuid).

`NodeSettings.sys_enabled` / `sys_manage` are pushed in `set_runtime`.
The agent refuses mutating `SysOp` unless manage is on. Helper RPCs have a
read deadline (`status` 5s) so a stuck socket cannot block Docker Commands
on the ingest loop. `updates_list` runs `apt-get update`, `apt list
--upgradable`, and `apt-get -s dist-upgrade` (phased updates included).
Apply is still `apt-get upgrade`, not `dist-upgrade`, with
`NEEDRESTART_MODE=list` so Ubuntu 24.04 does not auto-restart docker or
ssh mid-upgrade. `status` parses `needrestart -b -r l` and
`systemctl --failed` (empty if the binary is missing or times out).
`status` also parses `timedatectl show -p NTPSynchronized` and the newest
`*_gitlab_backup.tar` under `/var/opt/gitlab/backups` (Omnibus only;
restore is not an op). `journal` follows `journalctl -u` for a
hardcoded unit list (not a textbox). `reboot` is hardcoded
`systemctl reboot` (not poweroff). Tests must not run live `apt-get`,
`gitlab-backup`, `journalctl -f`, or `systemctl reboot`.

| Operation | Mutating | Permission | Description |
|---|---|---|---|
| `status` | no | `sys_view` | Host snapshot (addresses, reboot-needed, leftover services, failed units, NTP, GitLab dump age, helper) |
| `updates_list` | no | `sys_view` | List pending apt upgrades |
| `updates_apply` | yes | `sys_manage` | Apply apt upgrades (streamed). `NEEDRESTART_MODE=list`. |
| `net_set` | yes | `sys_manage` | Set IPv4 DHCP or static on one interface |
| `gitlab_backup` | yes | `sys_manage` | Omnibus `gitlab-backup create` (streamed). Missing binary is an error; tests must not run it. Docker GitLab is not this op. |
| `reboot` | yes | `sys_manage` | Hardcoded `systemctl reboot`. Not streamed. Tests must not invoke it. Poweroff is not an op. |
| `journal` | no | `sys_view` | Follow `journalctl` for one allowlisted unit (streamed). Not a PTY. Tests must not follow a live journal. |

Mutating ops are written to SQLite `audit` (header `GET /audit`). The ingest
token cannot call these routes.
