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
- `users` — username + Argon2id `password_hash`. Extra columns (all
  `ALTER TABLE` with defaults so existing DBs open):
  `must_change_password` (0/1): set when the admin is created from
  `KEYSTONE_ADMIN_PASSWORD` or the default `changeme`, cleared after a
  successful UI password change.
  TOTP: `totp_secret`, `totp_pending`, `totp_enabled`, `totp_backup_json`
  (JSON array of Argon2 hashes), `totp_last_step` (login, IPv4, VLAN, leftover
  unit-restart replay).
  `set_user_password` updates only hash + `must_change_password`.
- `sessions` — cookie id, username, expiry (purged on read), `pending_2fa`
  (0/1). `put_session(..., pending_2fa)`. Finished logins are two hours
  idle; `touch_session` slides `expires_unix` and refuses `pending_2fa`
  rows.
- `audit` — mutating Docker and System ops with username, node, op, target,
  ok, detail. Header **Audit** (`GET /audit`) lists the newest 200.
  Settings retention does not prune this table.
- `kv` — `k` / `v` text. Server operator settings are key `server`
  (`ServerSettings::KV_KEY`), JSON. Previous fleet-chip firing map is key
  `alerts_state` (`ALERTS_STATE_KV_KEY`), JSON object keyed
  `{node_id}::{chip}` so a restart does not re-POST the webhook.

Node settings and dashboard layouts are JSON on the node row, not kv, so
they travel with the node if you ever dump that table.

## Redb

Two tables:

- `latest` — per node, last sample set (for Overview “now”, Customize
  sensor discovery, and fleet chips on the home page).
- `series` — keys `{node_id}\\0{metric}\\0{labels_key}\\0{timestamp_ms}` →
  f64. `history()` range-scans for sparklines.

Retention is an atomic millisecond window on the `RedbSeries` handle.
Settings updates call `set_retention_hours` without reopening the file.
Writes drop points older than now − retention.

## Open

Packaged `data_dir` is `/var/lib/keystone`. Examples use `.smoke` so you
can run without root. Do not point two server processes at the same files.
