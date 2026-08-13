<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Architecture

KeyStone is a Rust workspace. Agents **push** over a bidirectional gRPC
stream; the server never dials an agent and never opens a remote
`docker.sock`.

```
  agent (collectors + optional bollard)
       │  gRPC Session: PushFrame / Command / StreamChunk
       ▼
  keystone-server
       ├── HTTP UI + cookie session (axum)
       ├── ingest (tonic)
       ├── scrape (Prometheus HTTP, SNMP GET)
       ├── keystone-store (SQLite metadata + Redb series)
       └── widgets hydrate → JSON for app.js
```

## Crates

| Crate | Binary / role |
|---|---|
| `keystone-core` | Catalog, `DockerOp`, `Permission`, configs, `NodeSettings` / `ServerSettings`, widget kinds and hydrate. No I/O. |
| `keystone-proto` | Generated from `proto/ingest.proto`. |
| `keystone-store` | `keystone.sqlite` + `series.redb`. |
| `keystone-agent` | `keystone-agent`: sysinfo / hwmon / GPU, Docker handle, session client. |
| `keystone-server` | `keystone`: UI, ingest, scrape, `/help`. |

Do not `sudo cargo`. Prefer `TMPDIR=.smoke/tmp` if `/tmp` is full. Smoke
data dir in examples is `.smoke`.

## Control flow

1. Agent opens `Ingest.Session`, sends `PushFrame` (heartbeat + samples +
   token) on an interval.
2. Server allowlists samples, upserts the node row, stores series, ACKs.
3. On first good push for a node id, the server registers the stream in
   `AgentRegistry` and sends `set_runtime` (poll interval, labels, Docker
   flags, compose paths) from `NodeSettings`.
4. UI Docker POSTs become `Command { op, payload_json }` on that stream.
   The agent runs `DockerOp`, returns `CommandResult`. Logs/stats can use
   `StreamChunk` (current UI often waits for a single result payload).
5. Overview polls `GET /api/v1/nodes/{id}/dashboard` at `poll_secs`.

`set_interval` still exists on the agent for older payloads; current servers
send `set_runtime`.

## What is not in this slice

SSO, multi-user RBAC enforcement beyond the permission enum, HTTPS
termination, remote Docker, 32-bit ARM packages, a node cap.
