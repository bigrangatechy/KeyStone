<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Features

What this binary does, what we still want, and what we will not add.
Walkthrough: [User guide](using.md). Dated history: [Changelog](changelog.md).

## Current

One UI for live host metrics and Docker Engine on every Linux box you enroll.
Unlimited nodes. Agents **push** over gRPC; the server never opens a remote
`docker.sock`. Two packages: `keystone-server` and `keystone-agent`.

- **Fleet** — home page chips (CPU, RAM, disk, temperature), [Alerts](alerts.md),
  optional webhook, [Audit](audit.md) of Docker and System mutations.
- **Overview** — customisable widgets, default 1s poll, history on the server
  (default 24h). GPU and hwmon when the kernel has them.
- **Docker** — Containers and Compose as cards; local Images as cards (inspect
  drops `Env`); Hub search cards that fill Pull; Hub/GHCR login on that node;
  Engine disk use (images, containers, volumes, build cache) and prune unused
  build cache; Volumes and Networks as cards (inspect drops Labels); live logs.
  Manage is opt-in. Exec is not in the UI. See [Docker](docker.md).
- **System** — Ubuntu / Debian / Raspberry Pi OS when the root helper is on
  (`keystone-sys.socket` plus Settings). Health vs actions: apt, autoremove,
  leftover/failed unit restart, journals (including
  `unattended-upgrades.service`), NTP, timezone dropdown, unattended-upgrades glance,
  GitLab Omnibus backup/restore, IPv4/IPv6, VLAN create, Wi-Fi join, SSH
  password yes/no, Start KeyStone on boot, confirmed reboot. A laptop without
  the helper still shows addresses and NTP; mutations stay hidden. Appliance
  OSes (Proxmox, TrueNAS, OMV, Unraid) stay Observe. See [System](system.md).
- **Sign-in** — one local admin, first-login password change, optional
  authenticator 2FA. Listed System mutations ask for a current 6-digit code
  when TOTP is on. Sessions idle after two hours. Optional in-tree TLS.
- **Help** — this book for **this** binary (header Help). `amd64` and
  `arm64` `.deb`s. Packaged listen **8080** / **9100**; smoke examples
  **18080** / **19100**.

## Later

One slice at a time. Not mixed into the current tree:

- unattended-upgrades enable/disable; Fedora `dnf`.
- Interactive `docker exec` in the UI. Bidirectional `StreamChunk` on ingest
  is in; the Settings exec checkbox stays reserved.
- LAN-only LLM Agent API (Unix socket, off by default, never on the internet).

## Not this product

These stay out on purpose:

- A node cap, remote `docker.sock`, `sh -c`, a free PTY, a unit-name textbox.
- Poweroff / shutdown from the UI (reboot is the power action).
- VLAN QinQ or VLAN delete; hidden SSID, hotspot, or 802.1X.
- Hostname, users, firewall, `PermitRootLogin`, or a timezone textbox.
- Harbor, public GHCR browse, Watchtower, a CasaOS-style app shop.
- Enabling `keystone-sys.socket` from the UI; rewriting unit files.
- Exposing the LLM Agent API to the internet.
- Kubernetes, 32-bit ARM packages, SSO, required 2FA, WebAuthn, PagerDuty.
