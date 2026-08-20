<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Troubleshooting

## `apt` Notice: `_apt` / Permission denied

The `.deb` is under a home directory (`~/Downloads`) that user `_apt`
cannot enter. Copy it to `/tmp` and install from there, or ignore the
Notice if `dpkg -l keystone-server` (or `keystone-agent`) already lists
the package.

## Cannot log in

- Username is `[auth] username` in `server.toml` (default `admin`), not the
  Unix account.
- Packaged first start is password `changeme` unless you set
  `KEYSTONE_ADMIN_PASSWORD` or `password_hash` before the first successful
  start. After that, the password is whatever you chose on the “Choose a
  password” page.
- After you set a bootstrap env password, remove the env var so a stale
  value is not surprising on the next restart.
- Authenticator enabled: the second page wants a current 6-digit code or a
  backup code (`XXXX-XXXX`).
- “too many attempts”: eight failed password or TOTP tries in 15 minutes
  for that username. Wait for the window to pass.
- Sent back to login after leaving the tab: closing the last KeyStone tab
  signs you out. A session also dies after two hours with no UI traffic
  (a tab left in the background counts). Sign in again.

## Stuck on “Choose a password”

The account was created with the bootstrap password (`changeme`, or
`KEYSTONE_ADMIN_PASSWORD` if you set it). Pick a new password of at least 8
characters that is not the bootstrap one. **Log out** is on that page if you
need to leave. After you save, the node list and welcome tour appear.

## Lost authenticator

Use a backup code at sign-in, then disable and enroll again from Settings
(password plus a remaining backup code, or a code from a new app after you
still have one working factor).

If you lost the app **and** the backup codes, you need root (or the
`keystone` data user) on the **server** host. 2FA lives in
`data_dir/keystone.sqlite`. Restoring a backup of `data_dir` restores the
account. There is no email reset.

On a packaged install, as root:

```
sqlite3 /var/lib/keystone/keystone.sqlite \
  "UPDATE users SET totp_enabled=0, totp_secret='', totp_pending='', totp_backup_json='[]', totp_last_step=0 WHERE username='admin';"
```

Use the username from `server.toml` if it is not `admin`. Sign in with the
password only, then turn 2FA back on.

## Agent stays “awaiting” or never “control connected”

1. `ingest_url` must be the **gRPC** listen address (`grpc_listen`), not
   `:8080` — or **`mdns`** on the same LAN. `journalctl -u keystone-agent`
   says `no KeyStone server found via mDNS` if multicast never reaches
   the UI (other VLAN, AP isolation, firewall dropping **UDP 5353**).
   Then set `ingest_url = "http://<ui-lan-ip>:9100"`.
2. `ingest_token` must match **Settings** exactly (and
   `KEYSTONE_INGEST_TOKEN` on the server if that is set). Packaged
   `change-me` is wrong after you generate a token in the UI.
3. `node_id` must be the id you enrolled, or omit it and use hostname.
4. Firewall: agents connect **out** to the server on **TCP 9100**. The
   server does not dial the agent. Allow **UDP 5353** both ways on the
   LAN if you use mDNS.
5. `journalctl -u keystone-agent` — TLS/HTTP mix-ups, connection refused,
   ingest nack, mDNS miss. **`using defaults (localhost ingest)`** is an
   old binary: it could not read `/etc/keystone/agent.toml` and dialed
   `127.0.0.1`. Current packages **exit** instead. Fix ownership:

   ```
   ls -ld /etc/keystone /etc/keystone/agent.toml
   sudo install -d -m 0750 -o root -g keystone /etc/keystone
   sudo chown root:keystone /etc/keystone/agent.toml
   sudo chmod 640 /etc/keystone/agent.toml
   sudo -u keystone cat /etc/keystone/agent.toml
   sudo systemctl restart keystone-agent
   ```

   `sudo -u keystone cat` must print the file.
6. `journalctl -u keystone-server` — `push rejected` if the token is wrong;
   `mDNS advertised` on a healthy start.

If the server has ingest TLS: `ingest_url` must be `https://`, and the host
must match the certificate. Let's Encrypt: no `tls_ca_file`. Self-signed:
set `tls_ca_file` to the CA PEM. `http://` against a TLS ingest port fails
(and the other way around). The UI scheme follows `[tls]` independently
(`ingest = false` leaves agents on HTTP).

An unknown agent with a good token still enrolls; a known id with a bad
token does not.

## Metrics missing

- CPU/memory/load should appear on any Linux agent. If Overview is empty,
  the agent is not pushing or `data_dir` is not writable.
- Disks skip some pseudo filesystems; unusual mounts may be absent.
- GPU and hwmon need kernel devices. VMs often have neither. Check **All
  samples**.
- Container series need Observe Docker on that node.
- Prometheus scrape: URL must be reachable **from the server**, interval at
  least 5s, and the names must be in the KeyStone allowlist (node_exporter
  `node_*` names that match the catalog are kept; unrelated names are
  dropped).

## Docker tab errors

