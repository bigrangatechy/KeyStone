<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Configuration

KeyStone splits **bootstrap** (files and environment, needed before the UI
exists) from **operator settings** (the UI after first start).

## What stays in files

### Server — `/etc/keystone/server.toml`

| Field | Meaning |
|---|---|
| `http_listen` | UI and HTTP API. Default `0.0.0.0:8080`. |
| `grpc_listen` | Agent ingest. Default `0.0.0.0:9100`. |
| `data_dir` | SQLite, session cookies’ store, series database. Packaged: `/var/lib/keystone`. |
| `[auth] username` | Local admin name. Default `admin`. |
| `[auth] password_hash` | Argon2id hash. Empty means hash `KEYSTONE_ADMIN_PASSWORD` on first start. |

Restart `keystone-server` after changing listen addresses or `data_dir`.

Environment:

| Variable | Meaning |
|---|---|
| `KEYSTONE_SERVER_CONFIG` | Config path if you do not pass `--config`. Default `/etc/keystone/server.toml`. |
| `KEYSTONE_ADMIN_PASSWORD` | First-start password when `password_hash` is empty. Also used by `keystone hash-password`. |
| `KEYSTONE_INGEST_TOKEN` | If set, **always** overrides the ingest token stored in Settings. The Settings field is read-only until you unset it. |

### Agent — `/etc/keystone/agent.toml`

| Field | Meaning |
|---|---|
| `ingest_url` | gRPC URL of the server, e.g. `http://keystone.home.arpa:9100`. |
| `ingest_token` | Must match Settings (or `KEYSTONE_INGEST_TOKEN`). |
| `node_id` | Stable id. Empty = hostname. |
| `buffer_dir` | On-disk push buffer while the server is unreachable. Packaged: `/var/lib/keystone/agent-buffer`. |
| `docker.host` | Engine socket or TCP URL. Empty = `/var/run/docker.sock`. Not a UI field. |

`interval_secs`, `[docker] enabled/manage/allow_exec`, `compose_paths`, and
`labels` in the agent file are **fallbacks until the node connects**. After
that, node Settings replace them at runtime. You do not need to restart the
agent to change poll interval or Docker flags.

`KEYSTONE_AGENT_CONFIG` overrides the default path. `KEYSTONE_INGEST_TOKEN`
on the **agent** host can fill `ingest_token` if you prefer not to put it in
the file (same name as the server override — set it only where you mean it).

## Server Settings (UI)

Header **Settings**. Stored in SQLite after first start. TOML values for
these keys are copied **once** when the settings row is created.

### Series retention

How long metric history is kept, in hours. Default **24**. Range **1–8760**
(one year). Sparklines and anything that reads history use this window.
Changing it applies to new writes; you do not restart the server.

### Ingest token

Shared secret agents present on the gRPC session. Empty allows **any**
token — only for local development. **Generate new ingest token** replaces
it with a random value; every agent must be updated and restarted (the
token is read from `agent.toml` at process start).

The ingest token cannot log into the UI and cannot run Docker mutations.

### Prometheus scrape

One job per line:

```
name | url | interval_secs | node_id
```

`interval_secs` and `node_id` are optional (interval defaults to 30, minimum
5). Blank lines and `#` comments are ignored.

Example:

```
local-node-exporter | http://127.0.0.1:9100/metrics | 15 | exporter-local
```

The server HTTP-GETs the exposition URL. Only metric names in KeyStone’s
allowlist are stored; everything else is dropped. Samples attach to
`node_id` if set, otherwise to `name` (that string appears as a node).

Saving Settings reloads scrape workers. You do not restart the process.

### SNMP scrape

```
name | target | community | interval_secs | node_id
```

`target` is `host:port` (port defaults to 161). Community defaults to
`public`. This version reads `sysUpTime.0` and a scrape-ok flag — enough to
see that a switch answers, not a full NMS.

### Alert webhook

Optional URL. Empty is off. The server POSTs JSON when a fleet chip starts
firing, changes severity, or clears. See [Alerts](alerts.md). Only
`http://` and `https://` are accepted. Ingest does not wait on the POST.

### Admin password

Username stays in `server.toml`. Leave the password fields empty to keep
the current hash. New password must be entered twice.

## Node Settings (UI)

On each node’s **Settings** tab:

| Field | Meaning |
|---|---|
| Display name | Shown instead of hostname when set. |
| Notes | Free text for you. |
| Poll interval | Agent push **and** Overview refresh, 1–60 seconds, default 1. |
| Network interfaces | One device name per line. Empty = automatic (skip loopback/docker/veth). |
| Labels | `key=value` per line, attached to the heartbeat. Replaces `agent.toml` labels once connected. |
| Observe / Manage / Exec | Docker gates. See [Docker](docker.md). |
| Compose files | Extra `docker compose -f` paths, one per line. |

Save applies to a connected agent immediately.

## Commands

```
keystone serve --config /etc/keystone/server.toml
keystone hash-password
keystone docs
keystone docs --section install
keystone-agent --config /etc/keystone/agent.toml
```

`keystone` with no subcommand is `serve`. `keystone docs` prints this
operator book (the same markdown as `/help`).
