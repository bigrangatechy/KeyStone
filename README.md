<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# KeyStone

Unlimited-node server monitoring with per-node Docker management. Licensed
under **GPL-2.0-or-later**. There is no node cap in the software.

Agents push host (and optional container) metrics to a central server. The
server can also scrape Prometheus exporters and SNMP devices. Each node has
a dedicated view for metrics and, when enabled, full Docker Engine control
(containers, Compose, images, volumes, networks).

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

They do not conflict. CI builds `amd64` and `arm64` (Pi 4/5). See [docs/src/install.md](docs/src/install.md).

```
# UI machine
sudo apt install ./keystone-server_0.1.0-1_arm64.deb ./keystone-agent_0.1.0-1_arm64.deb
sudo systemctl enable --now keystone-server keystone-agent

# Every other node
sudo apt install ./keystone-agent_0.1.0-1_arm64.deb
sudo systemctl enable --now keystone-agent
```

Build with `cargo deb -p keystone-server` and `cargo deb -p keystone-agent`.

## Install from source

Rust 1.85 or newer is required.

```
cargo build --release -p keystone-server -p keystone-agent
```

Binaries: `target/release/keystone` (server) and
`target/release/keystone-agent`.

Example configs: [examples/server.toml](examples/server.toml),
[examples/agent.toml](examples/agent.toml).

```
keystone --config examples/server.toml
keystone-agent --config examples/agent.toml
```

Set `KEYSTONE_ADMIN_PASSWORD` on first server start (or put a hash in
config). The ingest token is seeded from the server config (or
`KEYSTONE_INGEST_TOKEN`) and then edited in **Settings**. Agents must
present the same token.

## Living documentation

Reference docs are generated from the same types the process runs:

```
cargo xtask docs
```

The running server serves **this version's** docs at `/help` (after login).
The mdBook in `docs/` is published by GitLab Pages. Do not copy metric
tables into this README; they will go stale. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Architecture (short)

- **Agent** collects catalog metrics, optionally talks to Docker via the
  local engine socket, and opens a gRPC session to the server (push +
  control, NAT-friendly).
- **Server** stores series, scrapes Prometheus/SNMP, and renders the
  per-node UI. It never connects to a remote `docker.sock`.
- **Catalog** is an allowlist: unknown metric names are dropped.

## Security

Docker socket access is root-equivalent on that host. Enable Docker and
`docker.manage` from the node’s Settings (opt-in). Container `exec` is off
by default. Mutating Docker calls require a logged-in UI session, not the
ingest token. See the threat-model chapter in the book.
