// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Host system-admin ops (apt, leftover services, failed units, reboot,
//! journal follow, IPv4, GitLab Omnibus backup, unattended-upgrades
//! observe). No I/O — the helper and agent run them. Keep
//! `docs/dev/src/system.md` in sync.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use thiserror::Error;

use crate::rbac::Permission;

/// Unix socket the opt-in root helper listens on (`0660 root:keystone`).
pub const SYS_SOCKET_PATH: &str = "/run/keystone/sys.sock";

/// Omnibus GitLab backup binary. Docker GitLab is not this path.
pub const GITLAB_BACKUP_BIN: &str = "/opt/gitlab/bin/gitlab-backup";

/// Omnibus default dump directory. Restore is not a SysOp.
pub const GITLAB_BACKUP_DIR: &str = "/var/opt/gitlab/backups";

/// Units the System tab may follow. Not a textbox — stolen cookie reads
/// these journals only.
pub const JOURNAL_UNITS: &[&str] = &[
    "keystone-agent.service",
    "keystone-server.service",
    "docker.service",
    "ssh.service",
    "gitlab-runsvdir.service",
];

/// Binary that means the unattended-upgrades package is installed.
pub const UNATTENDED_UPGRADE_BIN: &str = "/usr/bin/unattended-upgrade";

/// Debian/Ubuntu apt periodic file. Observe only — not an editor.
pub const UNATTENDED_AUTO_UPGRADES: &str = "/etc/apt/apt.conf.d/20auto-upgrades";

/// Stamp written when apt periodic finishes an unattended run.
pub const UNATTENDED_STAMP: &str = "/var/lib/apt/periodic/unattended-upgrades-stamp";

/// Fallback last-run signal when the stamp is missing.
pub const UNATTENDED_LOG: &str = "/var/log/unattended-upgrades/unattended-upgrades.log";

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    EnumIter,
    IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum SysOp {
    Status,
    UpdatesList,
    UpdatesApply,
    UpdatesAutoremove,
    NetSet,
    GitlabBackup,
    Reboot,
    Journal,
}

