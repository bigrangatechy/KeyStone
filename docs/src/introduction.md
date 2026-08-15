<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# KeyStone

KeyStone is a **homelab replacement for Portainer and Netdata**: one UI for
live host metrics *and* Docker Engine on every machine. It is licensed
under GPL-2.0-or-later. The software does not impose a node cap.

You run **one** dashboard (the server) and an **agent** on every box you
want in that dashboard. Agents **push** host metrics over a gRPC session, so
the machines can sit behind NAT. The same session carries Docker commands
when you opt in on that node. The server can also **scrape** Prometheus
exporters and SNMP devices that do not run an agent.

Agents have no UI. Docker Engine is only ever opened on the node that runs
the agent — the server never talks to a remote `docker.sock`.

## Instead of two stacks

| You used to run | KeyStone |
|---|---|
| **Netdata** (or similar) on each host for CPU, RAM, disks, NICs, GPU, temperatures | Per-node Overview: customisable widgets, default **1s** poll, history kept on the server (default 24h) |
| **Portainer** (or similar) against Docker Engine for containers, Compose, images, volumes, networks, logs | The same node page: Observe / Manage / Exec, local socket via the agent |

One login, one agent per machine, as many nodes as you have. You do not
expose `docker.sock` over the network, and you do not run a metrics UI on
every Pi.

This version is the homelab slice: Linux agents (`amd64` / `arm64`), a
local admin account with optional authenticator 2FA, catalog metrics, and
per-node Docker. It is not Kubernetes, not a SaaS cloud, and not every
collector Netdata ships.

## Two packages

| Package | Role | Where it goes |
|---|---|---|
| `keystone-server` | HTTP UI + gRPC ingest | **One** machine |
| `keystone-agent` | Collectors + optional Docker | **Every** node you want listed |

They do not conflict. Install both on the UI host if that box should appear
as a node too.

## This book

These chapters are the **operator** documentation: install, add nodes,
Settings, dashboards, alerts, Docker, and security as you run it. The running server
serves the same text at `/help` after you log in, for this version of the
binary. `keystone docs` prints it on stdout.

Changing KeyStone itself (crates, catalog, widgets, ingest protocol) is
documented separately in `docs/dev/`. That material is not mixed into `/help`.
