<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Settings

Two structs in `crates/keystone-core/src/settings.rs`. TOML is bootstrap
(and a fallback until the UI row exists). After first start, the UI is
source of truth for the fields below.

## `ServerSettings`

JSON in SQLite kv key `server`. Seeded by `ServerSettings::from_config` on
first open (`state.rs`).

| Field | Range / notes |
|---|---|
| `retention_hours` | 1–8760, default 24. Applied to `RedbSeries::set_retention_hours`. |
| `ingest_token` | Agents must match. Empty = accept any. |
| `prometheus_scrape` | `Vec<PrometheusScrape>` |
| `snmp_scrape` | `Vec<SnmpScrape>` |
| `alert_webhook_url` | Optional. Empty = off. `http://` or `https://` only (`parse_webhook_url`). POST JSON on alert **transitions** (new fire, severity change, resolved). Spawned; ingest does not wait. |

Pipe-separated textareas:

- Prometheus: `name | url | interval_secs | node_id` (interval default 30,
  min 5; node_id optional).
- SNMP: `name | target | community | interval_secs | node_id` (community
  default `public`).

Saving bumps `scrape_epoch` so scrape tasks restart.

`KEYSTONE_INGEST_TOKEN` overrides the stored token for ingest **and** the
setup snippet. The Settings input is read-only in that case. Rotate is
disabled while the env is set.

Listen addresses, `data_dir`, `auth.username`, and `[tls]` stay on
`ServerConfig` (`server.toml`). Password hash and TOTP live in the `users`
table after `ensure_admin`. TOTP is not a `ServerSettings` field. Helpers in
`crates/keystone-server/src/totp.rs`. TLS helpers in
`crates/keystone-server/src/tls.rs`. Empty cert/key is plaintext.

## `NodeSettings`

JSON on `nodes.settings_json`. Empty object = defaults (poll 1s, Docker
off).

| Field | Notes |
|---|---|
| `display_name`, `notes` | UI only. |
| `network_devices` | NIC allowlist for network widgets; empty = automatic. |
| `poll_secs` | 1–60, agent push + Overview poll. |
| `docker_enabled`, `docker_manage`, `docker_allow_exec` | Agent policy. |
| `sys_enabled`, `sys_manage` | Host system-admin observe / apply (opt-in `keystone-sys` socket). Default off. |
| `compose_paths` | Extra `-f` files. |
| `labels` | Heartbeat labels; replace TOML labels once connected. |

`agent_runtime()` is the payload for `set_runtime`. Add-node “runs Docker”
sets `docker_enabled` on the new row. `sys_manage` is sent only when
`sys_enabled` is also on.

## `AgentConfig` / `ServerConfig`

Serde TOML. Agent required at runtime: `ingest_url` (`http(s)://…` or
`mdns`), token, `node_id` or hostname, `buffer_dir`. `tls_ca_file` when
the resolved ingest URL is `https://` and the CA is not in web PKI. Docker
enable/manage/exec in TOML apply only until the first `set_runtime`.
Comments on the structs should describe that split; do not point at
generated markdown.

Examples: `examples/agent.toml`, `examples/server.toml`. Packaged copies:
`packaging/deb/*/`. Tests in `config/mod.rs` parse the examples.
