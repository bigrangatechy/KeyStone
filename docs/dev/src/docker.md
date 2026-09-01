<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Docker control

The UI, control RPC, audit log, and agent dispatcher share `DockerOp` in
`crates/keystone-core/src/docker.rs`. `op` on the wire is `DockerOp::as_str()`
(snake_case). `payload_json` is op-specific.

The server never opens `docker.sock`. `keystone-agent` uses bollard (and
`docker compose` subprocesses for Compose ops) on the local engine.

## Policy

`NodeSettings` (`docker_enabled`, `docker_manage`, `docker_allow_exec`,
`compose_paths`) is pushed as `set_runtime`. The agent refuses:

- any `DockerOp` when observe is off (no handle)
- `mutating()` ops unless `manage`
- `container_exec` unless `allow_exec` (even if manage is on)

`docker.host` stays in `agent.toml`. Socket access is root-equivalent.

UI POST `/nodes/{id}/docker/{op}` requires a cookie session. The ingest
token cannot call it. Mutations are written to `audit` (header `GET /audit`).
Streaming ops (`container_logs`, `compose_logs`) are not POSTed; they use
SSE (below). `DockerOp::needs_step_up()` is empty this version: confirm
only. The same `consume_step_up` helper as System will enforce a current
6-digit `totp` form field when a Docker op opts in. Backup codes are for
sign-in only. TOTP off stays confirm-only.

`Permission` mapping: `container_exec` → `docker_exec`; other mutating ops
→ `docker_manage`; the rest → `docker_view`. The signed-in admin has all of
them. The node page hides Manage buttons and pull/create toolbars when
`docker_manage` is off, and skips listing when the agent is offline or
Observe is off. The agent gates still apply.

## Streaming logs

`DockerOp::streams()` is `container_logs` and `compose_logs`. The agent
sends `StreamChunk` (`data`, then `eof`) followed by `CommandResult`.
`op == "cancel"` with `{"request_id":"..."}` aborts the task.

Non-streaming Docker and System RPCs (`container_list`, `status`, …) are
also spawned off the ingest `select!` loop. Awaiting them there meant the
agent stopped reading Commands while one list (or a hung helper) ran, so
the node page hit **Docker: agent command timed out**. Host collect and
per-container `stats` stay off that loop too; `stats` runs in parallel
with a short cap so it cannot monopolise `docker.sock`. While a node-page
list is in flight the agent skips `docker stats` and `engine_version`, and
those list RPCs themselves have a 6s budget so a hung Engine still replies
before the server's 8s wait. System `status` runs `ip` and the helper
together, each capped, instead of in series. Pushes `try_send` and overflow
to the disk buffer so CommandResults are not stuck behind a reconnect dump.

The server ingest loop must keep reading `CommandResult`s while it writes
Commands. Blocking on the gRPC sink reset the session (**agent dropped
command**) or stalled Result reads (**agent command timed out**). Acks and
Commands are try-queued to a side writer; Commands are preferred over a
burst of Pushes. Series writes (and the `series.redb` retention prune, at
most once a minute) run on a side task so a large history cannot block
Result reads. The agent also spawns
`set_runtime` so a Docker socket connect after reconnect cannot block
`container_list`. A replaced ingest session keeps in-flight waits and
replays those Commands on the new channel so the UI does not show
**agent dropped command**. The node page runs `container_list` first, then
the other Docker tables with the remaining 8s, in parallel with System
`status`.

HTTP:

- `GET /nodes/{id}/containers/{cid}/logs` — HTML follow page
- `GET /nodes/{id}/containers/{cid}/logs/stream?follow=1` — SSE
- `GET /nodes/{id}/compose/{project}/logs` and `.../logs/stream` — same for
  Compose

SSE events: JSON `{"t":"<text>"}` as default `message`; `event: done` on
eof. Dropping the SSE connection cancels the agent stream. `container_stats`
is a one-shot JSON GET, not wired in the UI.

List payloads the UI expects:

- containers: `[{id, id_full, names, image, state, status, compose_project, ports, cpu_ratio?, memory_bytes?}]`
  (cards; click loads summarized `container_inspect` via
  `GET /api/v1/nodes/{id}/containers/{cid}`. That summary drops `Env`.
  `cpu_ratio` / `memory_bytes` are joined from pushed
  `container_cpu_usage_ratio` / `container_memory_usage_bytes` at page load;
  the tab then polls `GET /api/v1/nodes/{id}/container-usage`. Not a live
  `container_stats` stream. `ports` is a host publish string, e.g.
  `0.0.0.0:8080->80/tcp`)
