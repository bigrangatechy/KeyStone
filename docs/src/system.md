<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# System

The **System** tab is the machine, not Docker: pending apt upgrades, apply
them, and IPv4 DHCP vs static. Cloudflare Tunnel and other containers stay
on **Compose** (use **Update** = pull then up).

This is **off until you enable it**, twice:

1. On the node, start the root helper socket (the metrics agent is **not**
   root):

   ```
   sudo systemctl enable --now keystone-sys.socket
   ```

2. On the node **Settings** tab: **Observe host updates and addressing**,
   and **Allow apt upgrade and IPv4 changes** if you want Apply.

Missing socket → the tab shows that `systemctl` line. The helper listens
on `/run/keystone/sys.sock` (`root:keystone` mode `0660`). It only runs
allowlisted ops (`apt-get update` / `upgrade`, netplan or `nmcli`). There
is no shell string.

## Updates

**Check for updates** runs `apt-get update` and a simulated upgrade on
**this** node (Debian / Ubuntu / Raspberry Pi OS). **Apply updates** runs
`apt-get -y upgrade` (not `dist-upgrade`, not autoremove) and streams the
log. Leave the page to cancel follow. A reboot-needed flag is shown when
`/run/reboot-required` exists; this version does not reboot for you.

## IPv4

Pick an Ethernet interface that is already up. **DHCP** or **static**
(address, prefix, gateway, optional DNS). Backend is Netplan when
`/etc/netplan` exists, otherwise NetworkManager.

Changing the address can drop the agent session (and SSH). Keep a console.
Wi-Fi, VLANs, and IPv6 are not in this version.

Anyone who can sign in to the UI and who turned Manage on can change that
host. Same class of trust as Docker Manage. Mutations are audit-logged.
The ingest token cannot call these actions.
