<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Docker

Docker control is **per node** and **off until you enable it**. The server
never opens a remote engine socket. The agent on that machine talks to
Docker Engine locally (default `/var/run/docker.sock`). Anyone who can use
that socket can take over the host — treat Observe/Manage/Exec as
root-equivalent on that box.

## Enable it

On the node **Settings** tab:

1. **Observe Docker** — list and inspect containers, Compose projects,
   images, volumes, and networks; stream logs and stats. No start/stop.
2. **Allow mutations** — start, stop, restart, kill, remove; Compose
   up/down/pull; image pull/prune/remove; volume and network create/remove.
3. **Allow `docker exec`** — run a process in a container. This is a root
   shell on the host namespaces. Default **off**. Turn it on only if you
   need it.

The add-node form’s “runs Docker” checkbox only turns on Observe.

A connected agent picks these flags up immediately (`set_runtime` on the
session). Until the first connect, `agent.toml` `[docker]` values are a
fallback. After that, the UI wins for enable/manage/exec and Compose paths.

The socket path stays in `agent.toml` as `docker.host` only if it is not
the default Unix socket. The packaged agent user is in the `docker` group
when Docker was installed at package time; if you installed Docker later,
add `keystone` to `docker` and restart the agent.

## What the tabs do

With Observe on and a live session:

- **Containers** — list, inspect, logs, live stats; with Manage, start /
  stop / restart / kill / remove.
- **Compose** — project status; with Manage, up / down / pull / logs.
  **Compose files** on Settings are extra `-f` paths when a command does
  not name a file (one path per line).
- **Images** — list and inspect; pull and prune forms when Manage is on.
- **Volumes** and **Networks** — list/inspect; create/remove with Manage.

Mutations require a **logged-in UI session**. The ingest token used by
agents cannot call these actions. Every mutation is written to the audit
log (who, node, operation, target, success).

Exec is a separate checkbox and a separate gate on the agent. Manage
without Exec still cannot exec.

## If the tabs are empty or error

- Observe is off on that node.
- The agent is not control-connected (see [Troubleshooting](troubleshooting.md)).
- The `keystone` user cannot open the socket (`Permission denied`).
- Docker is not installed, or `docker.host` points at the wrong path.

The Overview still works without Docker. Container CPU/memory gauges on the
dashboard only appear when Observe is on and the agent is pushing container
aggregates.
