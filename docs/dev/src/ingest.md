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

NAT-friendly: the agent dials `ingest_url`. `mdns` (or empty) browses
`_keystone._tcp.local.` for about 8s and uses the first usable LAN address
(prefers `192.168/16` over docker0). Plaintext is HTTP/2 h2c
(`http://host:9100`). With `[tls]` on the server and `ingest` true
(default once certs are set), the same port is TLS (`https://host:9100`).
mDNS then advertises `scheme=https`; SNI still needs a name that matches
the cert, so ingest TLS installs should set an explicit `https://` URL
(Add node does that). The agent sets tonic `ClientTlsConfig` (webpki
roots, or `tls_ca_file` for a private CA). Helpers:
`crates/keystone-server/src/tls.rs`. Server advertise:
`crates/keystone-server/src/mdns.rs`. Agent browse:
`crates/keystone-agent/src/mdns.rs`. URL picking (no sockets):
`crates/keystone-core/src/mdns.rs`.

Tests that follow the operator path (not just string sentinels):

- Add node snippet: `mdns` + Settings token + gRPC `:9100` fallback, never
  the UI port; ingest TLS fills `https://` instead of `mdns`
  (`http.rs` tests).
- Matching token enrolls (Add node then first push, or skip-the-form);
  wrong token does not; allowlist drops unknown series (`ingest.rs`).
- Loopback gRPC `Session` ACK + enroll (`ingest.rs`).
- Advertised TXT is `scheme` only; same-host advertise/browse
  (`mdns.rs` on server and agent).

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
