<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Install

Homelab default is a **.deb** on each machine (Debian, Ubuntu, Raspberry Pi
OS 64-bit). There is no node cap: install the agent on 10 boxes the same
way you install it on one.

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

Install the matching artifact (example):

```
sudo apt install ./keystone-agent_0.1.0-1_arm64.deb
```

On the box that runs the UI:

```
sudo apt install ./keystone_0.1.0-1_arm64.deb
```

Then edit `/etc/keystone/agent.toml` or `/etc/keystone/server.toml` (same
ingest token on every agent), set `KEYSTONE_ADMIN_PASSWORD` in
`/etc/default/keystone` for the first server start, and:

```
sudo systemctl enable --now keystone          # server
sudo systemctl enable --now keystone-agent    # every node
```

If Docker is installed, the `keystone` user is added to the `docker` group
so the agent can use the engine socket. Set `[docker] enabled = true` (and
`manage = true` if you want mutations) in the agent config.

### Raspberry Pi 4 / 5

- Use **64-bit Raspberry Pi OS** (Bookworm or newer). Pi 5 is 64-bit only;
  Pi 4 should run the 64-bit image as well.
- **Agent** is fine on a Pi 4 with 1 GB. That is the usual role for a Pi
  in the lab (Pi-hole, MQTT, a camera box).
- **Server** wants headroom: Pi 4 with 2 GB+ or a Pi 5 if this Pi is the
  central KeyStone box. A x86 VM or the Docker host is also a good server.
- Build on the Pi itself if you prefer not to cross-compile:

```
cargo install cargo-deb --locked
cargo deb -p keystone-agent
cargo deb -p keystone-server
```

## Build a .deb elsewhere

On amd64, with `gcc-aarch64-linux-gnu` installed:

```
rustup target add aarch64-unknown-linux-gnu
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
cargo deb -p keystone-agent --target aarch64-unknown-linux-gnu
cargo deb -p keystone-server --target aarch64-unknown-linux-gnu
```

Native amd64: `cargo deb -p keystone-agent` and `cargo deb -p keystone-server`.

CI builds on Debian Bookworm (glibc 2.36), which matches 64-bit Raspberry Pi OS
Bookworm. A `.deb` built on a newer PC may depend on a newer libc and refuse
to install on the Pi — use the CI artifacts or build inside Bookworm.

## From source (no package)

Rust 1.85+ is required.

```
cargo build --release -p keystone-server -p keystone-agent
```

Copy example configs from `examples/` and set:

- `ingest_token` (same value on server and agents)
- `KEYSTONE_ADMIN_PASSWORD` on first server start, or `keystone hash-password`

The agent defaults to hostname as `node_id`. Enable Docker on a node with:

```
[docker]
enabled = true
manage = true   # opt-in; socket access is root-equivalent
allow_exec = false
```

There is no license file or seat count to raise when you add nodes.
