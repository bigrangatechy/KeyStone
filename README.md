<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# KeyStone

A homelab replacement for **Portainer** and **Netdata**: one UI for live
host metrics and Docker Engine on every machine. Licensed under
**GPL-2.0-or-later**. There is no node cap in the software.

Agents push host (and optional container) metrics to a central server. The
server can also scrape Prometheus exporters and SNMP devices. Each node has
a customisable metrics overview and, when you enable it, Docker control
(containers, Compose, images, volumes, networks, logs) through the **local**
engine socket — the server never opens a remote `docker.sock`.

## License

KeyStone is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free
Software Foundation, either version 2 of the License, or (at your option)
any later version. See [COPYING](COPYING).

A binary linked with Apache-2.0-only crates is distributed under GPLv3
(the `or-later` choice). Prefer MIT-or-Apache dependencies and take the MIT
side when both are offered.

## Install

Two Debian packages. That is the easy path: **one UI**, agents everywhere else.

| Package | On which machines | Unit |
|---|---|---|
| `keystone-server` | One box (the dashboard) | `keystone-server.service` |
| `keystone-agent` | Every box you want in the UI, including the server host if you want it monitored | `keystone-agent.service` |

They do not conflict. CI builds `amd64` and `arm64` (Pi 4/5). See
[docs/src/install.md](docs/src/install.md).

```
# Match dpkg --print-architecture (amd64 on typical Ubuntu, arm64 on a 64-bit Pi)
sudo apt install ./keystone-server_0.1.0-4_amd64.deb ./keystone-agent_0.1.0-4_amd64.deb
sudo systemctl enable --now keystone-server keystone-agent

# Every other node
sudo apt install ./keystone-agent_0.1.0-4_amd64.deb
sudo systemctl enable --now keystone-agent
```

## Install from source

Rust 1.85 or newer is required.

```
cargo build --release -p keystone-server -p keystone-agent
```

Binaries: `target/release/keystone` (server) and
`target/release/keystone-agent`.

Example configs: [examples/server.toml](examples/server.toml),
[examples/agent.toml](examples/agent.toml). Those listen on **127.0.0.1:18080**
(UI) and **:19100** (gRPC) so `cargo run` can sit beside an installed
`keystone-server` on 8080/9100. Open `http://127.0.0.1:18080`.

```
mkdir -p .smoke/tmp .smoke/agent-buffer
TMPDIR=.smoke/tmp KEYSTONE_ADMIN_PASSWORD=changeme cargo run -p keystone-server -- serve --config examples/server.toml
TMPDIR=.smoke/tmp cargo run -p keystone-agent -- --config examples/agent.toml
```

First UI login is `admin` / `changeme` unless you set
`KEYSTONE_ADMIN_PASSWORD` or a hash in config. The ingest token is seeded from the server config (or
`KEYSTONE_INGEST_TOKEN`) and then edited in **Settings**. Agents must
present the same token.

## Documentation

Operator chapters live in [`docs/src/`](docs/src/) (install, Settings,
dashboards, Docker, security). The running server serves **this version**
at `/help` after login. `keystone docs` prints the same text.

How to extend the catalog, widgets, ingest protocol, and crates is in
[`docs/dev/src/`](docs/dev/src/) — not mixed into `/help`. GitLab Pages
publishes the operator book at the site root and the developer book at
`/dev/`. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Architecture (short)

- **Agent** collects catalog metrics, optionally talks to Docker via the
  local engine socket, and opens a gRPC session to the server (push +
  control, NAT-friendly).
- **Server** stores series, scrapes Prometheus/SNMP, and renders the
  per-node UI. It never connects to a remote `docker.sock`.
- **Catalog** is an allowlist: unknown metric names are dropped.

## Security

Docker socket access is root-equivalent on that host. Enable Docker and
manage from the node’s Settings (opt-in). Container `exec` is off by
default. Mutating Docker calls require a logged-in UI session, not the
ingest token. See the operator [security](docs/src/security.md) chapter.
