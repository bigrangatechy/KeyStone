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
token cannot call it. Mutations are written to `audit`. Streaming ops
(`container_logs`, `compose_logs`) are not POSTed; they use SSE (below).

`Permission` mapping: `container_exec` → `docker_exec`; other mutating ops
→ `docker_manage`; the rest → `docker_view`. The signed-in admin has all of
them. The node page hides Manage buttons and pull/create toolbars when
`docker_manage` is off, and skips listing when the agent is offline or
Observe is off. The agent gates still apply.

## Streaming logs

`DockerOp::streams()` is `container_logs` and `compose_logs`. The agent
sends `StreamChunk` (`data`, then `eof`) followed by `CommandResult`.
`op == "cancel"` with `{"request_id":"..."}` aborts the task.

HTTP:

- `GET /nodes/{id}/containers/{cid}/logs` — HTML follow page
- `GET /nodes/{id}/containers/{cid}/logs/stream?follow=1` — SSE
- `GET /nodes/{id}/compose/{project}/logs` and `.../logs/stream` — same for
  Compose

SSE events: JSON `{"t":"<text>"}` as default `message`; `event: done` on
eof. Dropping the SSE connection cancels the agent stream. `container_stats`
is a one-shot JSON GET, not wired in the UI.

List payloads the UI tables expect:

- containers: `[{id, id_full, names, image, state, status, compose_project, cpu_ratio?, memory_bytes?}]`
  (`cpu_ratio` / `memory_bytes` are joined from pushed
  `container_cpu_usage_ratio` / `container_memory_usage_bytes` at page load;
  the tab then polls `GET /api/v1/nodes/{id}/container-usage`. Not a live
  `container_stats` stream)
- compose ps: `{ "<project>": [{id, id_short, name, image, state, status, service}] }`
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
| `container_logs` | no | `docker_view` | Stream container logs (on-demand) |
| `container_stats` | no | `docker_view` | Stream live container stats (on-demand) |
| `container_exec` | yes | `docker_exec` | Exec a command in a container (disabled unless allow_exec) |
| `compose_ps` | no | `docker_view` | List Compose project services |
| `compose_up` | yes | `docker_manage` | Compose up |
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
| `network_list` | no | `docker_view` | List networks |
| `network_inspect` | no | `docker_view` | Inspect a network |
| `network_create` | yes | `docker_manage` | Create a network |
| `network_remove` | yes | `docker_manage` | Remove a network |

When adding an op: extend the enum, `description`, `mutating`,
`permission`, implement it in `keystone-agent/src/docker.rs`, wire the
node template / `app.js`, and add the `` `snake_name` `` row here.

Control-plane ops that are **not** `DockerOp`: `set_runtime`,
`set_interval`, `cancel`, and host `SysOp` (`status`, `updates_list`,
`updates_apply`, `net_set`) — see [Host system admin](system.md). The agent
handles `set_runtime` / `set_interval` / `cancel` before `handle_command`.

Docker Hub search is not a `DockerOp`. Cookie-authed
`GET /api/v1/dockerhub/search` and `.../tags` fetch Hub’s public HTTP API
from the **server** and map JSON in `keystone-core` (`dockerhub.rs`).
The UI fills the existing `image_pull` form. The server does not pull
images and does not open `docker.sock`. Tests use Hub JSON fixtures; they
must not hit the network.