impl SysOp {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Status => {
                "Host snapshot (addresses, reboot-needed, leftover services, failed units, NTP, GitLab dump age, unattended-upgrades, helper)"
            }
            Self::UpdatesList => "List pending apt upgrades",
            Self::UpdatesApply => "Apply apt upgrades",
            Self::UpdatesAutoremove => "Remove unused packages (apt-get autoremove)",
            Self::NetSet => "Set IPv4 DHCP or static on one interface",
            Self::GitlabBackup => "Create a GitLab Omnibus backup (gitlab-backup create)",
            Self::Reboot => "Reboot the node (systemctl reboot)",
            Self::Journal => "Follow journalctl for one allowlisted unit",
        }
    }

    pub fn mutating(self) -> bool {
        matches!(
            self,
            Self::UpdatesApply
                | Self::UpdatesAutoremove
                | Self::NetSet
                | Self::GitlabBackup
                | Self::Reboot
        )
    }

    pub fn permission(self) -> Permission {
        if self.mutating() {
            Permission::SysManage
        } else {
            Permission::SysView
        }
    }

    pub fn streams(self) -> bool {
        matches!(
            self,
            Self::UpdatesApply | Self::UpdatesAutoremove | Self::GitlabBackup | Self::Journal
        )
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SysError {
    #[error("interface name is invalid")]
    Iface,
    #[error("IPv4 address is invalid")]
    Address,
    #[error("prefix must be 1–32")]
    Prefix,
    #[error("gateway is invalid")]
    Gateway,
    #[error("DNS address is invalid")]
    Dns,
    #[error("static IPv4 needs address, prefix, and gateway")]
    StaticIncomplete,
    #[error("unknown sys op")]
    Op,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetMethod {
    Dhcp,
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetSet {
    pub iface: String,
    pub method: NetMethod,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub prefix: u8,
    #[serde(default)]
    pub gateway: String,
    #[serde(default)]
    pub dns: Vec<String>,
}

impl NetSet {
    pub fn parse_json(raw: &str) -> Result<Self, SysError> {
        let v: Self = serde_json::from_str(raw).map_err(|_| SysError::Op)?;
        v.validate()
    }

    pub fn validate(mut self) -> Result<Self, SysError> {
        self.iface = validate_iface(&self.iface)?;
        match self.method {
            NetMethod::Dhcp => {
                self.address.clear();
                self.prefix = 0;
                self.gateway.clear();
                self.dns.clear();
            }
            NetMethod::Static => {
                if self.address.trim().is_empty() || self.gateway.trim().is_empty() {
                    return Err(SysError::StaticIncomplete);
                }
                let addr = validate_ipv4(&self.address)?;
                self.address = addr.to_string();
                if !(1..=32).contains(&self.prefix) {
                    return Err(SysError::Prefix);
                }
                let gw = validate_ipv4(&self.gateway)?;
                self.gateway = gw.to_string();
                let mut dns = Vec::new();
                for d in self.dns.iter().take(3) {
                    if d.trim().is_empty() {
                        continue;
                    }
                    dns.push(validate_ipv4(d)?.to_string());
                }
                self.dns = dns;
            }
        }
        Ok(self)
    }
}

/// Linux iface token, not Wi-Fi/virtual, not a shell string.
pub fn validate_iface(raw: &str) -> Result<String, SysError> {
    let s = raw.trim();
    if s.is_empty() || s.len() > 15 {
        return Err(SysError::Iface);
    }
    let b = s.as_bytes();
    if !b[0].is_ascii_alphabetic() {
        return Err(SysError::Iface);
    }
    if !b
        .iter()
        .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
    {
        return Err(SysError::Iface);
    }
    let lower = s.to_ascii_lowercase();
    if lower == "lo"
        || lower.starts_with("lo.")
        || lower.starts_with("wl")
        || lower.starts_with("ww")
        || lower.starts_with("docker")
        || lower.starts_with("br-")
        || lower.starts_with("veth")
        || lower.starts_with("virbr")
        || lower.starts_with("cni")
        || lower.starts_with("flannel")
        || lower.starts_with("tun")
        || lower.starts_with("tap")
    {
        return Err(SysError::Iface);
    }
    Ok(s.to_string())
}

pub fn validate_ipv4(raw: &str) -> Result<Ipv4Addr, SysError> {
    let s = raw.trim();
    if s.is_empty() || s.contains('/') || s.contains(';') || s.contains('|') {
        return Err(SysError::Address);
    }
    s.parse::<Ipv4Addr>().map_err(|_| SysError::Address)
}

/// Netplan fragment for `/etc/netplan/99-keystone.yaml` only.
pub fn netplan_yaml(req: &NetSet) -> Result<String, SysError> {
    let req = req.clone().validate()?;
    let mut out = String::from("network:\n  version: 2\n  ethernets:\n");
    match req.method {
        NetMethod::Dhcp => {
            out.push_str(&format!("    {}:\n      dhcp4: true\n", req.iface));
        }
        NetMethod::Static => {
            out.push_str(&format!(
                "    {}:\n      dhcp4: false\n      addresses:\n        - {}/{}\n      routes:\n        - to: default\n          via: {}\n",
                req.iface, req.address, req.prefix, req.gateway
            ));
            if !req.dns.is_empty() {
                out.push_str("      nameservers:\n        addresses:\n");
                for d in &req.dns {
                    out.push_str(&format!("          - {d}\n"));
                }
            }
        }
    }
    Ok(out)
}

/// `nmcli connection modify` argv after the connection id (no shell).
pub fn nmcli_modify_args(req: &NetSet) -> Result<Vec<String>, SysError> {
    let req = req.clone().validate()?;
    let mut args = vec!["connection".into(), "modify".into(), req.iface.clone()];
    match req.method {
        NetMethod::Dhcp => {
            args.extend([
                "ipv4.method".into(),
                "auto".into(),
                "ipv4.addresses".into(),
                "".into(),
                "ipv4.gateway".into(),
                "".into(),
                "ipv4.dns".into(),
                "".into(),
            ]);
        }
        NetMethod::Static => {
            args.extend([
                "ipv4.method".into(),
                "manual".into(),
                "ipv4.addresses".into(),
                format!("{}/{}", req.address, req.prefix),
                "ipv4.gateway".into(),
                req.gateway,
            ]);
            if !req.dns.is_empty() {
                args.push("ipv4.dns".into());
                args.push(req.dns.join(" "));
            }
        }
    }
    Ok(args)
}

/// Max packages returned by Check for updates (UI table).
pub const UPDATES_LIST_CAP: usize = 500;

/// Max leftover / failed unit names returned in `status`.
pub const HOST_UNIT_LIST_CAP: usize = 32;

/// `Inst pkg [old] (new …)` lines from `apt-get -s upgrade` / `dist-upgrade`,
/// plus apt 2.9+ `Upgrading:` sections and `kept back` names.
pub fn parse_apt_simulate(stdout: &str) -> Vec<Upgradable> {
    let mut out = parse_apt_inst_lines(stdout);
    merge_upgradable(&mut out, parse_apt_named_section(stdout, "Upgrading:"));
    merge_upgradable(
        &mut out,
        parse_apt_named_section(stdout, "The following packages will be upgraded:"),
    );
    merge_upgradable(
        &mut out,
        parse_apt_named_section(stdout, "The following packages have been kept back:"),
    );
    cap_upgradable(out)
}

/// `apt list --upgradable` lines (`pkg/suite version arch [upgradable from: old]`).
pub fn parse_apt_list_upgradable(stdout: &str) -> Vec<Upgradable> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        let Some(from_at) = line.find("[upgradable from:") else {
            continue;
        };
        let Some(slash) = line.find('/') else {
            continue;
        };
        let name = line[..slash].trim();
        if !package_name_ok(name) {
            continue;
        }
        let after_suite = line[slash + 1..from_at].trim();
        let mut fields = after_suite.split_whitespace();
        let _suite = fields.next();
        let to = fields.next().unwrap_or("").to_string();
        let from_rest = line[from_at + "[upgradable from:".len()..].trim();
        let from = from_rest.trim_end_matches(']').trim().to_string();
        out.push(Upgradable {
            name: name.to_string(),
            from,
            to,
        });
        if out.len() >= UPDATES_LIST_CAP {
            break;
        }
    }
    cap_upgradable(out)
}

/// Union by package name. Existing `from`/`to` win when the extra row is blank.
pub fn merge_upgradable(into: &mut Vec<Upgradable>, extra: Vec<Upgradable>) {
    for pkg in extra {
        if let Some(existing) = into.iter_mut().find(|p| p.name == pkg.name) {
            if existing.from.is_empty() {
                existing.from = pkg.from;
            }
            if existing.to.is_empty() {
                existing.to = pkg.to;
            }
        } else {
            into.push(pkg);
        }
    }
}

fn cap_upgradable(mut pkgs: Vec<Upgradable>) -> Vec<Upgradable> {
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    pkgs.truncate(UPDATES_LIST_CAP);
    pkgs
}

fn parse_apt_inst_lines(stdout: &str) -> Vec<Upgradable> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Inst ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        if !package_name_ok(name) {
            continue;
        }
        let mut from = String::new();
        let mut to = String::new();
        if let Some(start) = rest.find('[') {
            if let Some(end) = rest[start + 1..].find(']') {
                from = rest[start + 1..start + 1 + end].trim().to_string();
            }
        }
        if let Some(start) = rest.find('(') {
            if let Some(ver) = rest[start + 1..].split_whitespace().next() {
                to = ver.trim().to_string();
            }
        }
        out.push(Upgradable {
            name: name.to_string(),
            from,
            to,
        });
        if out.len() >= UPDATES_LIST_CAP {
            break;
        }
    }
    out
}

