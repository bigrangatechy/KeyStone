<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Headless Linux OS support

**Plan.** One source tree, several package formats. Do not fork the
application per distro.

This binary already **detects** the host from `/etc/os-release` (then
`/usr/lib/os-release`). Fedora Server and openSUSE Server can run the same
agent, server, and `keystone-sys.socket` once we **build** `.rpm` as well as
`.deb`. System **Apply** is still apt. Fedora `dnf` and openSUSE `zypper`
apply are later slices, not this one.

Operator summary: Features **Later**. System Manage for appliances
(Proxmox, TrueNAS, OMV, Unraid) stays Observe even when `os-release` looks
like Debian.

## Hard rules

- **One tree.** Distro differences belong in `HostOs` (family + package
  kind), packaging metadata, and later package-manager adapters. This is
  not a per-distro application fork and not a `keystone-fedora` crate.
- Tests parse **fixture strings**. They must not require a live
  `/etc/os-release`, and they must not run live `dnf`, `zypper`, or `apt-get`.
- Same keep-outs as the product: no `sh -c`, no free host PTY, no unit-name
  textbox, no remote `docker.sock`, no enabling `keystone-sys.socket` from
  the package or the UI.
- Socket path stays `/run/keystone/sys.sock`. Unit names stay
  `keystone-server.service`, `keystone-agent.service`, `keystone-sys.socket`.
- `adduser` is Debian postinst. RPM uses `useradd` / `groupadd`. Same
  `keystone` user, docker group optional, never root for the metrics agent.

## What this slice does

`keystone-core` `os` parses `ID`, `ID_LIKE`, `VERSION_ID`, `PRETTY_NAME`.

| Family | `ID` / `ID_LIKE` examples | Package kind | Apply in this binary |
|---|---|---|---|
| Debian | ubuntu, debian, raspbian, linuxmint | apt | yes |
| Fedora | fedora, rhel, centos, rocky, almalinux | dnf | refuse with “apt on this version” |
| Suse | opensuse-leap, opensuse-tumbleweed, sles | zypper | refuse with “apt on this version” |
| Other | anything else | unknown | refuse |

`status` (local, no helper required) includes
`os: { id, version_id, pretty, family, package }`. The System tab shows
`pretty` and hides apt Check/Apply/Autoremove when `package` is not `apt`.

Packaging on Debian is unchanged except: after `#DEBHELPER#`, if the unit
**is-enabled**, start it (`deb-systemd-invoke start`) or `try-restart` when
already active. That is boot + first configure, not a Fedora-specific
script.

## Later slices (order)

Build the packages; do not rewrite the daemons.

1. **RPM from this tree** (`cargo-generate-rpm` or an equivalent spec).
   Same binaries and unit files. Install units under
   `/usr/lib/systemd/system` (Fedora/openSUSE). `%post` creates the
   `keystone` user with `useradd`/`groupadd`, owns `/var/lib/keystone` the
   same way as Debian (`chown` that inode only, no `chown -R`), and starts
   **if enabled** without enabling `keystone-sys.socket`. glibc must match
   the oldest target (do not build Fedora RPMs on Ubuntu 26.04 against a
   newer libc than the Server edition you ship).
2. **NetworkManager as the default addressing backend** on Fedora /
   openSUSE (status already prefers netplan when `/etc/netplan` exists).
   Fixtures for `nmcli` argv; tests must not run live `nmcli`.
3. **`dnf` apply / list** (hardcoded argv, confirm + Audit, `cfg!(test)`
   bail). Not `dnf distro-sync`. Equivalent of `NEEDRESTART_MODE=list` so
   Engine is not bounced mid-upgrade.
4. **`zypper` apply / list** on openSUSE Server / SLES. Same confirm +
   Audit. Tests must not run live zypper.
5. **Unattended equivalents** (`dnf-automatic`, transactional-update
   observe). No config editor. `unattended-upgrades` stays Debian.

Raspberry Pi OS 64-bit stays the Debian `.deb`. There is no 32-bit ARM
package.

## What we will not do

- A bespoke application per distro, or a `#ifdef fedora` copy of the UI.
- Enabling the sys helper from any package format.
- Detecting `apt` in `PATH` on a Fedora box and calling it. Family comes
  from os-release, not from which binary exists.
- Arch, Gentoo, or appliance OS as System Manage targets.
