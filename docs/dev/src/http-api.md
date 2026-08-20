<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# HTTP API

Axum router in `crates/keystone-server/src/http.rs`. Cookie `keystone_session` on almost everything. `HttpOnly`, `SameSite=Lax`.
`Secure` when in-tree UI TLS is on, or when `X-Forwarded-Proto: https`.
Finished logins are a session cookie (no `Max-Age`) so the browser drops
them on quit. They also expire after **two hours idle** (`SESSION_IDLE_SECS`);
`require_session` slides `expires_unix` about every ten minutes of traffic.
`GET /api/v1/session` is a cookie heartbeat so an open logs page counts as
traffic. Closing the last UI tab `sendBeacon`s `POST /logout`. `pending_2fa`
is still five minutes (`Max-Age=300`) and is not slid. Static CSS/JS are `include_str!`’d into the binary. A `pending_2fa` session
may only hit `/login/totp` and `/logout`. After a good code the pending row
is deleted and a new session id is issued.

## Unauthenticated

| Method | Path | Notes |
|---|---|---|
| GET | `/health` | Plain `ok`. |
| GET | `/login` | Form. |
| POST | `/login` | Cookie session. If TOTP is on, `pending_2fa` session (5 min) and redirect `/login/totp`. Else `/password` when `must_change_password`, else `/`. Eight fails / 15 min per username (`LoginGate`). |
| GET | `/static/app.css` | |
| GET | `/static/app.js` | Overview, Docker tabs, fleet home, alerts badge, welcome tour, widget drag-and-drop. |

## HTML (session required)

| Method | Path | Notes |
|---|---|---|
| GET | `/` | Node list. Banner if TOTP is off. Welcome tour runs once in the browser (`localStorage`). `?welcome=1` forces it (used after first-login password change). |
| GET/POST | `/password` | First-login password change when `must_change_password` is set. Blocks other authed routes (after 2FA if enrolled). |
| GET/POST | `/login/totp` | Second factor. Session required (`pending_2fa`). Authenticator or one-shot backup code. |
| GET/POST | `/nodes` | List / add node. |
| GET | `/nodes/new` | Add-node form. |
| GET | `/nodes/{id}` | Node page (Overview + Docker tabs + System + Settings). |
| GET | `/nodes/{id}/setup` | Agent TOML snippet. |
| POST | `/nodes/{id}/settings` | Save `NodeSettings`; `nudge_runtime` if connected. |
| GET | `/alerts` | Firing fleet chips (HTML). |
| GET | `/audit` | Mutation log (HTML). Newest first, last 200 rows from SQLite `audit`. Cookie session. Retention does not prune this table. The ingest token cannot write it. |
| GET/POST | `/settings` | `ServerSettings` + password change. `?err=totp-pw` (setup password), `totp` (disable), `totp-on`. |
| POST | `/settings/rotate-token` | Random ingest token (no-op if env override). |
| POST | `/settings/totp/start` | Password; writes `totp_pending`. Redirect `/settings/totp`. |
| GET | `/settings/totp` | QR + secret from `totp_pending`. |
| POST | `/settings/totp/confirm` | 6-digit code; enables TOTP, shows backup codes once. |
| POST | `/settings/totp/disable` | Password + TOTP or backup; clears TOTP columns. |
| POST | `/nodes/{id}/docker/{op}` | `{op}` is `DockerOp::as_str()`. Form `payload` JSON, or `name` / `id` / `project`. Redirect keeps `?panel=`. Audit log. Streaming ops are 400. |
| POST | `/nodes/{id}/sys/{op}` | `{op}` is `SysOp::as_str()`. Form JSON or `iface` / `method` / IPv4 fields. Audit log on mutate. Streaming ops redirect to their follow page (`updates_apply` → apply, `updates_autoremove` → autoremove, `gitlab_backup` → backup, `journal` → System tab). `reboot` is mutating and not streamed. |
| GET | `/nodes/{id}/sys/updates` | HTML follow page for `apt-get upgrade`. |
| GET | `/nodes/{id}/sys/updates/stream` | SSE for `updates_apply`. Cancel on drop. |
| GET | `/nodes/{id}/sys/autoremove` | HTML follow page for `apt-get autoremove`. Not dist-upgrade. |
| GET | `/nodes/{id}/sys/autoremove/stream` | SSE for `updates_autoremove`. Audit `started`. Cancel on drop. |
| GET | `/nodes/{id}/sys/gitlab-backup` | HTML follow page for Omnibus `gitlab-backup create`. |
| GET | `/nodes/{id}/sys/gitlab-backup/stream` | SSE for `gitlab_backup`. Audit `started`. Cancel on drop. |
| GET | `/nodes/{id}/sys/journal/{unit}` | HTML follow page. `{unit}` must be an allowlisted systemd unit (`journal_unit`). 400 otherwise. |
| GET | `/nodes/{id}/sys/journal/{unit}/stream` | SSE for `journal`. Observe; no audit. Cancel on drop. |
| GET | `/nodes/{id}/containers/{cid}/logs` | HTML follow page. |
| GET | `/nodes/{id}/containers/{cid}/logs/stream` | SSE: `{"t":"..."}` then `event: done`. Cancel on drop. |
| GET | `/nodes/{id}/containers/{cid}/stats` | One-shot JSON stats (not linked from the UI). |
| GET | `/nodes/{id}/compose/{project}/logs` | HTML follow page. |
| GET | `/nodes/{id}/compose/{project}/logs/stream` | SSE, same as container logs. |
| GET | `/help`, `/help/{slug}` | Operator markdown compiled in (`help.rs`). |
| POST | `/logout` | |

