<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Using the UI

The HTTP UI is the only console. Agents do not serve a web page. The home
page is the Netdata-shaped **fleet** (live chips per host). Open a node for
that machine’s Overview; the Docker tabs are the Portainer-shaped control
plane; **System** is health vs actions on the machine itself (apt, leftover
services, confirmed reboot, allowlisted journals, NTP, unattended-upgrades,
IPv4, GitLab Omnibus backup — not Docker).

## Sign in

Open the listen address from `http_listen` (packaged default port **8080**).
This version is a **single local admin** account: username from
`server.toml` (`admin` unless you changed it). Packaged first start is
password **`changeme`** (or `KEYSTONE_ADMIN_PASSWORD` if you set it).

The first sign-in asks you to **choose a new password** (at least 8
characters, not the bootstrap one). You cannot use the rest of the UI until
that is saved. After that, a short **welcome tour** points at Nodes, Alerts,
Audit, Settings, and Add node. Skip it anytime; replay it from Settings.

If you enabled an **authenticator** on Settings, sign-in asks for a 6-digit
code (or a backup code) after the password. See [Security](security.md).

Sessions are cookie-based. The UI stays signed in while a KeyStone tab is
open (including Compose/container logs, and while that tab is in the
background). After **two hours** with no heartbeat (discarded tab, asleep
laptop) you will need to sign in again. Closing a tab does not sign you
out — use **Log out** in the header, or quit the browser. If `[tls]` is set
in `server.toml`, open `https://` on `http_listen`. You
can still put a reverse proxy in front instead; see [Security](security.md).

`GET /health` returns `ok` without a session, for a reverse-proxy check.

## Nodes list

The home page lists every enrolled node with live **CPU, RAM, disk, and
temperature** chips (the Netdata-shaped fleet view). Click a host for that
machine’s Overview, Docker tabs, and System tab.

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
refreshes about once a second. A red count next to a hostname is the number
of chips currently warn or crit; open [Alerts](alerts.md).

## Add node

Enter a hostname (used as the default `node_id` unless you override it).
Optionally tick that the box runs Docker — that only pre-enables **Observe
Docker** on the node; Manage and Exec stay off until you turn them on.

The ingest URL defaults to **`mdns`**: the agent browses the LAN for this
UI (same broadcast domain, UDP 5353) and does not need this machine’s IP.
Paste an `http://host:9100` URL instead if the node is on another subnet
or multicast is blocked. (gRPC port **9100**, not the UI on **8080**.)
Ingest TLS needs a hostname that matches the certificate, so that path
fills the explicit `https://` URL.

The setup page is the install snippet: ingest URL, ingest token, `node_id`,
and `buffer_dir`. Copy it to `/etc/keystone/agent.toml` on the target and
start `keystone-agent`. You can reopen **install agent** from the node
header later.

Unknown agents that present the current ingest token are enrolled without
the form. Their `node_id` is whatever the agent sent (hostname if unset).
A packaged agent with `ingest_url = "mdns"` and a matching token can
appear on the home page without Add node at all.

## Node page

Tabs:

- **Overview** — widget dashboard. See [Dashboards](dashboard.md).
- **Containers / Compose / Images / Volumes / Networks** — Docker Engine on
  *this* node. Containers are cards (click for details); the other Docker
  tabs are tables. **Logs** follows that container or Compose
  project. Images can search Docker Hub to fill Pull. Empty or an explanation
  until Observe Docker is on and the agent can use the socket. See
  [Docker](docker.md).
- **System** — health on the left (leftovers, failed units, journals, NTP,
  unattended-upgrades, addresses) and actions on the right (apt, autoremove,
  reboot, GitLab backup, leftover restart, IPv4) on **this** Ubuntu or Debian server. Off until you
  enable the root helper and Settings flags. If 2FA is on, changing IPv4 or
  restarting a leftover unit also asks for a current authenticator code. Proxmox, TrueNAS, and other
  appliance OSes stay on Observe. See [System](system.md).
- **Settings** — display name, notes, poll interval, NICs, labels, Docker
  flags, Compose paths, System-admin flags. See [Configuration](configuration.md).

The header shows `node_id`, OS, kernel, agent version, last seen, and a link
back to the install snippet.

**All samples** at the bottom of Overview is the raw allowlisted series for
debugging, not the usual way to watch a host.

## Global Settings

The header **Settings** link is the server, not a node: retention, ingest
token, Prometheus/SNMP scrape jobs, optional alert webhook, admin password,
authenticator 2FA (and **Replay welcome tour**). Listen addresses and the
admin username stay in `server.toml`. Header **Alerts** is the live firing
list; see [Alerts](alerts.md). Header **Audit** is Docker and System
mutations from this UI (newest first); see [Audit](audit.md). The home page
reminds you to enable 2FA if it is still off.
