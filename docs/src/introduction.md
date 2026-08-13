<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# KeyStone

KeyStone is unlimited-node homelab monitoring with a per-node Docker control
plane. It is licensed under GPL-2.0-or-later. The software does not impose a
node cap.

You run **one** dashboard (the server) and an **agent** on every machine you
want in that dashboard. Agents **push** host metrics over a gRPC session, so
the boxes can sit behind NAT. The same session carries Docker commands when
you opt in on that node. The server can also **scrape** Prometheus exporters
and SNMP devices that do not run an agent.

Agents have no UI. Docker Engine is only ever opened on the node that runs
the agent — the server never talks to a remote `docker.sock`.

## Two packages

| Package | Role | Where it goes |
|---|---|---|
| `keystone-server` | HTTP UI + gRPC ingest | **One** machine |
| `keystone-agent` | Collectors + optional Docker | **Every** node you want listed |

They do not conflict. Install both on the UI host if that box should appear
as a node too.

## This book

These chapters are the **operator** documentation: install, add nodes,
Settings, dashboards, Docker, and security as you run it. The running server
serves the same text at `/help` after you log in, for this version of the
binary. `keystone docs` prints it on stdout.

Changing KeyStone itself (crates, catalog, widgets, ingest protocol) is
documented separately in `docs/dev/`. That material is not mixed into `/help`.
