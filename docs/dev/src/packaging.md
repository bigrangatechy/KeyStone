<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Packaging and CI

## Binaries

| Package | Binary | Unit |
|---|---|---|
| `keystone-server` | `/usr/bin/keystone` | `keystone-server.service` |
| `keystone-agent` | `/usr/bin/keystone-agent` | `keystone-agent.service` |
| (same agent `.deb`) | `/usr/lib/keystone/keystone-sys` | `keystone-sys.socket` + `.service` (**not** enabled by the package) |

They do not conflict. `cargo-deb` metadata is in each crate’s
`Cargo.toml`. Assets and maintainer scripts: `packaging/deb/server/` and
`packaging/deb/agent/`. `systemd-units` only auto-enables `keystone-agent`.
The sys helper units are extra assets; the System tab tells the operator
to `systemctl enable --now keystone-sys.socket`.

## Upgrade safety

Worst case for a homelab is an upgrade that `chown -R`s or `rm -rf`s into
Docker’s data root or `/`. Invariants (enforced by
`crates/keystone-core/src/packaging_safety.rs`):

- Never `Depends`/`Recommends` `docker.io`, `docker-ce`, `containerd`, or
  `podman`. Socket access is optional (`keystone` in group `docker`).
- Never `Requires=` / `BindsTo=` / `PartOf=` Docker in the units (including
  `keystone-sys`). The agent may `After=docker.socket` so the socket exists
  when Engine is installed. The agent unit keeps `NoNewPrivileges=true`.
- `postinst` creates `/var/lib/keystone` (and `agent-buffer`) and `chown`s
  **that directory only**. No `chown -R`. Abort if the path is a symlink
  or is `/`, `/var/lib/docker`, etc.
- `postinst` also sets `/etc/keystone` to `root:keystone` mode `0750` (the
  service user must read toml; `0750` `root:root` hides the file). Own
  that inode only — never `chown -R`.
- `prerm` / `#DEBHELPER#` stop `keystone-*` only.
- `postrm purge` deletes named KeyStone files (`keystone.sqlite`,
  `series.redb`) or `/var/lib/keystone/agent-buffer`. Never
  `rm -rf /var/lib/keystone` (the other package may still own files there)
  and never `/var/lib/docker`.

An upgrade restarts the KeyStone unit. Containers keep running. Mutating
Docker is only a logged-in UI action while Manage is on.

## Local `.deb`

```
cargo install cargo-deb --locked
cargo deb -p keystone-agent
cargo deb -p keystone-server
```

Native amd64 artifacts land under `target/debian/`. If `CARGO_TARGET_DIR`
is set (for example `.smoke/target`), look there instead.

On a Pi, building natively avoids a newer glibc than Bookworm.

## Cross-compile arm64 from amd64

Needs `gcc-aarch64-linux-gnu` and the Rust target:

```
rustup target add aarch64-unknown-linux-gnu
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
cargo deb -p keystone-agent --target aarch64-unknown-linux-gnu
cargo deb -p keystone-server --target aarch64-unknown-linux-gnu
```

CI (`package:amd64`, `package:arm64`) uses `rust:bookworm` so glibc matches
64-bit Raspberry Pi OS Bookworm. A `.deb` built on a newer host may not
install on the Pi.

`armhf` is not a target.

## CLI worth knowing

```
keystone serve -c /etc/keystone/server.toml
keystone hash-password          # stdin or KEYSTONE_ADMIN_PASSWORD
keystone docs [--section slug]
keystone-agent -c /etc/keystone/agent.toml
```

Env: `KEYSTONE_SERVER_CONFIG`, `KEYSTONE_AGENT_CONFIG`,
`KEYSTONE_ADMIN_PASSWORD`, `KEYSTONE_INGEST_TOKEN` (server override and
agent token fill-in).

## Checks

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
mdbook build docs
mdbook build docs/dev
```

GitLab: `fmt`, `clippy`, `test`, `deny`, `reuse`, then `docs` (both
mdBooks), then `.deb` jobs, then `pages` from the operator book plus
`/dev/`. SPDX headers on new source; `REUSE.toml` annotations for files
that cannot take a header.

License: GPL-2.0-or-later. Prefer MIT-or-Apache dependencies and take MIT
when both are offered (Apache-2.0-only in a GPLv2-or-later binary forces
GPLv3 at distribution time).
