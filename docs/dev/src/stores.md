<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Stores

`Stores::open(data_dir, retention_hours)` creates:

| File | Engine | Contents |
|---|---|---|
| `keystone.sqlite` | rusqlite, WAL | nodes, users, sessions, audit, kv |
| `series.redb` | redb | latest samples + historical points |

## SQLite

- `nodes` — heartbeat identity, `online`, `last_seen_unix`. Extra columns
  added with `ALTER TABLE` if missing: `dashboard_json`, `settings_json`.
- `users` — username + Argon2id `password_hash`.
- `sessions` — cookie id, username, expiry (purged on read).
- `audit` — mutating Docker (and similar) with username, node, op, target,
  ok, detail.
- `kv` — `k` / `v` text. Server operator settings are key `server`
  (`ServerSettings::KV_KEY`), JSON.

Node settings and dashboard layouts are JSON on the node row, not kv, so
they travel with the node if you ever dump that table.

## Redb

Two tables:

- `latest` — per node, last sample set (for Overview “now” and Customize
  sensor discovery).
- `series` — keys `{node_id}\\0{metric}\\0{labels_key}\\0{timestamp_ms}` →
  f64. `history()` range-scans for sparklines.

Retention is an atomic millisecond window on the `RedbSeries` handle.
Settings updates call `set_retention_hours` without reopening the file.
Writes drop points older than now − retention.

## Open

Packaged `data_dir` is `/var/lib/keystone`. Examples use `.smoke` so you
can run without root. Do not point two server processes at the same files.
