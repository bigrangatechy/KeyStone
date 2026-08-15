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
2. **Allow mutations** — start, stop, restart, kill, remove; Compose
   up/down/pull; image pull/prune/remove; volume and network create/remove.
   Destructive actions ask for confirmation. The pull/create forms take a
   plain name, not JSON.
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

- **Containers** — name, image, state, Compose project. **Logs** opens a
  follow view (last 200 lines, then live). With Manage: start / stop /
  restart / kill / remove.
- **Compose** — projects discovered from `com.docker.compose.project`
  labels, with a service table per project. **Logs** follows
  `docker compose logs`. With Manage: up / down / pull. **Compose files**
  on Settings are extra `-f` paths when a command does not name a file.
- **Images** — tags, short id, size. With Manage: pull by name, prune
  unused, remove.
- **Volumes** and **Networks** — list; create/remove with Manage.

Mutations require a **logged-in UI session**. The ingest token used by
agents cannot call these actions. Every mutation is written to the audit
log (who, node, operation, target, success).

Leave a logs page to stop follow: the browser disconnects, the server
cancels the agent stream. There is no interactive exec/PTY in this UI.

## If the tabs are empty or error

- Observe is off on that node.
- The agent is not control-connected (see [Troubleshooting](troubleshooting.md)).
- The `keystone` user cannot open the socket (`Permission denied`).
- Docker is not installed, or `docker.host` points at the wrong path.

The Overview still works without Docker. Container CPU/memory gauges on the
dashboard only appear when Observe is on and the agent is pushing container
aggregates.
