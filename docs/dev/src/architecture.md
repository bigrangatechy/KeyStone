<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Architecture

The product goal is a homelab replacement for Portainer and Netdata: one
UI, unlimited Linux nodes, live host metrics, per-node Docker through a
local agent.

KeyStone is a Rust workspace. Agents **push** over a bidirectional gRPC
stream; the server never dials an agent and never opens a remote
`docker.sock`.

```
  agent (collectors + optional bollard)
       │  gRPC Session: PushFrame / Command / StreamChunk
       ▼
  keystone-server
       ├── HTTP UI + cookie session (axum), optional TOTP (`totp.rs`)
       │     optional rustls on `http_listen` (`tls.rs`)
       ├── ingest (tonic), optional rustls on `grpc_listen`
       ├── scrape (Prometheus HTTP, SNMP GET)
       ├── keystone-store (SQLite metadata + Redb series)
       └── widgets hydrate → JSON for app.js
```

## Crates

| Crate | Binary / role |
|---|---|
| `keystone-core` | Catalog, `DockerOp`, `Permission`, configs, `NodeSettings` / `ServerSettings`, widget kinds and hydrate, `fleet_chips` / alert transitions. No I/O. |
| `keystone-proto` | Generated from `proto/ingest.proto`. |
| `keystone-store` | `keystone.sqlite` + `series.redb`. |
| `keystone-agent` | `keystone-agent`: sysinfo / hwmon / GPU, Docker handle, session client. |
| `keystone-server` | `keystone`: UI, ingest, scrape, `/help`. |

Do not `sudo cargo`. Prefer `TMPDIR=.smoke/tmp` if `/tmp` is full. Smoke
data dir in examples is `.smoke`.

## Control flow

1. Agent opens `Ingest.Session`, sends `PushFrame` (heartbeat + samples +
   token) on an interval.
2. Server allowlists samples, upserts the node row, stores series, diffs
   fleet-chip alerts (`apply_node_alerts`), ACKs. Webhook POSTs (if
   configured) are `tokio::spawn`’d and must not block the ACK.
3. On first good push for a node id, the server registers the stream in
   `AgentRegistry` and sends `set_runtime` (poll interval, labels, Docker
   flags, compose paths) from `NodeSettings`.
4. UI Docker POSTs become `Command { op, payload_json }` on that stream.
   The agent runs `DockerOp` and returns `CommandResult`. Logs use
   `StreamChunk` then a result; the HTML logs page is an EventSource onto
   that stream. `cancel` aborts a follow when the browser disconnects.
5. Overview polls `GET /api/v1/nodes/{id}/dashboard` at `poll_secs`. The
   home page polls `GET /api/v1/nodes` every second for fleet chips
   (`fleet_chips` in `keystone-core`). Header **Alerts** polls
   `GET /api/v1/alerts` every 2s. A chip fires when `FleetChip::is_firing`
   (`tone` is `warn` or `crit`). Previous firing map is kv `alerts_state`
   so a restart does not re-POST the webhook.

`set_interval` still exists on the agent for older payloads; current servers
send `set_runtime`.

## What is not in this slice

SSO, multi-user RBAC enforcement beyond the permission enum, required 2FA,
WebAuthn, remote Docker, 32-bit ARM packages, a node cap, per-node alert
thresholds, PagerDuty.
