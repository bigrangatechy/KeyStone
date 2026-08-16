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

CI builds `.deb` files for `amd64` (PCs, Ubuntu/Debian VMs) and `arm64`
(Raspberry Pi 4 and 5 on 64-bit Raspberry Pi OS). 32-bit Raspberry Pi OS
(`armhf`) is not packaged; use the 64-bit image.

Match the filename to the machine. Check:

```
dpkg --print-architecture
```

| That prints | Use |
|---|---|
| `amd64` | `*_amd64.deb` (typical Ubuntu server) |
| `arm64` | `*_arm64.deb` (64-bit Pi; `uname -m` is `aarch64`) |

Installing an `arm64` file on `amd64` (or the other way around) fails.
`apt` may say `Unsupported file` instead of a clear architecture error.

**Ubuntu / Debian PC or VM (`amd64`):**

```
# Copy out of ~/Downloads first if apt complains that _apt cannot read the file
sudo cp keystone-server_0.1.0-8_amd64.deb keystone-agent_0.1.0-6_amd64.deb /tmp/

# The one UI box (optional: agent as well, so this host is in the list)
sudo apt install /tmp/keystone-server_0.1.0-8_amd64.deb
sudo apt install /tmp/keystone-agent_0.1.0-6_amd64.deb   # optional on the UI host

# Every other node: agent only
sudo apt install /tmp/keystone-agent_0.1.0-6_amd64.deb
```

A **Notice** about `_apt` / `pkgAcquire::Run (13: Permission denied)` means
apt could not sandbox-read a file under your home directory. The install
often still succeeds. Installing from `/tmp` (world-traversable) avoids it.
Check with `dpkg -l keystone-server keystone-agent`.

**64-bit Raspberry Pi (`arm64`):** same commands with `arm64` in the
filename instead of `amd64`.

## First start (server)

Edit `/etc/keystone/server.toml` only for:

- `http_listen` — UI (default `0.0.0.0:8080`)
- `grpc_listen` — agent ingest (default `0.0.0.0:9100`)
- `data_dir` — SQLite, sessions, series store (packaged default `/var/lib/keystone`)
- `[auth] username` — local admin name (default `admin`)
- optional `[tls]` — see [Security](security.md#tls) before the UI or ingest
  is reachable from a network you do not trust

Do **not** keep changing retention, scrape jobs, or the ingest token in this
file after the first successful start. Those are seeded once, then edited on
the **Settings** page. See [Configuration](configuration.md).

Then:

```
sudo systemctl enable --now keystone-server
```

Open `http://<that-host>:8080` (not `https://` unless you enabled `[tls]`).
First sign-in is **`admin` / `changeme`**. The UI then requires a new
password (8+ characters, not `changeme`). To pick the bootstrap password
yourself instead, set `KEYSTONE_ADMIN_PASSWORD` in
`/etc/default/keystone-server` before the first start, or put a hash from
`keystone hash-password` in `server.toml`.

Go to **Settings**. Confirm:

- Ingest token (seeded from `ingest_token` in the file, or generated if empty)
- Series retention (default **24 hours**)
- Any Prometheus or SNMP scrape jobs you want

If you set `KEYSTONE_ADMIN_PASSWORD`, clear it from
`/etc/default/keystone-server` after the hash exists so the password is not
sitting in an env file.

If this UI will sit behind a reverse proxy or Cloudflare Tunnel, enable an
**authenticator** on Settings before the hostname is public. See
[Security](security.md). Do not publish port 8080 to the internet in
plaintext. In-tree TLS (`[tls]` in `server.toml`) encrypts the UI and,
by default, agent ingest as well.

## First start (agent)

Prefer **Add node** in the UI: it writes a short `agent.toml` with ingest
URL (`mdns` on a typical LAN), token, and node id. Copy that file to
`/etc/keystone/agent.toml` on the node and:

```
sudo systemctl enable --now keystone-agent
```

If you skip the form, the packaged agent already uses mDNS. Set
`ingest_token` to the value on **Settings** (not `change-me` if you
rotated it) and start the unit. Or edit `/etc/keystone/agent.toml`
yourself. The `keystone` user must be able to read that file. The
package sets `/etc/keystone` to `root:keystone` mode `0750`. If you
create the directory by hand:

```
sudo install -d -m 0750 -o root -g keystone /etc/keystone
```

A `0750` directory owned by `root:root` is invisible to the agent (it
looks like a missing config). `sudo -u keystone cat /etc/keystone/agent.toml`
must work. The agent **exits** if the file is missing or unreadable; it
does not fall back to localhost.

```
ingest_url = "mdns"
# ingest_url = "http://192.168.1.10:9100"  # other subnet / no multicast
ingest_token = "the-token-from-Settings"
# node_id defaults to hostname when omitted
buffer_dir = "/var/lib/keystone/agent-buffer"
# tls_ca_file = "/etc/keystone/ca.pem"  # https:// ingest with a private CA
```

`ingest_url` is the **gRPC** address (`grpc_listen` on the server), not the
HTTP UI port — except `mdns`, which finds that address on the LAN. Use
`https://` when the server has ingest TLS (mDNS cannot satisfy certificate
names; set the URL by hand).

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
set `KEYSTONE_ADMIN_PASSWORD` on first server start if you do not want
`admin` / `changeme`. Matching ingest token
on Settings and each agent.

The agent defaults to hostname as `node_id`. Enable Docker from that node’s
**Settings** after it connects.

There is no license file or seat count to raise when you add nodes.

## Add a node (one UI)

The dashboard runs on **one** machine. Other boxes only run `keystone-agent`.
After the server is up, open the UI, click **Add node**, and enter the
hostname. KeyStone registers it as “awaiting agent” and shows a generated
`agent.toml` (`mdns` or an explicit gRPC URL, shared token, `node_id`).

Install that config on the remote host and start the agent. The next
heartbeat replaces “awaiting agent” with “connected”. You can also skip the
form: an unknown agent that connects with a matching token is enrolled
automatically — including a packaged agent that found the UI via mDNS.

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
keeps your edits (it may prompt if a packaged default changed). Keep the
installed file (`N`) unless you intend to take the new defaults. A new
agent package defaults `ingest_url` to `mdns`; that does **not** apply
until you accept the new file or edit the existing one. `ingest_token`
must still match **Settings** — do not reset it to `change-me` if you
already generated a token.

Same Debian revision (`0.1.0-1` over itself) looks like “already the
newest version” and does not replace the binary; use
`apt install --reinstall ./….deb`. A newer revision (`0.1.0-8`) is a
normal upgrade.

Do **not** `apt purge` to pick up a new binary: purge deletes
`keystone.sqlite` (admin password, 2FA, nodes) and `series.redb`.
`apt remove` without purge, or installing over the top, keeps that state.

`apt remove` stops the KeyStone unit and leaves data on disk. `apt purge`
deletes only KeyStone’s own state: `keystone.sqlite` / `series.redb` for
the server, `agent-buffer` for the agent. It never touches `/var/lib/docker`.
Purging one package will not delete the other’s files under
`/var/lib/keystone`.

Do not turn `/var/lib/keystone` into a symlink (or bind-mount) of `/`,
`/var/lib/docker`, or another tree. Maintainer scripts refuse to `chown`
through a symlink there so an upgrade cannot take the OS or Engine with it.

