<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Changelog

Newest first. Each heading is `HH:MM:SS DD/MM/YYYY AEST` (Australian Eastern
Standard Time). What this binary does vs later work is in [Features](features.md).

## 00:31:52 10/09/2026 AEST

Ingest can send stdin (or a TTY size) to an in-flight agent stream. Logs
still ignore those bytes. Interactive exec is not in the UI.

## 00:09:40 10/09/2026 AEST

Show Volumes and Networks as glance cards. Click loads summarized inspect
(no Labels). Create and prune unused stay on the toolbar; Remove is on the
card.

## 23:00:52 09/09/2026 AEST

Show Engine disk use on Images and prune unused build cache. Tests hide
System mutations until the helper is on, and `cargo test` cannot run apt
apply or GitLab backup. This changelog and the Features chapter are in Help.

## 22:13:29 09/09/2026 AEST

Enable KeyStone on boot from Settings so a reboot does not leave the UI down.

## 20:56:21 09/09/2026 AEST

Serve a walkthrough user guide from Help so operators follow this binary.

## 20:37:38 09/09/2026 AEST

Show local images as glance cards so a click opens inspect without Env.

## 01:09:10 02/09/2026 AEST

Show Compose projects as glance cards so a click opens services, logs, and actions.

## 23:45:59 01/09/2026 AEST

Log in to Docker Hub or GHCR on the node so Pull can use stored credentials.

## 23:32:00 01/09/2026 AEST

Allow or refuse SSH password logins from a yes/no toggle on the System tab.

## 22:44:10 01/09/2026 AEST

Show Docker Hub search as glance cards that fill Pull.

## 22:34:04 01/09/2026 AEST

Join listed Wi-Fi from a scan on the System tab.

## 21:43:30 01/09/2026 AEST

Create 802.1Q VLANs from a listed Ethernet parent and numeric id.

## 21:27:31 01/09/2026 AEST

Set IPv6 automatic or static on the same Ethernet apply as IPv4.

## 21:00:14 01/09/2026 AEST

Restore GitLab Omnibus from listed dumps on the System tab.

## 20:07:57 01/09/2026 AEST

Restart leftover and failed units from listed names on the System tab.

## 19:41:23 01/09/2026 AEST

Require a current authenticator code to change IPv4 when TOTP is on.

## 18:41:18 01/09/2026 AEST

Keep the UI session across tab switch; pagehide is not a real logout.

## 18:40:21 01/09/2026 AEST

Point KeyStone at the shared community guidelines so CoC and templates stay in one place.

## 23:32:50 20/08/2026 AEST

Package server 0.1.0-17 and agent 0.1.0-14.

## 23:15:58 20/08/2026 AEST

Show containers as clickable cards and keep System health apart from host mutations.

## 22:42:02 20/08/2026 AEST

Add confirmed apt autoremove and show whether unattended-upgrades is also running.

## 21:22:31 20/08/2026 AEST

Show NTP, allowlisted journals, and GitLab dump age on the System tab.

## 19:36:58 20/08/2026 AEST

Package server 0.1.0-16 and agent 0.1.0-13.

## 19:18:13 20/08/2026 AEST

Stop Ubuntu from bouncing docker during Apply, and allow a confirmed reboot now that sessions idle out.

## 18:04:26 20/08/2026 AEST

Keep KeyStone up after reboot and make Compose and apt updates usable.

## 22:55:37 17/08/2026 AEST

Say a wrong password on 2FA setup is not a failed code.

## 22:03:07 16/08/2026 AEST

Package server 0.1.0-15 and agent 0.1.0-12.

## 21:57:42 16/08/2026 AEST

Show the KeyStone logo in the header and on login.

## 21:56:09 16/08/2026 AEST

Let Customize rename Overview cards and hide empty ones.

## 21:36:55 16/08/2026 AEST

Let Overview Customize pick page chrome and per-card drawing styles.

## 21:24:52 16/08/2026 AEST

Keep Audit to mutations and add GitLab Omnibus backup.

## 21:03:28 16/08/2026 AEST

Show Docker and System mutations on an Audit page.

## 20:45:09 16/08/2026 AEST

Keep ingest reading Docker and System replies while series persist.

## 20:25:09 16/08/2026 AEST

Keep Docker and System tabs answering inside the 8s page wait.

## 19:59:43 16/08/2026 AEST

Keep in-flight Docker and System waits across an ingest reconnect.

## 19:49:42 16/08/2026 AEST

Keep node-page RPCs readable while set_runtime connects Docker.

## 19:39:46 16/08/2026 AEST

Keep pipelined Docker lists from resetting the ingest session.

## 19:26:48 16/08/2026 AEST

Keep Docker lists answering while stats or the sys helper run.

## 19:12:56 16/08/2026 AEST

Keep Containers CPU and memory updating from pushed samples.

## 19:05:29 16/08/2026 AEST

Keep Docker control after reconnect and show container CPU and memory.

## 20:04:31 15/08/2026 AEST

Let the sandboxed agent use keystone-sys after the socket is enabled.

## 19:09:59 15/08/2026 AEST

Keep cargo smoke off packaged 8080/9100 and fail tests that would miss it.

## 18:51:29 15/08/2026 AEST

Let operators apply apt upgrades and set IPv4 from a System tab.

## 18:01:37 15/08/2026 AEST

Let the Images tab search Docker Hub to fill pull.

## 17:37:55 15/08/2026 AEST

Fail if agent.toml is unreadable instead of dialing localhost.

## 16:24:47 15/08/2026 AEST

Let agents find the UI on the LAN via mDNS, and default packaged first login to admin/changeme.

## 14:59:13 15/08/2026 AEST

Add optional authenticator 2FA and in-tree TLS for the UI and agent ingest.

## 14:07:23 15/08/2026 AEST

Prompt for a new password on first login, add a welcome tour, and let Overview cards be dragged into place.

## 13:51:06 15/08/2026 AEST

Alert on fleet-chip warn/crit with a page, row mark, and optional webhook.

## 13:35:30 15/08/2026 AEST

Show live CPU, RAM, disk, and temperature chips on the node list.

## 13:15:20 15/08/2026 AEST

Keep package upgrades from touching Docker or the host OS.

## 13:15:06 15/08/2026 AEST

Add live Docker logs and Portainer-shaped control tables.

## 12:42:18 15/08/2026 AEST

State KeyStone's goal as a homelab replacement for Portainer and Netdata.

## 00:53:34 14/08/2026 AEST

Replace generated docs with dedicated operator and developer books.

## 00:26:36 14/08/2026 AEST

Move operator settings into the UI and add per-sensor temperature widgets.

## 23:46:22 13/08/2026 AEST

Add a customisable node dashboard with 1s polling, GPU metrics, and hardware temperatures.

## 22:56:17 13/08/2026 AEST

Add a central node dashboard with enroll, widgets, and network metrics.

## 22:17:23 13/08/2026 AEST

Add amd64 and arm64 .deb packages for Debian and Raspberry Pi 4/5.

## 22:04:37 13/08/2026 AEST

Bootstrap GPLv2+ KeyStone with living docs, hybrid ingest, and per-node Docker.

## 09:25:23 06/07/2026 AEST

Initial commit.
