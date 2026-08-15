<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Install

Two `.deb` packages, on purpose: **one UI** for metrics and Docker, agents
on every other machine — not Portainer on a Docker host and Netdata on each
Pi.

| Package | Role | Install on |
|---|---|---|
| `keystone-server` | HTTP UI + gRPC ingest | **One** machine |
| `keystone-agent` | Collectors + optional Docker | **Every** node you want in the dashboard |

They do not conflict. Put both on the UI host if that box should show up as a
node too.

Homelab default is Debian / Ubuntu / 64-bit Raspberry Pi OS. There is no node
cap: install the agent on as many boxes as you want.

## Packages (amd64 and arm64)

CI builds `.deb` files for `amd64` (PCs, most VMs) and `arm64` (Raspberry
Pi 4 and 5 on 64-bit Raspberry Pi OS). 32-bit Raspberry Pi OS (`armhf`)
is not packaged; use the 64-bit image.

On a Pi, check:

```
dpkg --print-architecture
uname -m
```

You want `arm64` and `aarch64`.

Install the matching artifacts (example, 64-bit Pi):

```
# The one UI box (optional: agent as well, so this host is in the list)
sudo apt install ./keystone-server_0.1.0-1_arm64.deb
sudo apt install ./keystone-agent_0.1.0-1_arm64.deb   # optional on the UI host

# Every other node: agent only
sudo apt install ./keystone-agent_0.1.0-1_arm64.deb
```

On a PC or VM, use the `amd64` files the same way.

## First start (server)

Edit `/etc/keystone/server.toml` only for:

- `http_listen` — UI (default `0.0.0.0:8080`)
- `grpc_listen` — agent ingest (default `0.0.0.0:9100`)
- `data_dir` — SQLite, sessions, series store (packaged default `/var/lib/keystone`)
- `[auth] username` — local admin name (default `admin`)

Do **not** keep changing retention, scrape jobs, or the ingest token in this
file after the first successful start. Those are seeded once, then edited on
the **Settings** page. See [Configuration](configuration.md).

Set the first admin password in `/etc/default/keystone-server`:

```
KEYSTONE_ADMIN_PASSWORD=a-password-you-chose
```

Alternatively leave `auth.password_hash` empty and set the environment for
one start, or put a hash from `keystone hash-password` into `server.toml`.

Then:

```
sudo systemctl enable --now keystone-server
```

Open `http://<that-host>:8080`, log in, and go to **Settings**. Confirm:

- Ingest token (seeded from `ingest_token` in the file, or generated if empty)
- Series retention (default **24 hours**)
- Any Prometheus or SNMP scrape jobs you want

Clear `KEYSTONE_ADMIN_PASSWORD` from `/etc/default/keystone-server` after the
hash exists so the password is not sitting in an env file.

## First start (agent)

Prefer **Add node** in the UI: it writes a short `agent.toml` with ingest URL,
token, and node id. Copy that file to `/etc/keystone/agent.toml` on the node
and:

```
sudo systemctl enable --now keystone-agent
```

If you skip the form, edit `/etc/keystone/agent.toml` yourself:

```
ingest_url = "http://keystone.home.arpa:9100"
ingest_token = "the-token-from-Settings"
# node_id defaults to hostname when omitted
buffer_dir = "/var/lib/keystone/agent-buffer"
```

`ingest_url` is the **gRPC** address (`grpc_listen` on the server), not the
HTTP UI port.

Poll interval, Docker, labels, and Compose paths are **node Settings** after
the agent connects. The only Docker field that stays in `agent.toml` is
`docker.host`, and only if the engine socket is not `/var/run/docker.sock`.

If Docker is installed, the package adds the `keystone` user to the `docker`
group so the agent can use the socket. You still have to enable Observe /
Manage / Exec on that node in the UI.

```
sudo systemctl enable --now keystone-agent
```

## Raspberry Pi 4 / 5

- Use **64-bit Raspberry Pi OS** (Bookworm or newer). Pi 5 is 64-bit only;
  Pi 4 should run the 64-bit image as well.
- **Agent** is fine on a Pi 4 with 1 GB. That is the usual role for a Pi
  in the lab (Pi-hole, MQTT, a camera box).
- **Server** wants headroom: Pi 4 with 2 GB+ or a Pi 5 if this Pi is the
  central KeyStone box. An x86 VM or the Docker host is also a good server.

CI builds on Debian Bookworm (glibc 2.36), which matches 64-bit Raspberry Pi
OS Bookworm. A `.deb` built on a newer PC may depend on a newer libc and
refuse to install on the Pi — use the CI artifacts.

## From source (no package)

Rust 1.85+ is required.

```
cargo build --release -p keystone-server -p keystone-agent
```

Binaries: `target/release/keystone` (server) and
`target/release/keystone-agent`. Copy example configs from `examples/` and
set `KEYSTONE_ADMIN_PASSWORD` on first server start. Matching ingest token
on Settings and each agent.

The agent defaults to hostname as `node_id`. Enable Docker from that node’s
**Settings** after it connects.

There is no license file or seat count to raise when you add nodes.

## Add a node (one UI)

The dashboard runs on **one** machine. Other boxes only run `keystone-agent`.
After the server is up, open the UI, click **Add node**, and enter the
hostname. KeyStone registers it as “awaiting agent” and shows a generated
`agent.toml` (ingest URL on the gRPC port, shared token, `node_id`).

Install that config on the remote host and start the agent. The next
heartbeat replaces “awaiting agent” with “connected”. You can also skip the
form: an unknown agent that connects with a matching token is enrolled
automatically.

If you ticked “this node runs Docker” on the add-node form, Observe Docker
is already enabled on that node’s Settings when the agent appears.

## Upgrades

Installing a newer `.deb` over an older one restarts **only**
`keystone-server` and/or `keystone-agent`. It does not stop Docker Engine,
does not prune images or volumes, and does not run Compose down. Containers
keep running. KeyStone never `Depends:` on `docker.io` / `docker-ce`, so
`apt upgrade` of KeyStone will not pull an Engine upgrade that would bounce
every container.

`/etc/keystone/*.toml` and `/etc/default/keystone-*` are conffiles: dpkg
keeps your edits (it may prompt if a packaged default changed).

`apt remove` stops the KeyStone unit and leaves data on disk. `apt purge`
deletes only KeyStone’s own state: `keystone.sqlite` / `series.redb` for
the server, `agent-buffer` for the agent. It never touches `/var/lib/docker`.
Purging one package will not delete the other’s files under
`/var/lib/keystone`.

Do not turn `/var/lib/keystone` into a symlink (or bind-mount) of `/`,
`/var/lib/docker`, or another tree. Maintainer scripts refuse to `chown`
through a symlink there so an upgrade cannot take the OS or Engine with it.

