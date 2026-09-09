<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# LLM Agent API

**Plan.** Not in this binary. Do not implement it in the same slice as
other work.

This is a **dedicated control API for self-hosted AI agents** (a local
Ollama, Open WebUI, or similar on the LAN). It is not the KeyStone
`keystone-agent` daemon, not the ingest gRPC session, and not the header
Help chat. Those stay separate.

Operator summary lives under Features **Later**. This page is the
contract so a later slice does not invent a second trust model.

## Purpose

Let a homelab LLM drive KeyStone programmatically instead of clicking the
web UI. The LLM host may have its own outbound internet for research.
That traffic never enters KeyStone’s trust boundary. KeyStone only
accepts **local** control connections and does not care what the model
fetches.

## Hard rules

- The Agent API is **never exposed to the internet**. Not via the UI
  listen address, not via ingest `:9100`, not via a “just for LAN”
  TCP bind on `0.0.0.0`.
- First shipping slice is a **Unix socket on the node**, modelled on
  `keystone-sys.socket`: off by default, socket-activated, **not**
  enabled by the package, **not** enabled from the UI.
- Separated from the agent’s main gRPC session. A wedged ingest stream
  must not starve this socket, and a looping LLM must not starve
  ingest, lists, or the UI.
- The ingest token is **not** an Agent API key and cannot call it.
- No fresh-code TOTP on this path. Agents cannot tap an authenticator.
  Ops that need step-up in the UI are **denied** here. Damaging ops are
  denied at the socket, full stop.
- No `sh -c`, no free PTY, no unit-name textbox, no remote `docker.sock`.
  Same keep-outs as the product. `container_exec` stays out until the
  UI exec slice exists, and even then this API does not get it by
  default (root-equivalent).
- Tests must not talk to Ollama, must not bind a public port, and must
  not spawn live apt / reboot / netplan / docker login.

## Surfaces

| Surface | Who talks | Power | First slice |
|---|---|---|---|
| Unix socket on the **node** (`keystone-llm.socket` → `/run/keystone/llm.sock`) | LLM on that same box | That node only, gated by key scope | **Yes** |
| Server-relayed LAN path | LLM on another LAN host; it dials the **server**; server forwards down the existing push-only gRPC session | Still only what that node’s socket would allow | Deferred |
| UI listen (`8080` / smoke `18080`) | Browser | Cookie session, 2FA step-up | Never this API |
| Ingest (`9100` / smoke `19100`) | `keystone-agent` | Metrics + `Command` | Never this API |

The server still never dials out to nodes. A remote LLM on the LAN, if
we add it later, must **dial in**. The on-node Unix socket remains the
only place with node-level power; a relay is a front door, not a second
enforcer.

A TCP listener on the node (even RFC1918) is how this gets
port-forwarded. Do not add one in the first slice.

## Packaging (when it ships)

Mirror the sys helper:

- Extra units on the **agent** `.deb`: `keystone-llm.socket` +
  `.service`. `ListenStream=/run/keystone/llm.sock`. `SocketUser` /
  `SocketGroup` so the packaged `keystone` user can accept; the LLM
  process needs group access (document `usermod -aG`, never a world-open
  socket).
- `enable = false`. The package does not start it. The operator enables
  the socket, same as `keystone-sys.socket`.
- Not a UI checkbox that runs `systemctl enable`.
- Do not `Requires=` Docker. Observe/Manage flags still come from
  `NodeSettings` via `set_runtime`.

The helper binary can be `keystone-agent llm-api` (subcommand) or a
small extra binary. Prefer a subcommand so we do not ship a third `.deb`.

## Authentication

Per-key API secrets, issued and revoked in **server** Settings. Store
only a hash (same class of care as the admin password; never write the
secret to Audit). Push `{ key_id, hash, scope }` to the node on
`set_runtime`. The Unix socket checks the hash locally so a wedged
ingest session cannot be the only path to revoke… **Revoke must still
reach the node.** Until the next `set_runtime`, a stolen key on that
socket still works — so reconnect/`nudge_runtime` after revoke, and
document it.