fn parse_apt_named_section(stdout: &str, header: &str) -> Vec<Upgradable> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in stdout.lines() {
        let t = line.trim();
        if t == header {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if t.is_empty() || t.ends_with(':') {
            break;
        }
        for name in t.split_whitespace() {
            if package_name_ok(name) {
                out.push(Upgradable {
                    name: name.to_string(),
                    from: String::new(),
                    to: String::new(),
                });
            }
        }
    }
    out
}

fn package_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > 64 {
        return false;
    }
    b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upgradable {
    pub name: String,
    pub from: String,
    pub to: String,
}

/// `ip -j addr` (inet only).
pub fn parse_ip_addr_json(body: &str) -> Vec<IfaceAddr> {
    let Ok(rows) = serde_json::from_str::<Vec<IpLink>>(body) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        if row.ifname.is_empty() || row.ifname == "lo" {
            continue;
        }
        let mut ipv4 = Vec::new();
        for a in row.addr_info {
            if a.family == "inet" && !a.local.is_empty() {
                ipv4.push(format!("{}/{}", a.local, a.prefixlen));
            }
        }
        out.push(IfaceAddr {
            name: row.ifname,
            ipv4,
            up: row.operstate.eq_ignore_ascii_case("UP"),
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IfaceAddr {
    pub name: String,
    pub ipv4: Vec<String>,
    pub up: bool,
}

#[derive(Deserialize)]
struct IpLink {
    #[serde(default)]
    ifname: String,
    #[serde(default)]
    operstate: String,
    #[serde(default)]
    addr_info: Vec<IpAddrInfo>,
}

#[derive(Deserialize)]
struct IpAddrInfo {
    #[serde(default)]
    family: String,
    #[serde(default)]
    local: String,
    #[serde(default)]
    prefixlen: u8,
}

/// Parsed `needrestart -b` (observe only). Services that still have old
/// libraries mapped; kernel pending when `NEEDRESTART-KSTA` is 2 or 3.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NeedrestartBatch {
    pub services: Vec<String>,
    pub kernel_pending: bool,
}

/// `needrestart -b` stdout. Unknown / injected unit names are dropped.
pub fn parse_needrestart_batch(stdout: &str) -> NeedrestartBatch {
    let mut services = Vec::new();
    let mut kernel_pending = false;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("NEEDRESTART-KSTA:") {
            kernel_pending = rest.trim().parse::<u8>().ok().is_some_and(|n| n >= 2);
            continue;
        }
        let Some(rest) = line.strip_prefix("NEEDRESTART-SVC:") else {
            continue;
        };
        let name = rest.trim().trim_matches('"').trim_matches('\'');
        if !unit_name_ok(name) {
            continue;
        }
        if !services.iter().any(|s| s == name) {
            services.push(name.to_string());
        }
        if services.len() >= HOST_UNIT_LIST_CAP {
            break;
        }
    }
    services.sort();
    services.truncate(HOST_UNIT_LIST_CAP);
    NeedrestartBatch {
        services,
        kernel_pending,
    }
}

/// `systemctl --failed --plain --no-legend --no-pager` (first column).
pub fn parse_systemctl_failed(stdout: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim().trim_start_matches(['●', '*', '○', '•']).trim();
        if line.is_empty() {
            continue;
        }
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        if !unit_name_ok(name) {
            continue;
        }
        if !out.iter().any(|s| s == name) {
            out.push(name.to_string());
        }
        if out.len() >= HOST_UNIT_LIST_CAP {
            break;
        }
    }
    out.sort();
    out.truncate(HOST_UNIT_LIST_CAP);
    out
}

