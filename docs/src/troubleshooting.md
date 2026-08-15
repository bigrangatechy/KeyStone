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
   ingest nack, mDNS miss.
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
- `keystone` user not in `docker` group — `Permission denied` on the
  socket. Log out/in is not enough for a systemd service: restart
  `keystone-agent` after `usermod -aG docker keystone`.
- Custom socket: set `docker.host` in `agent.toml` (for example
  `unix:///run/user/1000/docker.sock` for rootless) and restart the agent.
- Manage/Exec refused with a message that the flag is disabled: turn the
  checkbox on and save; a connected agent applies it without restart.
- Logs page stays empty: the agent must be **control connected**. Leave the
  page to cancel follow. Exec is not in the UI yet.

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