Each key has a **scope**. Preference is the operator’s, not a single
hardcoded role:

| Scope | May | Must not |
|---|---|---|
| `observe` | Lists, inspect, summarized df, journals, updates list, wifi scan, status | Any `mutating()` op |
| `reversible` | Observe plus start/stop/restart/pause/resume, Compose up/start/stop/restart (not Down), image pull | Remove, prune, kill, Down, login, System mutate, reboot |
| (none higher) | — | Everything the UI treats as step-up or damaging |

Denied at the socket (any scope), because there is no authenticator:

- Every `SysOp::needs_step_up()` (`net_set`, `vlan_add`, `wifi_join`,
  `ssh_password`, `unit_restart`, `unit_enable`, `gitlab_restore`)
- `reboot`, `updates_apply`, `updates_autoremove`
- `volume_remove` / `volume_prune`, `network_remove` / `network_prune`,
  `image_remove` / `image_prune`, `build_cache_prune`, `container_prune`,
  `container_remove`, `container_kill`, `compose_down`
- `image_login`, `container_exec`
- GitLab restore (already step-up). GitLab backup is mutating but not
  step-up; still **deny** until a later slice argues it is reversible.

`gitlab_backup` and Compose **Down** look tempting. Keep them denied
until a human has used the UI path. Dry-run can still *describe* them.

Key material is not the ingest token and not the admin cookie.

## Audit, rate limit, dry-run

- Every accepted or denied call writes Audit: actor is `llm:<key_id>`
  (never the secret), plus node, op, target, ok/error.
- Rate limit from day one, per key, on the socket. A looping agent must
  not thrash the fleet or fill Audit. Tests cover the limiter with
  fixtures, not a live model.
- Optional later: `plan: true` returns the op, target, and scope check
  without running it. Useful, not a substitute for deny-lists.

## Handshake (mixed fleets)

Hard requirement: mixed-version fleets must not break.

- First message is a hello, not an op. The server/node replies with
  **capability flags** (ops this socket will run), not a “are you v2.1”
  check.
- Unknown ops return a structured unsupported error. Never crash, never
  close the socket.
- Older nodes without the unit: the Settings page says the socket is
  off. The UI chat (below) must not assume the socket exists.
- Newer Settings + older agent: `set_runtime` extra JSON fields are
  already ignored (`old_node_json_ignores_unknown_keys`). Keep it that
  way.

Capability flags are the same strings as `DockerOp::as_str()` /
`SysOp::as_str()`. Do not invent a parallel op catalog.

## Conversational UI (related, separate)

A chat box in the **server UI** is not this API. It uses the existing
cookie session, idle timeout, Audit, and TOTP step-up. No new auth
surface. The model may *propose* a destructive op; a human confirms
with the same confirm (+ current 6-digit code when TOTP is on) that the
forms already use.

Do not fork UI mutations onto the Unix socket “for convenience.”

## Proposed slices (when we build it)

One at a time. Not mixed with exec UI.

1. **Observe Unix socket** — unit + hello + `observe` keys in Settings +
   Audit + rate limit. Lists and inspect only. No TCP.
2. **Reversible Docker** — start/stop/restart/pause/resume, Compose
   start/stop/restart/up, image pull. Still no remove/prune/Down.
3. **Dry-run / plan** — `plan: true` for allowed ops.
4. **Server-relayed LAN LLM** — LLM dials the server on a **new**
   loopback-or-RFC1918-only bind (not 8080, not 9100). Server forwards
   onto the node’s socket semantics. Refuse binds that are not
   loopback / RFC1918 / ULA. Still never `0.0.0.0`.

Stay out of those slices: internet exposure, SSO, required 2FA,
WebAuthn, a CasaOS-style app shop, Harbor, Watchtower, node cap.

## Why not the ingest session?

Ingest is NAT-friendly push from `keystone-agent`. Mixing LLM traffic
onto it would share queues with `container_list` and metrics. A wedged
model would look like **agent command timed out**. The stdin
`StreamChunk` path is for a later **docker exec** UI, not for this API.
