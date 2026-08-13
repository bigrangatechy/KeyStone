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

They do not conflict. `cargo-deb` metadata is in each crate’s
`Cargo.toml`. Assets and maintainer scripts: `packaging/deb/server/` and
`packaging/deb/agent/`.

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
