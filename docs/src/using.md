<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Using the UI

The HTTP UI is the only console. Agents do not serve a web page. The home
page is the Netdata-shaped **fleet** (live chips per host). Open a node for
that machine’s Overview; the Docker tabs are the Portainer-shaped control
plane.

## Sign in

Open the listen address from `http_listen` (packaged default port **8080**).
This version is a **single local admin** account: username from
`server.toml` (`admin` unless you changed it), password from the first-start
hash.

Sessions are cookie-based. Use **Log out** in the header when you are done.
Put TLS in front of the UI if the network is not yours — KeyStone does not
terminate HTTPS itself.

`GET /health` returns `ok` without a session, for a reverse-proxy check.

## Nodes list

The home page lists every enrolled node with live **CPU, RAM, disk, and
temperature** chips (the Netdata-shaped fleet view). Click a host for that
machine’s Overview and Docker tabs.

Statuses you will see:

| State | Meaning |
|---|---|
| **Awaiting agent** | You added the node in the UI; no matching agent has connected yet. Open **install agent** for the snippet. |
| **Control connected** | A live gRPC session is open. Metrics are flowing; Docker commands can be sent if enabled. |
| **Seen, not connected** | Heartbeats arrived recently but the session dropped (restart, network blip). |
| **Offline** | No recent heartbeat. |

Click a row to open that node. **Add node** is the enroll form. Chips stay
blank (`—`) until the agent has pushed samples. Disk is the fullest real
filesystem (overlay/tmpfs skipped). Temperature is the CPU package when
the kernel exposes it, otherwise the hottest hwmon reading. The list
refreshes about once a second.

## Add node

Enter a hostname (used as the default `node_id` unless you override it).
Optionally tick that the box runs Docker — that only pre-enables **Observe
Docker** on the node; Manage and Exec stay off until you turn them on.

The setup page is the install snippet: ingest URL (gRPC port), ingest token,
`node_id`, and `buffer_dir`. Copy it to `/etc/keystone/agent.toml` on the
target and start `keystone-agent`. You can reopen **install agent** from the
node header later.

Unknown agents that present the current ingest token are enrolled without
the form. Their `node_id` is whatever the agent sent (hostname if unset).

## Node page

Tabs:

- **Overview** — widget dashboard. See [Dashboards](dashboard.md).
- **Containers / Compose / Images / Volumes / Networks** — Docker Engine on
  *this* node. Tables for lists; **Logs** follows that container or Compose
  project. Empty or an explanation until Observe Docker is on and the agent
  can use the socket. See [Docker](docker.md).
- **Settings** — display name, notes, poll interval, NICs, labels, Docker
  flags, Compose paths. See [Configuration](configuration.md).

The header shows `node_id`, OS, kernel, agent version, last seen, and a link
back to the install snippet.

**All samples** at the bottom of Overview is the raw allowlisted series for
debugging, not the usual way to watch a host.

## Global Settings

The header **Settings** link is the server, not a node: retention, ingest
token, Prometheus/SNMP scrape jobs, admin password. Listen addresses and the
admin username stay in `server.toml`.
