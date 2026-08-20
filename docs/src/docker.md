<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Docker

This is the Portainer-shaped half of KeyStone: containers, Compose, images,
volumes, networks, and **live logs** for **this** node, from the same UI as
the metrics.

Docker control is **per node** and **off until you enable it**. The server
never opens a remote engine socket. The agent on that machine talks to
Docker Engine locally (default `/var/run/docker.sock`). Anyone who can use
that socket can take over the host — treat Observe/Manage/Exec as
root-equivalent on that box.

## Enable it

On the node **Settings** tab:

1. **Observe Docker** — list containers, Compose projects, images, volumes,
   and networks; follow logs. No start/stop.
2. **Allow mutations** — start, stop, restart, pause/resume, kill, remove,
   prune stopped; Compose up/start/stop/restart/down/pull/**Update**; image
   pull/prune/remove; volume and network create/remove/prune.
   Destructive actions ask for confirmation. The pull/create forms take a
   plain name, not JSON. Image pull can also be filled from a Docker Hub
   search on that same form.
3. **Allow `docker exec`** — reserved for a future interactive exec. This
   version does **not** expose exec in the UI even if the box is ticked.
   Leave it off.

The add-node form’s “runs Docker” checkbox only turns on Observe.

A connected agent picks these flags up immediately (`set_runtime` on the
session). Until the first connect, `agent.toml` `[docker]` values are a
fallback. After that, the UI wins for enable/manage/exec and Compose paths.

The socket path stays in `agent.toml` as `docker.host` only if it is not
the default Unix socket. The packaged agent user is in the `docker` group
when Docker was installed at package time; if you installed Docker later,
add `keystone` to `docker` and restart the agent.

## What the tabs do

With Observe on and a live session, each tab is a table (not a JSON dump).
If the agent is offline or Observe is off, the tab says so instead of
showing stale lists. Manage buttons and pull/create toolbars are hidden
when mutations are off.

- **Containers** — name, image, published ports, CPU, memory, state, Compose
  project. CPU and
  memory come from the same background samples as Overview and update at
  that node’s poll interval while the tab is open (not a live Engine
  `stats` stream). **Logs** opens a follow view (last 200 lines, then live).
  With Manage: start / stop / restart / pause / resume / kill / remove, and
  **Prune stopped**.
- **Compose** — projects discovered from `com.docker.compose.project`
  labels **and** from **Compose files** on Settings, with a service table
  per project.   **Logs** follows `docker compose logs`. With Manage: **Up**, **Start**,
  **Stop**, **Restart**, **Down**, **Pull**, **Update** (pull then up — use
  this for a Cloudflare Tunnel stack, not the System tab). **Stop** keeps
  the containers (exited) on this tab. **Restart** bounces them without
  removing the project. **Down** removes that project’s
  containers (Docker’s `compose down`). The project **stays on this tab**
  so you can **Up** or **Start** it again; it is not gone. **Pull** uses that project’s
  compose file when KeyStone can read it (labels
  `com.docker.compose.project.config_files` / `working_dir`, or the
  matching Settings path). If the file is missing, Pull still refreshes
  images from the running/stopped containers. **Up** after Down needs a
  readable compose file on Settings. The packaged agent user is
  `keystone`: the YAML (and its directory) must be readable by that user.
  Put stacks in `/opt/…` or `chmod`/`setfacl` a home path; `ProtectHome`
  is read-only, not a hidden `/home`.
- **Images** — tags, short id, size. With Manage: pull by name, search
  Docker Hub to fill that name, prune unused, remove.
- **Volumes** and **Networks** — list; create/remove and prune unused with
  Manage.

Mutations require a **logged-in UI session**. The ingest token used by
agents cannot call these actions. Every mutation is written to
[Audit](audit.md) (who, node, operation, target, success).

Leave a logs page to stop follow: the browser disconnects, the server
cancels the agent stream. There is no interactive exec/PTY in this UI.

## Pulling an image

Type `nginx:1.27` (or `ghcr.io/…`) in **Pull** and submit. That is
`image_pull` on the **agent**. The server never talks to Docker Engine.

Optional: **Search Docker Hub** on the Images toolbar. The browser asks
the KeyStone server; the server queries Hub’s public HTTP API (not
`docker.sock`, and not your ingest token). Official images are marked.
Pick a tag to see last updated and architectures (`amd64`, `arm64`, …).
That **fills** the pull field — you still press Pull. This is not an app store
and does not log into Hub.

Unauthenticated Hub search is rate-limited per the **server’s** IP. If
search fails, type the name yourself. GHCR and private registries are
not browsed here; you can still pull them by typing the name if that
node can reach the registry.

## If the tabs are empty or error

- Observe is off on that node.
- The agent is not control-connected (see [Troubleshooting](troubleshooting.md)).
- **Docker: agent command timed out** — ingest Docker RPC, not the System
  helper. Restarting `keystone-sys.socket` does not fix it; upgrade/restart
  `keystone-agent`.
- **agent dropped command** — the session reset while lists were in flight.
  Upgrade `keystone-server` (0.1.0-14 or newer) so in-flight lists survive
  a session replace, Commands are written without blocking Results, and
  `container_list` does not share `docker.sock` with images/volumes.
- The `keystone` user cannot open the socket (`Permission denied`).
- Docker is not installed, or `docker.host` points at the wrong path.

The Overview still works without Docker. Container CPU/memory gauges on the
dashboard only appear when Observe is on and the agent is pushing container
aggregates.
