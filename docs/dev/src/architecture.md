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
       │  optional mDNS browse `_keystone._tcp.local.` then
       │  gRPC Session: PushFrame / Command / StreamChunk
       ▼
  keystone-server
       ├── HTTP UI + cookie session (axum), optional TOTP (`totp.rs`)
       │     optional rustls on `http_listen` (`tls.rs`)
       ├── ingest (tonic), optional rustls on `grpc_listen`
       ├── mDNS advertise of ingest (`mdns.rs`, UDP 5353; no token in TXT)
       ├── scrape (Prometheus HTTP, SNMP GET)
       ├── keystone-store (SQLite metadata + Redb series)
       └── widgets hydrate → JSON for app.js
```

## Crates

| Crate | Binary / role |
|---|---|
| `keystone-core` | Catalog, `DockerOp`, `SysOp`, `Permission`, configs, `NodeSettings` / `ServerSettings`, widget kinds and hydrate, `fleet_chips` / alert transitions, mDNS URL helpers, Docker Hub search/tag mapping (no I/O). |
| `keystone-proto` | Generated from `proto/ingest.proto`. |
| `keystone-store` | `keystone.sqlite` + `series.redb`. |
| `keystone-agent` | `keystone-agent`: sysinfo / hwmon / GPU, Docker handle, session client, optional mDNS browse. Extra bin `keystone-sys` (root helper, socket-activated, off until enabled). |
| `keystone-server` | `keystone`: UI, ingest, scrape, `/help`, mDNS advertise. |

Do not `sudo cargo`. Prefer `TMPDIR=.smoke/tmp` if `/tmp` is full. Smoke
data dir in examples is `.smoke`. `examples/server.toml` binds loopback
**18080/19100** so it does not collide with packaged **8080/9100** and does
not advertise mDNS.

## Control flow

1. Agent opens `Ingest.Session`, sends `PushFrame` (heartbeat + samples +
   token) on an interval. Packaged `ingest_url = "mdns"` browses
   `_keystone._tcp.local.` first (server advertises the gRPC port +
   `scheme=` TXT; never the ingest token). Explicit `http(s)://` skips
   browse. Rediscover on each reconnect.
2. Server allowlists samples, upserts the node row, stores series, diffs
   fleet-chip alerts (`apply_node_alerts`), ACKs. Webhook POSTs (if
   configured) are `tokio::spawn`’d and must not block the ACK.
3. On first good push for a node id, the server registers the stream in
   `AgentRegistry` and sends `set_runtime` (poll interval, labels, Docker
   flags, compose paths, `sys_enabled` / `sys_manage`) from `NodeSettings`.
4. UI Docker POSTs become `Command { op, payload_json }` on that stream.
   The agent runs `DockerOp` and returns `CommandResult`. Logs use
   `StreamChunk` then a result; the HTML logs page is an EventSource onto
   that stream. `cancel` aborts a follow when the browser disconnects.
   Image pull is still that path. Docker Hub search is a separate
   cookie-authed GET: the server talks to `hub.docker.com` over HTTPS and
   returns names/tags; it never pulls and never opens `docker.sock`.
   Host System POSTs are the same gRPC path with `SysOp`; the agent talks
   to `/run/keystone/sys.sock` only if `keystone-sys.socket` is enabled.
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
thresholds, PagerDuty, a CasaOS-style app shop, GHCR/private registry
browse, Docker Hub login, System reboot/shutdown from the UI, hostname /
timezone / users / SSH / firewall editors, Wi-Fi / VLAN / IPv6, Fedora /
Arch host updates, unattended-upgrades config, Watchtower.