## JSON (session required)

| Method | Path | Body / result |
|---|---|---|
| GET | `/api/v1/session` | `{ ok: true }`. Cookie heartbeat. |
| GET | `/api/v1/catalog` | `{ "metrics": [ { name, metric_type, unit, help, labels } ] }` from `catalog()`. |
| GET | `/api/v1/alerts` | `{ "alerts": [ { node_id, hostname, chip, label, severity, display, hint } ] }`. Live firing chips (`warn`/`crit`). Header badge polls this at 2s. |
| GET | `/api/v1/nodes` | `{ "nodes": [ { node_id, hostname, os, status, last_seen, chips, alert_count } ] }`. `chips` are CPU/RAM/disk/temp (`id`, `label`, `display`, `tone`, optional `hint`). `alert_count` is how many chips are firing. Home page polls this at 1s. |
| GET | `/api/v1/dockerhub/search` | Query Docker Hub (server-side). Cookie session. |
| GET | `/api/v1/dockerhub/tags` | Hub tags for a repo. Cookie session. |
| GET | `/api/v1/nodes/{id}/sys/updates` | `{ packages: [{ name, from, to }], capped?: bool }` from `updates_list` (`apt-get update`, `apt list --upgradable`, simulated `dist-upgrade`). Cookie session. Cap 500. |
| GET | `/api/v1/nodes/{id}/container-usage` | `{ "<short-id>": { cpu_ratio?, memory_bytes? }, … }` from latest pushed samples. 404 if unknown node. Cookie session. Does not talk to Docker Engine. |
| GET | `/api/v1/nodes/{id}/dashboard` | `{ source, layout, widgets }` — hydrated for the grid. 404 if unknown node. |
| PUT | `/api/v1/nodes/{id}/dashboard` | JSON `Dashboard`; `normalize()` then `validate()`; 204. |
| DELETE | `/api/v1/nodes/{id}/dashboard` | Clear custom layout; 204. |

`source` is `default` or `custom`. Optional `layout.page` (`density`,
`cards`, `accent`, `empty`) and per-widget `style` / `title` are clamped
on read/save so unknown values do not drop a custom layout. `app.js` PUTs
the layout from Customize and polls GET at `data-poll-secs`. The
Containers tab polls `container-usage` at the same interval while that
panel is visible.

There is no generated OpenAPI dump. Keep this chapter in sync when you add
routes; do not hang utoipa on handlers just for docs.
