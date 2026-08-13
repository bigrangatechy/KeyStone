<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Install

Rust 1.85+ is required to build from source.

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
