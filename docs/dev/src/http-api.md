<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# HTTP API

Axum router in `crates/keystone-server/src/http.rs`. Cookie
`keystone_session` on almost everything. Static CSS/JS are
`include_str!`’d into the binary.

## Unauthenticated

| Method | Path | Notes |
|---|---|---|
| GET | `/health` | Plain `ok`. |
| GET | `/login` | Form. |
| POST | `/login` | Sets session cookie. |
| GET | `/static/app.css` | |
| GET | `/static/app.js` | Overview, Docker tabs, fleet home, alerts badge. |

## HTML (session required)

| Method | Path | Notes |
|---|---|---|
| GET | `/` | Node list. |
| GET/POST | `/nodes` | List / add node. |
| GET | `/nodes/new` | Add-node form. |
| GET | `/nodes/{id}` | Node page (Overview + Docker tabs + Settings). |
| GET | `/nodes/{id}/setup` | Agent TOML snippet. |
| POST | `/nodes/{id}/settings` | Save `NodeSettings`; `nudge_runtime` if connected. |
| GET | `/alerts` | Firing fleet chips (HTML). |
| GET/POST | `/settings` | `ServerSettings` + password change. |
| POST | `/settings/rotate-token` | Random ingest token (no-op if env override). |
| POST | `/nodes/{id}/docker/{op}` | `{op}` is `DockerOp::as_str()`. Form `payload` JSON, or `name` / `id` / `project`. Redirect keeps `?panel=`. Audit log. Streaming ops are 400. |
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
| GET | `/api/v1/catalog` | `{ "metrics": [ { name, metric_type, unit, help, labels } ] }` from `catalog()`. |
| GET | `/api/v1/alerts` | `{ "alerts": [ { node_id, hostname, chip, label, severity, display, hint } ] }`. Live firing chips (`warn`/`crit`). Header badge polls this at 2s. |
| GET | `/api/v1/nodes` | `{ "nodes": [ { node_id, hostname, os, status, last_seen, chips, alert_count } ] }`. `chips` are CPU/RAM/disk/temp (`id`, `label`, `display`, `tone`, optional `hint`). `alert_count` is how many chips are firing. Home page polls this at 1s. |
| GET | `/api/v1/nodes/{id}/dashboard` | `{ source, layout, widgets }` — hydrated for the grid. 404 if unknown node. |
| PUT | `/api/v1/nodes/{id}/dashboard` | JSON `Dashboard`; `validate()`; 204. |
| DELETE | `/api/v1/nodes/{id}/dashboard` | Clear custom layout; 204. |

`source` is `default` or `custom`. `app.js` PUTs the layout from Customize
and polls GET at `data-poll-secs`.

There is no generated OpenAPI dump. Keep this chapter in sync when you add
routes; do not hang utoipa on handlers just for docs.
