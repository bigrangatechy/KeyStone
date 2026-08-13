<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Threat model

## Docker socket

The agent talks to Docker Engine through `docker.host` (default
`/var/run/docker.sock`). Anyone who can use that socket can take over the
host. KeyStone therefore:

- Leaves Docker **off** until `docker.enabled = true`
- Leaves mutations off until `docker.manage = true`
- Leaves `exec` off until `docker.allow_exec = true`
- Rejects mutating RPCs when those flags are false
- Requires a logged-in UI session for Docker actions (the ingest token
  cannot call manage)
- Writes every mutation to the audit log

Do not expose the gRPC ingest port without a token. Do not expose the HTTP
UI without TLS at a reverse proxy if it is on a network you do not trust.

## Ingest token versus UI session

The ingest token authenticates agents only. UI users are a local admin
account (Argon2id). SSO is out of scope for this slice.

## Metrics allowlist

Unknown metric names are dropped. Scraped Prometheus text cannot inject
arbitrary series names into the catalog.
