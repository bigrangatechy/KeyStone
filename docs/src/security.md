<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Security

## What you are trusting

- The **server** holds admin sessions, the ingest token, metric history, and
  the audit log. Anyone who can write `data_dir` or log in as the admin owns
  the lab view and can send Docker commands to connected agents that allow
  them.
- Each **agent** runs as a system user. With Docker Observe enabled it can
  use the engine socket — that is root-equivalent on **that** host.
- The **ingest token** proves an agent is allowed to push. It does **not**
  grant UI login and cannot start, stop, or exec containers.

## UI account

This version is one local admin (Argon2id). There is no SSO and no extra
roles: the signed-in user can view every node, and can manage Docker on a
node to the extent that node’s Settings allow. Put the UI behind TLS (a
reverse proxy) if it is reachable from a network you do not trust. Do not
expose port 8080 to the internet.

## Ingest

Do not expose `grpc_listen` without a non-empty ingest token. Empty token
means any client can push (and enroll) — local smoke tests only.

Rotate the token from Settings when it leaks. Update every `agent.toml` (or
agent environment) and restart those agents; connected sessions will fail
until they present the new value.

`KEYSTONE_INGEST_TOKEN` on the server host overrides the stored token until
you unset it. Use that for deployment secrets if you do not want the token
in SQLite or TOML.

## Docker

Leave Observe off on nodes that should only report host metrics. Leave
Manage off unless you want the UI to change that engine. Leave Exec off
unless you need a shell in a container — it is a further gate on top of
Manage.

Mutating calls require a logged-in browser session. They are audit-logged.
The agent still refuses mutations and exec when the corresponding Settings
flags are false, even if the UI were buggy.

Do not point `docker.host` at another machine’s engine. KeyStone’s model is
local socket on the agent host.

## Metrics allowlist

Unknown metric names are dropped at ingest and at scrape. Exposition text
from a Prometheus job cannot inject arbitrary series names into the catalog.

## Retention and data

Series live in `data_dir` (Redb). Metadata, users, sessions, node settings,
and audit live in SQLite beside it. Retention (Settings) bounds how long
points are kept, not how long audit rows are kept.