- Observe is off.
- Agent not control-connected (commands ride the same session as metrics).
  Overview widgets can still move from stored samples after a reconnect;
  Docker tabs need the live command channel. **Docker: agent command timed
  out** is that channel, not `keystone-sys.socket` (the System tab helper).
  **agent dropped command** means the wait was cancelled because the session
  reset (often while sending several lists at once). Current servers write
  Commands on a side task so Results still complete; install
  `keystone-server` 0.1.0-14 or newer. Restarting the helper will not
  unstick Docker lists. Current agents run Docker/System RPCs off the ingest
  loop so a slow helper or `docker stats` cannot eat the 8s page budget,
  skip per-container stats while those lists run, and reply to page RPCs
  within 6s. After reconnect the server sends `set_runtime`; that Docker
  socket connect must not block reading lists. Use agent 0.1.0-11+ and
  server 0.1.0-14+. Series writes and retention prune run off the ingest
  select so CommandResults are not stuck behind `series.redb`.
- `keystone` user not in `docker` group — `Permission denied` on the
  socket. Log out/in is not enough for a systemd service: restart
  `keystone-agent` after `usermod -aG docker keystone`.
- Custom socket: set `docker.host` in `agent.toml` (for example
  `unix:///run/user/1000/docker.sock` for rootless) and restart the agent.
- Manage/Exec refused with a message that the flag is disabled: turn the
  checkbox on and save; a connected agent applies it without restart.
- Logs page stays empty: the agent must be **control connected**. Leave the
  page to cancel follow. Exec is not in the UI yet.
- Docker Hub search empty or an error: type `nginx:1.27` in Pull yourself.
  Hub rate-limits the **server** IP (not each browser). GHCR and private
  registries are not searched; paste the full name if the node can pull it.

## System tab errors

- Observe host is off (Settings). Enabling the socket unit is not enough.
- Helper not running: `sudo systemctl enable --now keystone-sys.socket` on
  **that** node. If the unit is already enabled, `sudo systemctl restart
  keystone-agent` so the sandboxed agent can use `/run/keystone/sys.sock`,
  then reload the tab. The metrics agent is not root.
- Agent not control-connected (same session as metrics).
- Manage refused: turn **Allow apt upgrade, autoremove, IPv4, GitLab backup, and reboot**
  on and save.
- `apt-get` failed: read the apply or autoremove stream; the helper only runs
  `upgrade` or `autoremove`,
  not `dist-upgrade`. Debian / Ubuntu / Raspberry Pi OS only. Check for
  updates lists `apt list --upgradable` plus a simulated dist-upgrade
  (held-back / phased included); Apply still will not install new deps.
  Ubuntu 24.04 will not auto-restart docker or ssh during Apply
  (`NEEDRESTART_MODE=list`); leftover services stay listed on the tab.
- Unattended-upgrades also running: the tab shows enabled / last run. It
  does not edit `/etc/apt/apt.conf.d/20auto-upgrades`. Two updaters is a
  glance, not a fix.
- Reboot dropped the UI: you rebooted the node that serves KeyStone. Wait
  for `keystone-server` to come back, then sign in again. Poweroff is not
  in this UI.
- Static IPv4 dropped the session: that address is now on the interface.
  Use a console if you cannot reach the new IP.
- Journal page 400: that unit is not on the allowlist (`keystone-agent`,
  `keystone-server`, `docker`, `ssh`, `gitlab-runsvdir`). There is no
  unit-name textbox.
- Clock not synchronized: `timedatectl` on the node. The System tab does
  not set the timezone.
- GitLab dump age missing: no `*_gitlab_backup.tar` under
  `/var/opt/gitlab/backups` yet. Restore stays SSH.

## Did not start after reboot

`systemctl start` is not enough. The unit must be **enabled**:

```
systemctl is-enabled keystone-server keystone-agent
sudo systemctl enable --now keystone-server keystone-agent
```

`is-enabled` should print `enabled`. Current packages use `Restart=always`
and disable the start-limit so a boot race with Docker or the network does
not leave the service `failed`. Then:

```
systemctl status keystone-server keystone-agent
journalctl -u keystone-server -u keystone-agent -b --no-pager
```

Do not Apply host updates from the UI until those units are enabled, or a
kernel upgrade reboot will take the UI down until you start it by hand.

## Compose Pull fails / Down emptied the tab

Pull needs that **project’s** compose file, not the first path on Settings.
Set **Compose files** to the YAML for each stack (readable by user
`keystone`). If containers are still there, Pull can refresh their images
without the file. Prefer **Stop** / **Restart** when you want the stack to
stay listed. **Down** is Docker `compose down`: containers go away, but
the project should stay on the tab so you can Up it. Up after Down still
needs the YAML path. `sudo -u keystone cat /path/to/compose.yaml` must
work.

## Token rotate left every node red

Expected until each agent’s config matches. Update `ingest_token` and
`systemctl restart keystone-agent` on every box. The UI password is
unrelated.

## Disk filling up

Lower **retention** on Settings (hours). Default 24h is the homelab-sized
window. History lives in `data_dir/series.redb`.

## Alert webhook never fires

- The URL must be non-empty and start with `http://` or `https://`.
- Only **transitions** are POSTed (new fire, warn↔crit, resolved). A
  restart does not re-send chips that were already firing.
- A 4xx/5xx or timeout is logged (`alert webhook`) and then dropped;
  ingest still succeeds. Check `journalctl -u keystone-server`.
- The chip must be warn or crit on the home page first — same 75% / 90%
  and 75°C / 90°C rules.

## Help in the UI looks wrong

`/help` is this operator book compiled into the server binary. It matches
the version you installed, not necessarily a newer GitLab Pages build.
