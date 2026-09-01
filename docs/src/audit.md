<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Audit

Header **Audit** is the trail of Docker and System mutations from this UI:
who was signed in, which node, the operation, the target (container id,
Compose project, interface, …), whether it succeeded, and a short detail.
Newest first. The page shows the last 200 rows.

The ingest token used by agents cannot write this table. Listing containers
or following logs is not a mutation and does not appear. Metric heartbeats
do not appear.

Settings **retention** bounds how long metric points are kept. It does not
prune audit rows.

## What is logged

- Docker Manage: start, stop, restart, pause/resume, kill, remove, prune
  stopped; Compose up / start / stop / restart / down / pull / Update;
  image pull / remove / prune; volume and network create / remove / prune.
- System Manage: apt apply, apt autoremove, IPv4/IPv6 DHCP vs static, VLAN create, leftover/failed unit restart, GitLab Omnibus backup,
  GitLab Omnibus restore, confirmed reboot. A refused IPv4, VLAN, unit-restart, or
  restore change (missing or bad authenticator code) is still a row (`ok` false).

Observe-only lists and live logs are not rows. There is no interactive
exec/PTY in this UI.

## Trust

Anyone who can sign in as the admin can read the whole log. Anyone who can
write `data_dir` can rewrite SQLite. This is an operator trail for the
homelab, not a tamper-proof archive.

See [Docker](docker.md), [System](system.md), and [Security](security.md).