/// systemd unit token for display. No path, no shell.
pub fn unit_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > 256 || name.contains("..") || name.contains('/') {
        return false;
    }
    let Some((prefix, kind)) = name.rsplit_once('.') else {
        return false;
    };
    if prefix.is_empty() || kind.is_empty() || kind.len() > 16 {
        return false;
    }
    if !kind.bytes().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    b.iter().all(|&c| {
        c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_' | b'@' | b':' | b'\\')
    })
}

/// Exact allowlist match. No suffix folding, no shell string.
pub fn journal_unit(raw: &str) -> Result<&'static str, SysError> {
    let s = raw.trim();
    JOURNAL_UNITS
        .iter()
        .copied()
        .find(|u| *u == s)
        .ok_or(SysError::Op)
}

/// `timedatectl show -p NTPSynchronized --value` (`yes`/`no`) or status text.
pub fn parse_ntp_sync(stdout: &str) -> Option<bool> {
    let t = stdout.trim();
    if t.eq_ignore_ascii_case("yes") {
        return Some(true);
    }
    if t.eq_ignore_ascii_case("no") {
        return Some(false);
    }
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("NTPSynchronized=") {
            return Some(v.trim().eq_ignore_ascii_case("yes"));
        }
        if let Some(v) = line.strip_prefix("System clock synchronized:") {
            return Some(v.trim().eq_ignore_ascii_case("yes"));
        }
    }
    None
}

pub fn gitlab_backup_name_ok(name: &str) -> bool {
    if name.is_empty() || name.len() > 256 || name.contains('/') || name.contains("..") {
        return false;
    }
    name.ends_with("_gitlab_backup.tar")
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// Newest dump among `(filename, mtime_unix)` rows from the backups dir.
pub fn newest_gitlab_backup(entries: &[(String, i64)]) -> Option<(String, i64)> {
    entries
        .iter()
        .filter(|(name, _)| gitlab_backup_name_ok(name))
        .max_by_key(|(_, unix)| *unix)
        .cloned()
}

/// Last `APT::Periodic::Unattended-Upgrade` assignment in an apt conf snippet.
/// Comments are skipped. Not an editor — observe only.
pub fn parse_unattended_periodic(conf: &str) -> Option<bool> {
    const KEY: &str = "APT::Periodic::Unattended-Upgrade";
    let mut found = None;
    for raw in conf.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let Some(rest) = line.strip_prefix(KEY) else {
            continue;
        };
        if let Some(v) = apt_conf_bool(rest) {
            found = Some(v);
        }
    }
    found
}