- compose ps: `{ "<project>": [{id, id_short, name, image, state, status, service, ports}] }`
  (union of engine labels, Settings `compose_paths`, and last-seen projects
  so Down does not drop the tab)
- images: `[{id, id_short, tags, size}]`
- volumes: `[{name, driver, mountpoint}]`
- networks: `[{id, id_short, name, driver, scope}]`

## Operations

| Operation | Mutating | Permission | Description |
|---|---|---|---|
| `container_list` | no | `docker_view` | List containers |
| `container_inspect` | no | `docker_view` | Inspect a container |
| `container_start` | yes | `docker_manage` | Start a container |
| `container_stop` | yes | `docker_manage` | Stop a container |
| `container_restart` | yes | `docker_manage` | Restart a container |
| `container_kill` | yes | `docker_manage` | Kill a container |
| `container_remove` | yes | `docker_manage` | Remove a container |
| `container_pause` | yes | `docker_manage` | Pause a container |
| `container_unpause` | yes | `docker_manage` | Unpause a container |
| `container_prune` | yes | `docker_manage` | Prune stopped containers |
| `container_logs` | no | `docker_view` | Stream container logs (on-demand) |
| `container_stats` | no | `docker_view` | Stream live container stats (on-demand) |
| `container_exec` | yes | `docker_exec` | Exec a command in a container (disabled unless allow_exec) |
| `compose_ps` | no | `docker_view` | List Compose project services |
| `compose_up` | yes | `docker_manage` | Compose up |
| `compose_stop` | yes | `docker_manage` | Compose stop |
| `compose_start` | yes | `docker_manage` | Compose start |
| `compose_restart` | yes | `docker_manage` | Compose restart |
| `compose_down` | yes | `docker_manage` | Compose down |
| `compose_logs` | no | `docker_view` | Compose logs |
| `compose_pull` | yes | `docker_manage` | Compose pull |
| `compose_update` | yes | `docker_manage` | Compose pull then up |
| `image_list` | no | `docker_view` | List images |
| `image_inspect` | no | `docker_view` | Inspect an image |
| `image_pull` | yes | `docker_manage` | Pull an image |
| `image_prune` | yes | `docker_manage` | Prune unused images |
| `image_remove` | yes | `docker_manage` | Remove an image |
| `volume_list` | no | `docker_view` | List volumes |
| `volume_inspect` | no | `docker_view` | Inspect a volume |
| `volume_create` | yes | `docker_manage` | Create a volume |
| `volume_remove` | yes | `docker_manage` | Remove a volume |
| `volume_prune` | yes | `docker_manage` | Prune unused volumes |
| `network_list` | no | `docker_view` | List networks |
| `network_inspect` | no | `docker_view` | Inspect a network |
| `network_create` | yes | `docker_manage` | Create a network |
| `network_remove` | yes | `docker_manage` | Remove a network |
| `network_prune` | yes | `docker_manage` | Prune unused networks |

When adding an op: extend the enum, `description`, `mutating`,
`permission`, implement it in `keystone-agent/src/docker.rs`, wire the
node template / `app.js`, and add the `` `snake_name` `` row here.

Control-plane ops that are **not** `DockerOp`: `set_runtime`,
`set_interval`, `cancel`, and host `SysOp` (`status`, `updates_list`,
`updates_apply`, `updates_autoremove`, `net_set`, `gitlab_backup`, `gitlab_restore`, `reboot`,
`journal`, `unit_restart`) — see
[Host system admin](system.md). The agent
handles `set_runtime` / `set_interval` / `cancel` before `handle_command`.

Docker Hub search is not a `DockerOp`. Cookie-authed
`GET /api/v1/dockerhub/search` and `.../tags` fetch Hub’s public HTTP API
from the **server** and map JSON in `keystone-core` (`dockerhub.rs`).
The UI fills the existing `image_pull` form. The server does not pull
images and does not open `docker.sock`. Tests use Hub JSON fixtures; they
must not hit the network.
