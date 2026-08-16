<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Security

## What you are trusting

- The **server** holds admin sessions, the ingest token, metric history, and
  the audit log. Anyone who can write `data_dir` or log in as the admin owns
the lab view and can send Docker commands to connected agents that allow
them. The same session can apply apt upgrades and change IPv4 when System
Manage and the opt-in root helper are on.
- Each **agent** runs as a system user. With Docker Observe enabled it can
  use the engine socket — that is root-equivalent on **that** host.
- The **ingest token** proves an agent is allowed to push. It does **not**
  grant UI login and cannot start, stop, or exec containers, apply apt
  upgrades, or change IPv4.

## UI account

This version is one local admin (Argon2id). There is no SSO and no extra
roles: the signed-in user can view every node, and can manage Docker or
host System on a node to the extent that node’s Settings allow.

When the admin row is first created (empty `password_hash`), the password
is `KEYSTONE_ADMIN_PASSWORD` if set, otherwise **`changeme`**. The next
login must choose a different password (8+ characters) before the UI
unlocks. Putting a hash in `server.toml` yourself skips that prompt.

## Authenticator (2FA)

Optional TOTP (RFC 6238: SHA-1, 6 digits, 30 seconds). Not SMS. Enable it
from **Settings** with the current password, scan the QR (or type the
secret) in Aegis, Google Authenticator, or another TOTP app, then confirm
a code. Eight backup codes are shown **once** — store them offline.

Turn this on **before** the UI is reachable from the internet (reverse
proxy, port forward, or Cloudflare Tunnel). Password-only login is not
enough once the port is public. Existing installs keep working until you
enroll; a `.deb` upgrade does not force 2FA or rewrite `server.toml`.

Sign-in is password, then authenticator or a backup code. The second step
has five minutes. A used TOTP window cannot be reused; each backup code
works once. Eight failed password or TOTP tries for a username in 15
minutes blocks further tries for that name until the window passes.

KeyStone can terminate TLS itself (rustls) on the UI and, separately, on
gRPC ingest. A reverse proxy or Cloudflare Tunnel is still fine: leave
`[tls]` unset and terminate HTTPS in front. Do not expose port 8080 (or
9100) to the internet in plaintext.

When `[tls]` cert and key are set, the session cookie is marked `Secure`.
It is also `Secure` when the request carries `X-Forwarded-Proto: https`
(typical behind a tunnel or TLS proxy) so LAN HTTP still works without
in-tree certs. The cookie is `HttpOnly` and `SameSite=Lax`.

Lost phone: use a backup code, then enroll again from Settings. Lost both:
you need filesystem access to `data_dir` on the server host — there is no
email reset. See [Troubleshooting](troubleshooting.md). Root on that host
already owns the UI; 2FA does not change that.

## TLS

Optional. Empty `[tls]` (or omitted) is the packaged default: HTTP UI and
plaintext ingest, same as today. A `.deb` upgrade does not turn TLS on or
rewrite `server.toml`.

Put PEM files on the **server** host (`cert_file` = leaf + intermediates,
`key_file` = private key). The `keystone` user must be able to read them
(mode **640**, not world-readable). Restart `keystone-server` after
changing paths or files. There is no in-process reload.

```
[tls]
cert_file = "/etc/keystone/tls/fullchain.pem"
key_file = "/etc/keystone/tls/privkey.pem"
# ingest = true   # default: also wrap gRPC. Set false for UI-only HTTPS.
```

Let's Encrypt: point at `fullchain.pem` and `privkey.pem` (copy into
`/etc/keystone/tls` so the `keystone` user can read them; do not chown
`/etc/letsencrypt`). Self-signed for a LAN name:

```
sudo mkdir -p /etc/keystone/tls
sudo openssl req -x509 -newkey rsa:2048 -sha256 -days 825 -nodes \
  -keyout /etc/keystone/tls/privkey.pem \
  -out /etc/keystone/tls/fullchain.pem \
  -subj "/CN=keystone.home.arpa" \
  -addext "subjectAltName=DNS:keystone.home.arpa,DNS:localhost,IP:127.0.0.1"
sudo chown keystone:keystone /etc/keystone/tls/privkey.pem /etc/keystone/tls/fullchain.pem
sudo chmod 640 /etc/keystone/tls/privkey.pem
```

Copy `fullchain.pem` to each agent as `tls_ca_file` when the cert is
self-signed. The name in `ingest_url` must be one of the SAN names.