fn apt_conf_bool(rest: &str) -> Option<bool> {
    let t = rest
        .trim()
        .trim_start_matches(':')
        .trim()
        .trim_end_matches(';')
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    match t.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn mutating_ops_need_manage() {
        assert!(!SysOp::Status.mutating());
        assert!(!SysOp::UpdatesList.mutating());
        assert!(SysOp::UpdatesApply.mutating());
        assert!(SysOp::UpdatesAutoremove.mutating());
        assert!(SysOp::NetSet.mutating());
        assert!(SysOp::GitlabBackup.mutating());
        assert!(SysOp::Reboot.mutating());
        assert_eq!(SysOp::Status.permission(), Permission::SysView);
        assert_eq!(SysOp::NetSet.permission(), Permission::SysManage);
        assert_eq!(SysOp::GitlabBackup.permission(), Permission::SysManage);
        assert_eq!(SysOp::UpdatesAutoremove.permission(), Permission::SysManage);
        assert_eq!(SysOp::Reboot.permission(), Permission::SysManage);
        assert!(SysOp::UpdatesApply.streams());
        assert!(SysOp::UpdatesAutoremove.streams());
        assert!(SysOp::GitlabBackup.streams());
        assert!(!SysOp::Status.streams());
        assert!(!SysOp::Reboot.streams());
        assert_eq!(SysOp::GitlabBackup.as_str(), "gitlab_backup");
        assert!(!SysOp::Journal.mutating());
        assert_eq!(SysOp::Journal.permission(), Permission::SysView);
        assert!(SysOp::Journal.streams());
        assert_eq!(SysOp::UpdatesAutoremove.as_str(), "updates_autoremove");
        assert_eq!(UNATTENDED_UPGRADE_BIN, "/usr/bin/unattended-upgrade");
        assert_eq!(
            UNATTENDED_AUTO_UPGRADES,
            "/etc/apt/apt.conf.d/20auto-upgrades"
        );
        assert_eq!(GITLAB_BACKUP_DIR, "/var/opt/gitlab/backups");
        assert_eq!(GITLAB_BACKUP_BIN, "/opt/gitlab/bin/gitlab-backup");
        assert_eq!(JOURNAL_UNITS.len(), 5);
    }

    #[test]
    fn mutating_sys_ops_are_in_the_ui() {
        let js = include_str!("../../keystone-server/src/static/app.js");
        for op in SysOp::iter() {
            if !op.mutating() {
                continue;
            }
            let needle = match op {
                SysOp::UpdatesApply => "/sys/updates",
                SysOp::UpdatesAutoremove => "/sys/autoremove",
                SysOp::NetSet => "/sys/net_set",
                SysOp::GitlabBackup => "/sys/gitlab-backup",
                SysOp::Reboot => "/sys/reboot",
                SysOp::Status | SysOp::UpdatesList | SysOp::Journal => {
                    unreachable!("not mutating")
                }
            };
            assert!(
                js.contains(needle),
                "{} must appear in the System tab as {needle}",
                op.as_str()
            );
        }
        assert!(
            !js.contains("/sys/poweroff"),
            "poweroff stays out of this slice"
        );
        assert!(
            !js.contains("/sys/shutdown"),
            "shutdown stays out of this slice"
        );
        assert!(!SysOp::Status.mutating());
        assert!(!SysOp::UpdatesList.mutating());
    }

    #[test]
    fn journal_units_are_fixed_and_in_the_ui() {
        let js = include_str!("../../keystone-server/src/static/app.js");
        assert!(
            js.contains("/sys/journal/"),
            "System tab must link to journal follow pages"
        );
        assert_eq!(journal_unit(" ssh.service\n").unwrap(), "ssh.service");
        assert_eq!(journal_unit("ssh.service;rm"), Err(SysError::Op));
        assert_eq!(journal_unit("cron.service"), Err(SysError::Op));
        assert_eq!(journal_unit("sshd.service"), Err(SysError::Op));
        assert_eq!(journal_unit("SSH.service"), Err(SysError::Op));
        assert_eq!(journal_unit("ssh"), Err(SysError::Op));
        let block = js
            .split("el(\"h3\", null, \"Journals\")")
            .nth(1)
            .expect("Journals heading")
            .split(".forEach((unit)")
            .next()
            .expect("journal unit array");
        let listed: Vec<&str> = block
            .split('"')
            .skip(1)
            .step_by(2)
            .filter(|s| s.ends_with(".service"))
            .collect();
        assert_eq!(
            listed, JOURNAL_UNITS,
            "System tab unit list must match JOURNAL_UNITS in order"
        );
        assert!(
            !js.contains("name=\"unit\""),
            "journal must not be a textbox"
        );
    }

    #[test]
    fn iface_and_ip_reject_shell() {
        assert!(validate_iface("eth0").is_ok());
        assert!(validate_iface("enp0s3").is_ok());
        assert_eq!(validate_iface("eth0;rm"), Err(SysError::Iface));
        assert_eq!(validate_iface("wlan0"), Err(SysError::Iface));
        assert_eq!(validate_iface("docker0"), Err(SysError::Iface));
        assert_eq!(validate_iface("lo"), Err(SysError::Iface));
        assert!(validate_ipv4("192.168.0.50").is_ok());
        assert_eq!(validate_ipv4("1.2.3.4;reboot"), Err(SysError::Address));
        assert_eq!(validate_ipv4("192.168.0.50/24"), Err(SysError::Address));
    }

    #[test]
    fn static_netset_round_trip() {
        let req = NetSet {
            iface: "eth0".into(),
            method: NetMethod::Static,
            address: "192.168.0.50".into(),
            prefix: 24,
            gateway: "192.168.0.1".into(),
            dns: vec!["1.1.1.1".into()],
        }
        .validate()
        .unwrap();
        let yaml = netplan_yaml(&req).unwrap();
        assert!(yaml.contains("eth0:"));
        assert!(yaml.contains("192.168.0.50/24"));
        assert!(yaml.contains("via: 192.168.0.1"));
        assert!(!yaml.contains(';'));
        let args = nmcli_modify_args(&req).unwrap();
        assert_eq!(args[0], "connection");
        assert!(args.contains(&"manual".into()));
        assert!(args.contains(&"192.168.0.50/24".into()));
    }

    #[test]
    fn dhcp_clears_static_fields() {
        let req = NetSet {
            iface: "enp1s0".into(),
            method: NetMethod::Dhcp,
            address: "10.0.0.9".into(),
            prefix: 8,
            gateway: "10.0.0.1".into(),
            dns: vec!["8.8.8.8".into()],
        }
        .validate()
        .unwrap();
        assert!(req.address.is_empty());
        let yaml = netplan_yaml(&req).unwrap();
        assert!(yaml.contains("dhcp4: true"));
        assert!(!yaml.contains("10.0.0.9"));
    }

    #[test]
    fn parse_apt_inst_lines() {
        let pkgs = parse_apt_simulate(
            "NOTE: This is only a simulation!\nInst git [1:2.34.1-1] (1:2.34.1-2 Ubuntu:22.04 [amd64])\nConf git (1:2.34.1-2 Ubuntu:22.04 [amd64])\n",
        );
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "git");
        assert_eq!(pkgs[0].from, "1:2.34.1-1");
        assert_eq!(pkgs[0].to, "1:2.34.1-2");
    }

    #[test]
    fn parse_apt_list_upgradable_lines() {
        let pkgs = parse_apt_list_upgradable(
            "Listing...\ngit/noble-updates 1:2.43.0-1ubuntu7.3 amd64 [upgradable from: 1:2.43.0-1ubuntu7.1]\ncurl/noble-updates,noble-security 8.5.0-2ubuntu10.6 amd64 [upgradable from: 8.5.0-2ubuntu10.4]\n",
        );
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "curl");
        assert_eq!(pkgs[0].from, "8.5.0-2ubuntu10.4");
        assert_eq!(pkgs[0].to, "8.5.0-2ubuntu10.6");
        assert_eq!(pkgs[1].name, "git");
        assert_eq!(pkgs[1].from, "1:2.43.0-1ubuntu7.1");
        assert_eq!(pkgs[1].to, "1:2.43.0-1ubuntu7.3");
    }

    #[test]
    fn parse_apt_new_upgrading_section_and_kept_back() {
        let pkgs = parse_apt_simulate(
            "Upgrading:\n  curl vim\n\nThe following packages have been kept back:\n  linux-generic\n",
        );
        let names: Vec<_> = pkgs.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"curl"));
        assert!(names.contains(&"vim"));
        assert!(names.contains(&"linux-generic"));
    }

    #[test]
    fn merge_upgradable_fills_blank_versions() {
        let mut into = parse_apt_simulate("Upgrading:\n  git\n");
        merge_upgradable(
            &mut into,
            parse_apt_list_upgradable("git/noble 1:2.43.0 amd64 [upgradable from: 1:2.34.1]\n"),
        );
        assert_eq!(into.len(), 1);
        assert_eq!(into[0].from, "1:2.34.1");
        assert_eq!(into[0].to, "1:2.43.0");
    }

    #[test]
    fn parse_apt_caps_at_updates_list_cap() {
        let mut inst = String::new();
        let mut list = String::new();
        for i in 0..(UPDATES_LIST_CAP + 25) {
            inst.push_str(&format!("Inst pkg{i} [1] (2 Debian [amd64])\n"));
            list.push_str(&format!("pkg{i}/stable 2 amd64 [upgradable from: 1]\n"));
        }
        assert_eq!(parse_apt_simulate(&inst).len(), UPDATES_LIST_CAP);
        assert_eq!(parse_apt_list_upgradable(&list).len(), UPDATES_LIST_CAP);
    }

    #[test]
    fn parse_ip_json_skips_loopback() {
        let ifaces = parse_ip_addr_json(
            r#"[{"ifname":"lo","operstate":"UNKNOWN","addr_info":[{"family":"inet","local":"127.0.0.1","prefixlen":8}]},{"ifname":"eth0","operstate":"UP","addr_info":[{"family":"inet","local":"192.168.0.10","prefixlen":24},{"family":"inet6","local":"fe80::1","prefixlen":64}]}]"#,
        );
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces[0].name, "eth0");
        assert_eq!(ifaces[0].ipv4, vec!["192.168.0.10/24"]);
        assert!(ifaces[0].up);
    }

    #[test]
    fn compose_and_unknown_ops_are_not_sys() {
        assert!("compose_update".parse::<SysOp>().is_err());
        assert!("compose_pull".parse::<SysOp>().is_err());
        assert!("not_an_op".parse::<SysOp>().is_err());
        assert_eq!("status".parse::<SysOp>().unwrap(), SysOp::Status);
        assert_eq!(
            "gitlab_backup".parse::<SysOp>().unwrap(),
            SysOp::GitlabBackup
        );
        assert_eq!("reboot".parse::<SysOp>().unwrap(), SysOp::Reboot);
        assert_eq!("journal".parse::<SysOp>().unwrap(), SysOp::Journal);
        assert_eq!(
            "updates_autoremove".parse::<SysOp>().unwrap(),
            SysOp::UpdatesAutoremove
        );
        assert!("poweroff".parse::<SysOp>().is_err());
        assert!("shutdown".parse::<SysOp>().is_err());
    }

    #[test]
    fn parse_needrestart_batch_lists_services_and_kernel() {
        let parsed = parse_needrestart_batch(
            "NEEDRESTART-VER: 3.6\nNEEDRESTART-KCUR: 6.8.0-40-generic\nNEEDRESTART-KEXP: 6.8.0-41-generic\nNEEDRESTART-KSTA: 3\nNEEDRESTART-SVC: ssh.service\nNEEDRESTART-SVC: docker.service\nNEEDRESTART-CONT: some-container\nNEEDRESTART-SVC: ssh.service;rm\nNEEDRESTART-SVC: ../../etc/passwd\nNEEDRESTART-UCSTA: 0\n",
        );
        assert!(parsed.kernel_pending);
        assert_eq!(parsed.services, vec!["docker.service", "ssh.service"]);
        let idle = parse_needrestart_batch("NEEDRESTART-KSTA: 1\n");
        assert!(!idle.kernel_pending);
        assert!(idle.services.is_empty());
    }

    #[test]
    fn parse_systemctl_failed_strips_bullet_and_rejects_shell() {
        let units = parse_systemctl_failed(
            "● apparmor.service loaded failed failed Load AppArmor profiles\nssh.service loaded failed failed OpenBSD Secure Shell\nfoo.service;reboot loaded failed failed nope\n",
        );
        assert_eq!(units, vec!["apparmor.service", "ssh.service"]);
        assert!(parse_systemctl_failed("").is_empty());
        assert!(!unit_name_ok("ssh.service;rm"));
        assert!(!unit_name_ok("../escape.service"));
        assert!(unit_name_ok("user@1000.service"));
    }

    #[test]
    fn host_unit_lists_cap() {
        let mut nr = String::from("NEEDRESTART-KSTA: 1\n");
        let mut failed = String::new();
        for i in 0..(HOST_UNIT_LIST_CAP + 8) {
            nr.push_str(&format!("NEEDRESTART-SVC: pkg{i}.service\n"));
            failed.push_str(&format!("pkg{i}.service loaded failed failed x\n"));
        }
        assert_eq!(
            parse_needrestart_batch(&nr).services.len(),
            HOST_UNIT_LIST_CAP
        );
        assert_eq!(parse_systemctl_failed(&failed).len(), HOST_UNIT_LIST_CAP);
    }

    #[test]
    fn parse_ntp_sync_yes_no_and_status_text() {
        assert_eq!(parse_ntp_sync("yes\n"), Some(true));
        assert_eq!(parse_ntp_sync("no"), Some(false));
        assert_eq!(parse_ntp_sync("NTPSynchronized=yes\n"), Some(true));
        assert_eq!(parse_ntp_sync("NTPSynchronized=no"), Some(false));
        assert_eq!(
            parse_ntp_sync("               Local time: Thu 2026-08-20 10:00:00 UTC\nSystem clock synchronized: yes\nNTP service: active\n"),
            Some(true)
        );
        assert_eq!(
            parse_ntp_sync("System clock synchronized: no\n"),
            Some(false)
        );
        assert_eq!(parse_ntp_sync("YES"), Some(true));
        assert_eq!(parse_ntp_sync("timedatectl: command not found"), None);
        assert_eq!(parse_ntp_sync("yes, later"), None);
    }

    #[test]
    fn newest_gitlab_backup_picks_mtime_and_rejects_paths() {
        assert!(gitlab_backup_name_ok(
            "1712345678_2026_04_05_16.11.3_gitlab_backup.tar"
        ));
        assert!(!gitlab_backup_name_ok("../escape_gitlab_backup.tar"));
        assert!(!gitlab_backup_name_ok("foo_gitlab_backup.tar;rm"));
        assert!(!gitlab_backup_name_ok("notes.txt"));
        let newest = newest_gitlab_backup(&[
            ("old_gitlab_backup.tar".into(), 100),
            ("new_gitlab_backup.tar".into(), 200),
            ("skip.txt".into(), 999),
        ])
        .expect("newest");
        assert_eq!(newest.0, "new_gitlab_backup.tar");
        assert_eq!(newest.1, 200);
        assert!(newest_gitlab_backup(&[]).is_none());
    }

    #[test]
    fn parse_unattended_periodic_last_assignment_wins() {
        assert_eq!(
            parse_unattended_periodic(
                "APT::Periodic::Update-Package-Lists \"1\";\nAPT::Periodic::Unattended-Upgrade \"1\";\n"
            ),
            Some(true)
        );
        assert_eq!(
            parse_unattended_periodic("APT::Periodic::Unattended-Upgrade \"0\";\n"),
            Some(false)
        );
        assert_eq!(
            parse_unattended_periodic(
                "APT::Periodic::Unattended-Upgrade \"0\";\nAPT::Periodic::Unattended-Upgrade \"1\";\n"
            ),
            Some(true)
        );
        assert_eq!(
            parse_unattended_periodic("// APT::Periodic::Unattended-Upgrade \"1\";\n"),
            None
        );
        assert_eq!(
            parse_unattended_periodic("# APT::Periodic::Unattended-Upgrade \"1\";\n"),
            None
        );
        assert_eq!(parse_unattended_periodic(""), None);
        assert_eq!(
            parse_unattended_periodic("APT::Periodic::Unattended-Upgrade 1;\n"),
            Some(true)
        );
        assert_eq!(
            parse_unattended_periodic("APT::Periodic::Unattended-Upgrade \"true\";\n"),
            Some(true)
        );
        assert_eq!(
            parse_unattended_periodic("APT::Periodic::Unattended-Upgrade \"false\";\n"),
            Some(false)
        );
        assert_eq!(
            parse_unattended_periodic("APT::Periodic::Unattended-Upgrade \"yes, later\";\n"),
            None
        );
    }
}
