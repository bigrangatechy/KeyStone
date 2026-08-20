<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# System

The **System** tab is host admin for **headless Ubuntu / Debian / Raspberry
Pi OS** boxes (apt, leftover services, failed units, confirmed reboot,
allowlisted journals, NTP, IPv4, GitLab Omnibus backup). It is not a TrueNAS,
Proxmox, OMV, or Unraid control plane — those already have a GUI. Put an
agent on them for **Overview metrics** (and Docker Observe if they run
Engine). Leave **System manage** off.

Cloudflare Tunnel and other containers stay on **Compose** (use **Update**
= pull then up).

This is **off until you enable it**, twice:

1. On the node **Settings** tab: **Observe host updates and addressing**,
   and **Allow apt upgrade, IPv4, GitLab backup, and reboot** if you want Apply
   or Reboot.
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
--upgradable`, simulated `dist-upgrade`, `needrestart -b`,
`systemctl --failed`, `timedatectl`, `journalctl -u` for five named
units, `systemctl reboot`, netplan or
`nmcli`, Omnibus `gitlab-backup create`). There is no shell string and no
unit-name textbox.

## Updates

**Check for updates** runs `apt-get update` on **this** node, then lists
what `apt list --upgradable` and a simulated `apt-get dist-upgrade` agree
is pending (Debian / Ubuntu / Raspberry Pi OS). Fedora Server can **Observe**
the host; this version does not run `dnf`. Held-back packages and
Ubuntu phased updates are included. The table is capped at 500 names.

**Apply updates** still runs `apt-get -y upgrade` (not `dist-upgrade`, not
autoremove) and streams the log. Leave the page to cancel follow. Apply
will not install new packages that only `dist-upgrade` would pull, and
Ubuntu may still skip a phased package. On Ubuntu 24.04, Apply sets
`NEEDRESTART_MODE=list` so needrestart does **not** auto-restart docker or
ssh in the middle of the upgrade.

After Apply, the System tab lists **services still using old libraries**
(`needrestart -b`) and **failed systemd units** (`systemctl --failed`).
There is no “restart this unit” button — use a shell if you want a
targeted restart.

A reboot-needed flag is shown when `/run/reboot-required` exists or
needrestart reports a pending kernel. **Reboot node** is a confirmed
`systemctl reboot` (manage on). Poweroff is not in this UI. If this node
is the machine serving the KeyStone UI, the tab warns that the session
will drop until the server is back.

With the helper on, the tab also shows whether the clock is synchronized
(`timedatectl`) and follow links for `keystone-agent.service`,
`keystone-server.service`, `docker.service`, `ssh.service`, and
`gitlab-runsvdir.service`. Same idea as Compose logs: last 200 lines, live
follow, leave the page to stop. Not a PTY and not a unit-name textbox.

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
The tab shows the age of the newest `*_gitlab_backup.tar` in
`/var/opt/gitlab/backups` when one is on disk. Copy `/etc/gitlab`
(`gitlab.rb` and `gitlab-secrets.json`) next to the archive yourself.
Restore is not in this UI. Docker GitLab is not this
button.

Anyone who can sign in to the UI and who turned Manage on can change that
host. Same class of trust as Docker Manage. Mutations are written to
[Audit](audit.md). The ingest token cannot call these actions.
