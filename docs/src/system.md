<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# System

The **System** tab is host admin for **headless Ubuntu / Debian / Raspberry
Pi OS** boxes. Health is on the left (leftover services, failed units,
allowlisted journals, NTP, unattended-upgrades glance, addresses). Actions
are on the right (apt, autoremove, confirmed reboot, IPv4, GitLab Omnibus
backup). It is not a TrueNAS,
Proxmox, OMV, or Unraid control plane — those already have a GUI. Put an
agent on them for **Overview metrics** (and Docker Observe if they run
Engine). Leave **System manage** off.

Cloudflare Tunnel and other containers stay on **Compose** (use **Update**
= pull then up).

This is **off until you enable it**, twice:

1. On the node **Settings** tab: **Observe host updates and addressing**,
   and **Allow apt upgrade, autoremove, IPv4, leftover restart, GitLab backup, GitLab restore, and reboot** if
   you want Apply, Autoremove, leftover Restart, or Reboot. That Manage checkbox is behind a
   warning: signed-in admin plus the root helper can change this host.
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
It only runs allowlisted ops (`apt-get update` / `upgrade` / `autoremove`,
`apt list --upgradable`, simulated `dist-upgrade`, `needrestart -b`,
`systemctl --failed`, `timedatectl`, `journalctl -u` for five named
units, `systemctl reboot`, `systemctl restart` of a leftover or failed
listed name, netplan or
`nmcli`, Omnibus `gitlab-backup create` / `restore`). There is no shell string and no
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

**Autoremove** is a separate confirmed button. It runs `apt-get -y
autoremove` (not `dist-upgrade`) and streams the same way as Apply. Use it
after Apply when leftover packages sit around. It is Manage, not Observe.

After Apply, the System tab lists **services still using old libraries**
(`needrestart -b`) and **failed systemd units** (`systemctl --failed`).
With Manage on, each listed name has a **Restart** button (`systemctl
restart` for that name only). There is no unit-name textbox. The helper
refuses a name that is not on the live leftover or failed list. If 2FA is
on, Restart also asks for a **current 6-digit code** (not a backup code).
Restarting `keystone-server`, `docker`, or `ssh` asks extra confirmation;
on the UI host, `keystone-server` warns that the session will drop.

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

If `/usr/bin/unattended-upgrade` is on the node, the tab shows whether
unattended-upgrades is enabled (`APT::Periodic::Unattended-Upgrade` in
`/etc/apt/apt.conf.d/20auto-upgrades`, or `systemctl is-enabled
unattended-upgrades` when that file has no assignment) and the age of the
last run (stamp `/var/lib/apt/periodic/unattended-upgrades-stamp`, else
the log). That is a glance so you can see two updaters fighting. There is
no config editor and no enable toggle.

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
If you enabled an authenticator, Apply IPv4, leftover **Restart**, and
GitLab **Restore** also ask for a **current 6-digit code** (not a backup
code). Wi-Fi, VLANs, and IPv6 are not in this version.

## GitLab backup and restore

If this node has Omnibus GitLab (`/opt/gitlab/bin/gitlab-backup`), the
System tab shows **Backup GitLab**. That runs `gitlab-backup create` on
the machine (GitLab’s own dump, not a volume tar) and streams the log.
The tab shows the age of the newest `*_gitlab_backup.tar` in
`/var/opt/gitlab/backups` when one is on disk, and lists dumps already
there. **Restore** on a listed name runs `gitlab-ctl stop` puma and
sidekiq, then `gitlab-backup restore`, then `gitlab-ctl restart`. It is
not a path textbox. The helper re-checks the live directory. This
replaces GitLab application data. Copy `/etc/gitlab` (`gitlab.rb` and
`gitlab-secrets.json`) next to the archive yourself — they are not in
the tar. If 2FA is on, Restore needs a current authenticator code.
Leaving the follow page only stops the log; the restore keeps running.
Docker GitLab is not this button.

Anyone who can sign in to the UI and who turned Manage on can change that
host. Same class of trust as Docker Manage. Mutations are written to
[Audit](audit.md). The ingest token cannot call these actions.
