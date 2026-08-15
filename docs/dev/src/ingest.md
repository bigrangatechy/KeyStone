<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Ingest protocol

Defined in `proto/ingest.proto`, package `keystone.v1`. One RPC:

```
service Ingest {
  rpc Session(stream AgentToServer) returns (stream ServerToAgent);
}
```

NAT-friendly: the agent dials `ingest_url` (HTTP/2 h2c in the current
binaries — `http://host:9100`). There is no TLS on this port in-tree; put
a proxy in front if you need it.

## Agent → server

`AgentToServer` oneof:

- `push` — `PushFrame`: `Heartbeat`, `repeated Sample`, `ingest_token`
- `result` — `CommandResult` for a prior `Command`
- `chunk` — `StreamChunk` (`request_id`, `data`, `eof`)

Heartbeat fields: `node_id`, `hostname`, `agent_version`, `os`, `kernel`,
`docker_version`, labels. Samples: `metric`, labels, `value`,
`timestamp_unix_ms`.

Token check: if Settings (or `KEYSTONE_INGEST_TOKEN`) is non-empty, the
frame token must match or the push is nacked. Empty server token accepts
any (dev only). Unknown metric names are dropped; the rest are written to
Redb and the node row is upserted. After a successful write the server
runs `apply_node_alerts` against the latest samples for that `node_id`
(same path on Prometheus/SNMP scrape). Webhook POSTs are spawned.

A new `node_id` on a good push is enrolled automatically.

## Server → agent

`ServerToAgent` oneof:

- `ack` — `ok` / `error` for a push
- `command` — `request_id`, `op`, `payload_json`

`op` is either a `DockerOp` string, `set_runtime` / `set_interval`, or
`cancel`. `cancel` payload is `{"request_id":"<id>"}` and aborts a
streaming logs task. `set_runtime` payload is `AgentRuntime` JSON
(`interval_secs`, `labels`, `docker_enabled`, `docker_manage`,
`docker_allow_exec`, `compose_paths`). Interval is clamped 1–60.

`AgentRegistry` maps `node_id` → command channel + pending oneshots +
in-flight log streams. Disconnect marks the node not-connected and fails
in-flight calls. Chunks are forwarded by `request_id` until `eof`.

## Buffer

If the server is unreachable, the agent writes to `buffer_dir` and replays
when the session returns. Buffer is host metrics, not a substitute for
Docker control (commands need a live stream).
