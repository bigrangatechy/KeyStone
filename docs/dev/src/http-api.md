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
| GET | `/static/app.js` | Overview + Docker tabs. |

## HTML (session required)

| Method | Path | Notes |
|---|---|---|
| GET | `/` | Node list. |
| GET/POST | `/nodes` | List / add node. |
| GET | `/nodes/new` | Add-node form. |
| GET | `/nodes/{id}` | Node page (Overview + Docker tabs + Settings). |
| GET | `/nodes/{id}/setup` | Agent TOML snippet. |
| POST | `/nodes/{id}/settings` | Save `NodeSettings`; `nudge_runtime` if connected. |
| GET/POST | `/settings` | `ServerSettings` + password change. |
| POST | `/settings/rotate-token` | Random ingest token (no-op if env override). |
| POST | `/nodes/{id}/docker/{op}` | `{op}` is `DockerOp::as_str()`. Form `payload` JSON. Audit log. |
| GET | `/nodes/{id}/containers/{cid}/logs` | SSE-ish payload of log text. |
| GET | `/nodes/{id}/containers/{cid}/stats` | Live stats. |
| GET | `/help`, `/help/{slug}` | Operator markdown compiled in (`help.rs`). |
| POST | `/logout` | |

## JSON (session required)

| Method | Path | Body / result |
|---|---|---|
| GET | `/api/v1/catalog` | `{ "metrics": [ { name, metric_type, unit, help, labels } ] }` from `catalog()`. |
| GET | `/api/v1/nodes/{id}/dashboard` | `{ source, layout, widgets }` — hydrated for the grid. 404 if unknown node. |
| PUT | `/api/v1/nodes/{id}/dashboard` | JSON `Dashboard`; `validate()`; 204. |
| DELETE | `/api/v1/nodes/{id}/dashboard` | Clear custom layout; 204. |

`source` is `default` or `custom`. `app.js` PUTs the layout from Customize
and polls GET at `data-poll-secs`.

There is no generated OpenAPI dump. Keep this chapter in sync when you add
routes; do not hang utoipa on handlers just for docs.
