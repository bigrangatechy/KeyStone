<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# System

The **System** tab is the machine, not Docker: pending apt upgrades, apply
them, and IPv4 DHCP vs static. Cloudflare Tunnel and other containers stay
on **Compose** (use **Update** = pull then up).

This is **off until you enable it**, twice:

1. On the node **Settings** tab: **Observe host updates and addressing**,
   and **Allow apt upgrade, IPv4 changes, and GitLab backup** if you want Apply.
2. On the node, start the root helper socket (the metrics agent is **not**
   root):

   ```
   sudo systemctl enable --now keystone-sys.socket
   ```

   If the tab still says the helper is not running, restart the agent so it
   can use the new socket:

   ```
   sudo systemctl restart keystone-agent
   ```

Missing socket → the tab shows that `systemctl` line. Observe off → it
points at Settings (the socket unit alone does not turn the tab on). The
helper listens on `/run/keystone/sys.sock` (`root:keystone` mode `0660`).
It only runs allowlisted ops (`apt-get update` / `upgrade`, `apt list
--upgradable`, simulated `dist-upgrade`, netplan or
`nmcli`, Omnibus `gitlab-backup create`). There is no shell string.

## Updates

**Check for updates** runs `apt-get update` on **this** node, then lists
what `apt list --upgradable` and a simulated `apt-get dist-upgrade` agree
is pending (Debian / Ubuntu / Raspberry Pi OS). Held-back packages and
Ubuntu phased updates are included. The table is capped at 500 names.

**Apply updates** still runs `apt-get -y upgrade` (not `dist-upgrade`, not
autoremove) and streams the log. Leave the page to cancel follow. Apply
will not install new packages that only `dist-upgrade` would pull, and
Ubuntu may still skip a phased package. A reboot-needed flag is shown when
`/run/reboot-required` exists; this version does not reboot for you.

Packaged `keystone-server` and `keystone-agent` use `Restart=always` and
are enabled for boot (`WantedBy=multi-user.target`). After a kernel or
`apt upgrade` reboot they should come back on their own. Confirm with
`systemctl is-enabled keystone-server keystone-agent` (must print
`enabled`) — `systemctl start` alone does not survive a reboot. See
[Troubleshooting](troubleshooting.md).

## IPv4

Pick an Ethernet interface that is already up. **DHCP** or **static**
(address, prefix, gateway, optional DNS). Backend is Netplan when
`/etc/netplan` exists, otherwise NetworkManager.

Changing the address can drop the agent session (and SSH). Keep a console.
Wi-Fi, VLANs, and IPv6 are not in this version.

## GitLab backup

If this node has Omnibus GitLab (`/opt/gitlab/bin/gitlab-backup`), the
System tab shows **Backup GitLab**. That runs `gitlab-backup create` on
the machine (GitLab’s own dump, not a volume tar) and streams the log.
Copy `/etc/gitlab` (`gitlab.rb` and `gitlab-secrets.json`) next to the
archive yourself. Restore is not in this UI. Docker GitLab is not this
button.

Anyone who can sign in to the UI and who turned Manage on can change that
host. Same class of trust as Docker Manage. Mutations are written to
[Audit](audit.md). The ingest token cannot call these actions.