With both paths set:

- The UI on `http_listen` is **HTTPS** (same port, now TLS). Browsers use
  `https://<host>:8080` unless you moved `http_listen` to `:443`.
- **Ingest** (`grpc_listen`) is TLS unless `ingest = false`. Agents must
  use `https://` on `ingest_url`. The hostname in that URL must match the
  certificate (SNI). Let's Encrypt: omit `tls_ca_file`. Self-signed or
  private CA: copy the CA PEM to each agent as `tls_ca_file`.

`ingest = false` encrypts the dashboard only. Use that when agents stay on
a trusted LAN and the UI is the part you expose (or when a tunnel already
terminates HTTPS for the browser).

Cloudflare Tunnel: the tunnel can speak HTTP to origin (leave `[tls]`
off) or HTTPS to origin (in-tree UI TLS). Agents usually dial `grpc_listen`
on the LAN, not through the tunnel — turn ingest TLS on if that path
crosses a network you do not trust.

There is no skip-verify flag.

## Ingest

Do not expose `grpc_listen` without a non-empty ingest token. Empty token
means any client can push (and enroll) — local smoke tests only.

The UI advertises ingest on **mDNS** (`_keystone._tcp.local.`, UDP 5353).
TXT has only `scheme=http` or `scheme=https`. The **ingest token is never
in mDNS**. Anyone on the LAN can learn the gRPC port; they still need the
token to push. mDNS does not cross routers unless you run a reflector.
If multicast is blocked, set `ingest_url` to an explicit `http(s)://`.

Rotate the token from Settings when it leaks. Update every `agent.toml` (or
agent environment) and restart those agents; connected sessions will fail
until they present the new value.

`KEYSTONE_INGEST_TOKEN` on the server host overrides the stored token until
you unset it. Use that for deployment secrets if you do not want the token
in SQLite or TOML.

## Docker

Leave Observe off on nodes that should only report host metrics. Leave
Manage off unless you want the UI to change that engine. Leave Exec off
unless you need a shell in a container — it is a further gate on top of
Manage.

Mutating calls require a logged-in browser session. They are written to
header **Audit** (last 200 rows). Settings retention does not prune that
table. See [Audit](audit.md). The agent still refuses mutations and exec
when the corresponding Settings flags are false, even if the UI were buggy.

Do not point `docker.host` at another machine’s engine. KeyStone’s model is
local socket on the agent host.

The Images tab can search **Docker Hub** through the server (public Hub
HTTP API, cookie session). That lookup is not a Docker Engine call and
does not use the ingest token. Pull still runs on the agent. Hub
rate-limits unauthenticated search per IP — if it fails, type the image
name. There is no Hub login in this version.

## System (host apt and IPv4)

The metrics agent stays unprivileged (`NoNewPrivileges`, `ProtectSystem`).
Host apt and addressing go through an **opt-in** root helper
(`keystone-sys`) on `/run/keystone/sys.sock`. The package does not enable
that socket. Compromised `keystone` user **plus** an enabled socket **plus**
System Manage is host root for the allowlist (same class as Docker Manage).
Keep the allowlist tiny; do not start the helper on nodes that should only
report metrics.

Changing IPv4 can lock you out of SSH and drop the agent session. Keep a
console. Mutations are written to [Audit](audit.md). The ingest token
cannot call them.

## Metrics allowlist

Unknown metric names are dropped at ingest and at scrape. Exposition text
from a Prometheus job cannot inject arbitrary series names into the catalog.

## Alert webhook

The optional Settings URL receives the live chip value (`display`) and
host identity when an alert fires or clears. Anyone who can change that
field, or who can receive the POST, sees those numbers. Use HTTPS to a
service you control. The server does not retry failed deliveries.

## Retention and data

Series live in `data_dir` (Redb). Metadata, users, sessions, node settings,
and audit live in SQLite beside it. Retention (Settings) bounds how long
points are kept, not how long audit rows are kept.

## Package upgrades

A KeyStone `.deb` upgrade restarts the KeyStone systemd unit only. It does
not stop Docker, prune, or Compose-down. Maintainer scripts own
`/var/lib/keystone` (directory inode, not recursive) and refuse to follow a
symlink. Purge deletes KeyStone’s sqlite/redb or agent-buffer, never
`/var/lib/docker`. See [Install](install.md#upgrades).

