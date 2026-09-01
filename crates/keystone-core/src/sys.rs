// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Host system-admin ops (apt, leftover services, failed units, unit
//! restart from those lists, reboot, journal follow, IPv4/IPv6, 802.1Q VLAN
//! create, Wi-Fi join from a scan list, SSH password-auth toggle, GitLab
//! Omnibus backup/restore, unattended-upgrades observe). No I/O — the helper
//! and agent run them. Keep `docs/dev/src/system.md` in sync.

use std::net::{Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use thiserror::Error;

use crate::rbac::Permission;

/// Unix socket the opt-in root helper listens on (`0660 root:keystone`).
pub const SYS_SOCKET_PATH: &str = "/run/keystone/sys.sock";

/// Omnibus GitLab backup binary. Docker GitLab is not this path.
pub const GITLAB_BACKUP_BIN: &str = "/opt/gitlab/bin/gitlab-backup";

/// Omnibus `gitlab-ctl`. Restore stops puma/sidekiq then restarts.
pub const GITLAB_CTL_BIN: &str = "/opt/gitlab/bin/gitlab-ctl";

/// Omnibus default dump directory. Restore picks a listed name here.
pub const GITLAB_BACKUP_DIR: &str = "/var/opt/gitlab/backups";

/// Cap `status` dump names offered for restore. Newest first.
pub const GITLAB_RESTORE_LIST_CAP: usize = 50;

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
    VlanAdd,
    WifiScan,
    WifiJoin,
    SshPassword,
    GitlabBackup,
    GitlabRestore,
    Reboot,
    Journal,
    UnitRestart,
}

impl SysOp {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Status => {
                "Host snapshot (addresses, reboot-needed, leftover services, failed units, NTP, GitLab dump age, unattended-upgrades, SSH password-auth, helper)"
            }
            Self::UpdatesList => "List pending apt upgrades",
            Self::UpdatesApply => "Apply apt upgrades",
            Self::UpdatesAutoremove => "Remove unused packages (apt-get autoremove)",
            Self::NetSet => "Set IPv4/IPv6 DHCP or static on one Ethernet interface",
            Self::VlanAdd => "Create an 802.1Q VLAN on a listed Ethernet parent",
            Self::WifiScan => "List nearby Wi-Fi SSIDs on one wireless interface",
            Self::WifiJoin => "Join a listed Wi-Fi SSID on one wireless interface",
            Self::SshPassword => "Allow or refuse SSH password logins on this host",
            Self::GitlabBackup => "Create a GitLab Omnibus backup (gitlab-backup create)",
            Self::GitlabRestore => {
                "Restore GitLab Omnibus from a listed dump (gitlab-backup restore)"
            }
            Self::Reboot => "Reboot the node (systemctl reboot)",
            Self::Journal => "Follow journalctl for one allowlisted unit",
            Self::UnitRestart => {
                "Restart one leftover or failed unit (systemctl restart, listed names only)"
            }
        }
    }

    pub fn mutating(self) -> bool {
        matches!(
            self,
            Self::UpdatesApply
                | Self::UpdatesAutoremove
                | Self::NetSet
                | Self::VlanAdd
                | Self::WifiJoin
                | Self::SshPassword
                | Self::GitlabBackup
                | Self::GitlabRestore
                | Self::Reboot
                | Self::UnitRestart
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
            Self::UpdatesApply
                | Self::UpdatesAutoremove
                | Self::GitlabBackup
                | Self::GitlabRestore
                | Self::Journal
        )
    }

    /// Fresh authenticator code when TOTP is on. Addressing can drop SSH and
    /// the agent; restarting leftover docker/ssh/keystone-server can too;
    /// GitLab restore replaces application data; joining Wi-Fi can drop the
    /// session if that is how you reach the node; turning off SSH passwords
    /// can lock you out of the box.
    pub fn needs_step_up(self) -> bool {
        matches!(
            self,
            Self::NetSet
                | Self::VlanAdd
                | Self::WifiJoin
                | Self::SshPassword
                | Self::UnitRestart
                | Self::GitlabRestore
        )
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SysError {
    #[error("interface name is invalid")]
    Iface,
    #[error("IPv4 address is invalid")]
    Address,
    #[error("IPv6 address is invalid")]
    Ipv6Address,
    #[error("prefix must be 1–32")]
    Prefix,
    #[error("IPv6 prefix must be 1–128")]
    Ipv6Prefix,
    #[error("gateway is invalid")]
    Gateway,
    #[error("DNS address is invalid")]
    Dns,
    #[error("static IPv4 needs address, prefix, and gateway")]
    StaticIncomplete,
    #[error("static IPv6 needs address, prefix, and gateway")]
    Ipv6StaticIncomplete,
    #[error("unknown sys op")]
    Op,
    #[error("unit name is invalid")]
    Unit,
    #[error("backup name is invalid")]
    Backup,
    #[error("VLAN id must be 1–4094")]
    Vlan,
    #[error("SSID is invalid")]
    Ssid,
    #[error("Wi-Fi password is invalid")]
    Psk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetMethod {
    Dhcp,
    Static,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Ipv6Method {
    #[default]
    Auto,
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
    #[serde(default)]
    pub ipv6_method: Ipv6Method,
    #[serde(default)]
    pub ipv6_address: String,
    #[serde(default)]
    pub ipv6_prefix: u8,
    #[serde(default)]
    pub ipv6_gateway: String,
    #[serde(default)]
    pub ipv6_dns: Vec<String>,
}

impl NetSet {
    pub fn parse_json(raw: &str) -> Result<Self, SysError> {
        let v: Self = serde_json::from_str(raw).map_err(|_| SysError::Op)?;
        v.validate()
    }

    pub fn validate(mut self) -> Result<Self, SysError> {
        self.iface = validate_iface(&self.iface)?;
        if self.iface.contains('.') && parse_vlan_iface(&self.iface).is_none() {
            return Err(SysError::Iface);
        }
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
        match self.ipv6_method {
            Ipv6Method::Auto => {
                self.ipv6_address.clear();
                self.ipv6_prefix = 0;
                self.ipv6_gateway.clear();
                self.ipv6_dns.clear();
            }
            Ipv6Method::Static => {
                if self.ipv6_address.trim().is_empty() || self.ipv6_gateway.trim().is_empty() {
                    return Err(SysError::Ipv6StaticIncomplete);
                }
                let addr = validate_ipv6(&self.ipv6_address)?;
                self.ipv6_address = addr.to_string();
                if !(1..=128).contains(&self.ipv6_prefix) {
                    return Err(SysError::Ipv6Prefix);
                }
                let gw = validate_ipv6(&self.ipv6_gateway)?;
                self.ipv6_gateway = gw.to_string();
                let mut dns = Vec::new();
                for d in self.ipv6_dns.iter().take(3) {
                    if d.trim().is_empty() {
                        continue;
                    }
                    dns.push(validate_ipv6(d)?.to_string());
                }
                self.ipv6_dns = dns;
            }
        }
        Ok(self)
    }
}

/// Create `parent.vid` (for example `eth0.10`). Parent is a listed Ethernet
/// name, not a VLAN and not a name textbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VlanAdd {
    pub iface: String,
    pub vlan: u16,
}

impl VlanAdd {
    pub fn parse_json(raw: &str) -> Result<Self, SysError> {
        let v: Self = serde_json::from_str(raw).map_err(|_| SysError::Op)?;
        v.validate()
    }

    pub fn validate(mut self) -> Result<Self, SysError> {
        self.iface = validate_iface(&self.iface)?;
        if self.iface.contains('.') {
            return Err(SysError::Iface);
        }
        if !(1..=4094).contains(&self.vlan) {
            return Err(SysError::Vlan);
        }
        let _ = validate_iface(&self.iface_name())?;
        Ok(self)
    }

    pub fn iface_name(&self) -> String {
        format!("{}.{}", self.iface, self.vlan)
    }
}

/// Scan or join one wireless LAN interface (`wlan0`, `wlp3s0`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiIface {
    pub iface: String,
}

impl WifiIface {
    pub fn parse_json(raw: &str) -> Result<Self, SysError> {
        let v: Self = serde_json::from_str(raw).map_err(|_| SysError::Op)?;
        v.validate()
    }

    pub fn validate(mut self) -> Result<Self, SysError> {
        self.iface = validate_wifi_iface(&self.iface)?;
        Ok(self)
    }
}

/// Join a listed SSID. `psk` is argv-only and must not be audited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiJoin {
    pub iface: String,
    pub ssid: String,
    pub psk: String,
}

impl WifiJoin {
    pub fn parse_json(raw: &str) -> Result<Self, SysError> {
        let v: Self = serde_json::from_str(raw).map_err(|_| SysError::Op)?;
        v.validate()
    }

    pub fn validate(mut self) -> Result<Self, SysError> {
        self.iface = validate_wifi_iface(&self.iface)?;
        self.ssid = validate_ssid(&self.ssid)?;
        self.psk = validate_psk(&self.psk)?;
        Ok(self)
    }
}

/// Allow or refuse SSH password logins. Not a user / `sshd_config` editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshPassword {
    pub password_auth: bool,
}

impl SshPassword {
    pub fn parse_json(raw: &str) -> Result<Self, SysError> {
        let v: Self = serde_json::from_str(raw).map_err(|_| SysError::Op)?;
        Ok(v)
    }
}

/// Form select `yes` / `no` only. JSON uses a bool.
pub fn parse_password_auth(raw: &str) -> Result<bool, SysError> {
    match raw.trim() {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err(SysError::Op),
    }
}

/// Drop-in so KeyStone wins first-match over `50-cloud-init.conf`.
pub const SSHD_KEYSTONE_DROPIN: &str = "/etc/ssh/sshd_config.d/00-keystone.conf";

/// Packaged OpenSSH server. PATH may omit `/usr/sbin` for the helper.
pub const SSHD_BIN: &str = "/usr/sbin/sshd";

pub fn sshd_t_args() -> Vec<String> {
    vec!["-T".into()]
}

pub fn sshd_test_args() -> Vec<String> {
    vec!["-t".into()]
}

pub fn ssh_reload_args(unit: &str) -> Result<Vec<String>, SysError> {
    match unit {
        "ssh.service" | "sshd.service" => Ok(vec!["reload".into(), "--".into(), unit.into()]),
        _ => Err(SysError::Unit),
    }
}

/// `PasswordAuthentication` only. No `PermitRootLogin`, `Match`, or port.
pub fn sshd_keystone_dropin(password_auth: bool) -> String {
    let yesno = if password_auth { "yes" } else { "no" };
    format!("# Managed by KeyStone. PasswordAuthentication only.\nPasswordAuthentication {yesno}\n")
}

/// `sshd -T` dumps lowercase keywords. Missing keyword is unavailable.
pub fn parse_sshd_t(stdout: &str) -> Option<bool> {
    for line in stdout.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("passwordauthentication ")
            .or_else(|| line.strip_prefix("PasswordAuthentication "))
        else {
            continue;
        };
        return match rest.trim().to_ascii_lowercase().as_str() {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        };
    }
    None
}

/// Cap SSIDs returned by a scan (UI picker).
pub const WIFI_SSID_CAP: usize = 32;

/// Drop `psk` from a Wi-Fi join payload before Audit.
pub fn audit_sys_target(op: SysOp, payload: &str) -> String {
    if op != SysOp::WifiJoin {
        return payload.to_string();
    }
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(payload) else {
        return "{\"psk\":\"\"}".into();
    };
    if let Some(obj) = v.as_object_mut() {
        if obj.contains_key("psk") {
            obj.insert("psk".into(), serde_json::json!(""));
        }
    }
    v.to_string()
}

/// Netplan fragment for physical Ethernet apply.
pub const NETPLAN_KEYSTONE: &str = "/etc/netplan/99-keystone.yaml";

/// `parent.vid` when `name` is a single 802.1Q subinterface.
pub fn parse_vlan_iface(name: &str) -> Option<(String, u16)> {
    let (parent, id) = name.rsplit_once('.')?;
    if parent.is_empty() || parent.contains('.') {
        return None;
    }
    let vlan: u16 = id.parse().ok()?;
    if !(1..=4094).contains(&vlan) {
        return None;
    }
    let parent = validate_iface(parent).ok()?;
    Some((parent, vlan))
}

/// `/etc/netplan/99-keystone.yaml` or a per-VLAN fragment so Ethernet apply
/// does not wipe 802.1Q.
pub fn netplan_fragment_path(iface: &str) -> Result<String, SysError> {
    let iface = validate_iface(iface)?;
    if parse_vlan_iface(&iface).is_some() {
        Ok(format!("/etc/netplan/99-keystone-vlan-{iface}.yaml"))
    } else {
        Ok(NETPLAN_KEYSTONE.to_string())
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

/// Wireless LAN token (`wlan0`, `wlp3s0`). Not Ethernet, not WWAN, not a shell string.
pub fn validate_wifi_iface(raw: &str) -> Result<String, SysError> {
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
    if !lower.starts_with("wl") {
        return Err(SysError::Iface);
    }
    Ok(s.to_string())
}

pub fn validate_ssid(raw: &str) -> Result<String, SysError> {
    let s = raw.trim();
    if s.is_empty() || s.len() > 32 {
        return Err(SysError::Ssid);
    }
    if s.bytes()
        .any(|c| c < 0x20 || c == 0x7f || c == b'"' || c == b'\\')
    {
        return Err(SysError::Ssid);
    }
    Ok(s.to_string())
}

pub fn validate_psk(raw: &str) -> Result<String, SysError> {
    let s = raw.trim();
    if s.contains('\0')
        || s.contains('\n')
        || s.contains('\r')
        || s.contains('"')
        || s.contains('\\')
    {
        return Err(SysError::Psk);
    }
    if s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Ok(s.to_string());
    }
    if (8..=63).contains(&s.len()) {
        return Ok(s.to_string());
    }
    Err(SysError::Psk)
}

pub fn ssid_listed(ssid: &str, names: &[String]) -> bool {
    names.iter().any(|s| s == ssid)
}

/// `nmcli -t -f SSID device wifi list` lines.
pub fn parse_nmcli_wifi_list(stdout: &str) -> Vec<String> {
    cap_ssids(stdout.lines().filter_map(|line| {
        let s = line.trim();
        if s.is_empty() || s == "--" {
            return None;
        }
        validate_ssid(s).ok()
    }))
}

/// `iw dev wlan0 scan` `SSID:` lines.
pub fn parse_iw_scan(stdout: &str) -> Vec<String> {
    cap_ssids(stdout.lines().filter_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("SSID:")?;
        let s = rest.trim();
        if s.is_empty() {
            return None;
        }
        validate_ssid(s).ok()
    }))
}

fn cap_ssids(iter: impl Iterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    for s in iter {
        if out.iter().any(|e| e == &s) {
            continue;
        }
        out.push(s);
        if out.len() >= WIFI_SSID_CAP {
            break;
        }
    }
    out
}

pub fn validate_ipv4(raw: &str) -> Result<Ipv4Addr, SysError> {
    let s = raw.trim();
    if s.is_empty() || s.contains('/') || s.contains(';') || s.contains('|') {
        return Err(SysError::Address);
    }
    s.parse::<Ipv4Addr>().map_err(|_| SysError::Address)
}

pub fn validate_ipv6(raw: &str) -> Result<Ipv6Addr, SysError> {
    let s = raw.trim();
    if s.is_empty()
        || s.contains('/')
        || s.contains('%')
        || s.contains(';')
        || s.contains('|')
        || s.contains(' ')
    {
        return Err(SysError::Ipv6Address);
    }
    let addr = s.parse::<Ipv6Addr>().map_err(|_| SysError::Ipv6Address)?;
    if addr.is_unspecified()
        || addr.is_loopback()
        || addr.is_multicast()
        || addr.to_ipv4_mapped().is_some()
    {
        return Err(SysError::Ipv6Address);
    }
    Ok(addr)
}

/// Netplan fragment for `/etc/netplan/99-keystone.yaml` only.
pub fn netplan_yaml(req: &NetSet) -> Result<String, SysError> {
    let req = req.clone().validate()?;
    let mut out = if let Some((parent, id)) = parse_vlan_iface(&req.iface) {
        format!(
            "network:\n  version: 2\n  vlans:\n    {}:\n      id: {id}\n      link: {parent}\n",
            req.iface
        )
    } else {
        format!("network:\n  version: 2\n  ethernets:\n    {}:\n", req.iface)
    };
    match req.method {
        NetMethod::Dhcp => out.push_str("      dhcp4: true\n"),
        NetMethod::Static => out.push_str("      dhcp4: false\n"),
    }
    match req.ipv6_method {
        Ipv6Method::Auto => out.push_str("      dhcp6: true\n"),
        Ipv6Method::Static => out.push_str("      dhcp6: false\n"),
    }
    let mut addrs = Vec::new();
    let mut routes = Vec::new();
    if req.method == NetMethod::Static {
        addrs.push(format!("{}/{}", req.address, req.prefix));
        routes.push(format!(
            "        - to: default\n          via: {}\n",
            req.gateway
        ));
    }
    if req.ipv6_method == Ipv6Method::Static {
        addrs.push(format!("{}/{}", req.ipv6_address, req.ipv6_prefix));
        routes.push(format!(
            "        - to: default\n          via: {}\n",
            req.ipv6_gateway
        ));
    }
    if !addrs.is_empty() {
        out.push_str("      addresses:\n");
        for a in &addrs {
            out.push_str(&format!("        - {a}\n"));
        }
    }
    if !routes.is_empty() {
        out.push_str("      routes:\n");
        for r in &routes {
            out.push_str(r);
        }
    }
    let mut nameservers = req.dns.clone();
    nameservers.extend(req.ipv6_dns.iter().cloned());
    if !nameservers.is_empty() {
        out.push_str("      nameservers:\n        addresses:\n");
        for d in &nameservers {
            out.push_str(&format!("          - {d}\n"));
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
    match req.ipv6_method {
        Ipv6Method::Auto => {
            args.extend([
                "ipv6.method".into(),
                "auto".into(),
                "ipv6.addresses".into(),
                "".into(),
                "ipv6.gateway".into(),
                "".into(),
                "ipv6.dns".into(),
                "".into(),
            ]);
        }
        Ipv6Method::Static => {
            args.extend([
                "ipv6.method".into(),
                "manual".into(),
                "ipv6.addresses".into(),
                format!("{}/{}", req.ipv6_address, req.ipv6_prefix),
                "ipv6.gateway".into(),
                req.ipv6_gateway,
            ]);
            if !req.ipv6_dns.is_empty() {
                args.push("ipv6.dns".into());
                args.push(req.ipv6_dns.join(" "));
            }
        }
    }
    Ok(args)
}

/// Netplan fragment that only creates the VLAN (DHCP/SLAAC until Apply).
pub fn netplan_vlan_yaml(req: &VlanAdd) -> Result<String, SysError> {
    let req = req.clone().validate()?;
    let name = req.iface_name();
    Ok(format!(
        "network:\n  version: 2\n  vlans:\n    {name}:\n      id: {}\n      link: {}\n      dhcp4: true\n      dhcp6: true\n",
        req.vlan, req.iface
    ))
}

/// `nmcli connection add type vlan` argv (no shell).
pub fn nmcli_vlan_add_args(req: &VlanAdd) -> Result<Vec<String>, SysError> {
    let req = req.clone().validate()?;
    let name = req.iface_name();
    Ok(vec![
        "connection".into(),
        "add".into(),
        "type".into(),
        "vlan".into(),
        "con-name".into(),
        name.clone(),
        "ifname".into(),
        name,
        "dev".into(),
        req.iface,
        "id".into(),
        req.vlan.to_string(),
        "ipv4.method".into(),
        "auto".into(),
        "ipv6.method".into(),
        "auto".into(),
    ])
}

/// `nmcli device wifi list` argv after `nmcli` (no shell).
pub fn nmcli_wifi_list_args(iface: &str) -> Result<Vec<String>, SysError> {
    let iface = validate_wifi_iface(iface)?;
    Ok(vec![
        "-t".into(),
        "-f".into(),
        "SSID".into(),
        "device".into(),
        "wifi".into(),
        "list".into(),
        "ifname".into(),
        iface,
    ])
}

/// `nmcli device wifi rescan` argv after `nmcli`.
pub fn nmcli_wifi_rescan_args(iface: &str) -> Result<Vec<String>, SysError> {
    let iface = validate_wifi_iface(iface)?;
    Ok(vec![
        "device".into(),
        "wifi".into(),
        "rescan".into(),
        "ifname".into(),
        iface,
    ])
}

/// `nmcli device wifi connect` argv after `nmcli` (password is a separate argv).
pub fn nmcli_wifi_join_args(req: &WifiJoin) -> Result<Vec<String>, SysError> {
    let req = req.clone().validate()?;
    Ok(vec![
        "device".into(),
        "wifi".into(),
        "connect".into(),
        req.ssid,
        "password".into(),
        req.psk,
        "ifname".into(),
        req.iface,
    ])
}

/// `iw dev IFACE scan` argv after `iw`.
pub fn iw_scan_args(iface: &str) -> Result<Vec<String>, SysError> {
    let iface = validate_wifi_iface(iface)?;
    Ok(vec!["dev".into(), iface, "scan".into()])
}

/// Netplan Wi-Fi fragment (DHCP/SLAAC). File mode must be 600 — it holds the PSK.
pub fn netplan_wifi_yaml(req: &WifiJoin) -> Result<String, SysError> {
    let req = req.clone().validate()?;
    Ok(format!(
        "network:\n  version: 2\n  wifis:\n    {}:\n      dhcp4: true\n      dhcp6: true\n      access-points:\n        \"{}\":\n          password: \"{}\"\n",
        req.iface, req.ssid, req.psk
    ))
}

pub fn netplan_wifi_path(iface: &str) -> Result<String, SysError> {
    let iface = validate_wifi_iface(iface)?;
    Ok(format!("/etc/netplan/99-keystone-wifi-{iface}.yaml"))
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

/// `ip -j addr` (inet and inet6).
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
        let mut ipv6 = Vec::new();
        for a in row.addr_info {
            if a.local.is_empty() {
                continue;
            }
            if a.family == "inet" {
                ipv4.push(format!("{}/{}", a.local, a.prefixlen));
            }
            if a.family == "inet6" {
                ipv6.push(format!("{}/{}", a.local, a.prefixlen));
            }
        }
        out.push(IfaceAddr {
            name: row.ifname,
            ipv4,
            ipv6,
            up: row.operstate.eq_ignore_ascii_case("UP"),
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IfaceAddr {
    pub name: String,
    pub ipv4: Vec<String>,
    #[serde(default)]
    pub ipv6: Vec<String>,
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

/// Units whose restart can drop the UI, Engine, or SSH. Extra confirm in
/// the UI; the helper still only restarts if needrestart or `--failed`
/// currently lists the name.
pub const SENSITIVE_RESTART_UNITS: &[&str] =
    &["keystone-server.service", "docker.service", "ssh.service"];

pub fn sensitive_restart_unit(name: &str) -> bool {
    SENSITIVE_RESTART_UNITS.iter().any(|u| *u == name)
}

/// Form/JSON `unit` for `unit_restart`. Token only — membership of the
/// leftover/failed lists is checked on the helper from a live snapshot.
pub fn parse_restart_unit(payload: &str) -> Result<String, SysError> {
    let v: serde_json::Value = serde_json::from_str(payload).map_err(|_| SysError::Op)?;
    let name = v.get("unit").and_then(|u| u.as_str()).unwrap_or("").trim();
    if !unit_name_ok(name) {
        return Err(SysError::Unit);
    }
    Ok(name.to_string())
}

/// True when `name` is on the live leftover or failed list (exact match).
pub fn unit_listed_for_restart(name: &str, leftovers: &[String], failed: &[String]) -> bool {
    unit_name_ok(name) && (leftovers.iter().any(|u| u == name) || failed.iter().any(|u| u == name))
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

/// `BACKUP=` id for Omnibus restore: filename without `_gitlab_backup.tar`.
pub fn gitlab_backup_id(name: &str) -> Option<&str> {
    if !gitlab_backup_name_ok(name) {
        return None;
    }
    name.strip_suffix("_gitlab_backup.tar")
        .filter(|id| !id.is_empty())
}

/// Form/JSON `name` for `gitlab_restore`. Token only — membership of the
/// backups dir is checked on the helper from a live listing.
pub fn parse_restore_backup(payload: &str) -> Result<String, SysError> {
    let v: serde_json::Value = serde_json::from_str(payload).map_err(|_| SysError::Op)?;
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
    if gitlab_backup_id(name).is_none() {
        return Err(SysError::Backup);
    }
    Ok(name.to_string())
}

/// True when `name` is on the live dump list (exact match).
pub fn gitlab_restore_listed(name: &str, dumps: &[String]) -> bool {
    gitlab_backup_id(name).is_some() && dumps.iter().any(|n| n == name)
}

/// Newest first, capped. Input is `(filename, mtime_unix)`.
pub fn gitlab_backups_for_restore(entries: &[(String, i64)]) -> Vec<(String, i64)> {
    let mut v: Vec<_> = entries
        .iter()
        .filter(|(n, _)| gitlab_backup_id(n).is_some())
        .cloned()
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(GITLAB_RESTORE_LIST_CAP);
    v
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
        assert!(SysOp::VlanAdd.mutating());
        assert!(SysOp::WifiJoin.mutating());
        assert!(SysOp::SshPassword.mutating());
        assert!(!SysOp::WifiScan.mutating());
        assert_eq!(SysOp::WifiScan.permission(), Permission::SysView);
        assert_eq!(SysOp::WifiJoin.permission(), Permission::SysManage);
        assert_eq!(SysOp::SshPassword.permission(), Permission::SysManage);
        assert!(SysOp::GitlabBackup.mutating());
        assert!(SysOp::GitlabRestore.mutating());
        assert!(SysOp::Reboot.mutating());
        assert!(SysOp::UnitRestart.mutating());
        assert_eq!(SysOp::Status.permission(), Permission::SysView);
        assert_eq!(SysOp::NetSet.permission(), Permission::SysManage);
        assert_eq!(SysOp::VlanAdd.permission(), Permission::SysManage);
        assert_eq!(SysOp::GitlabBackup.permission(), Permission::SysManage);
        assert_eq!(SysOp::GitlabRestore.permission(), Permission::SysManage);
        assert_eq!(SysOp::UpdatesAutoremove.permission(), Permission::SysManage);
        assert_eq!(SysOp::Reboot.permission(), Permission::SysManage);
        assert_eq!(SysOp::UnitRestart.permission(), Permission::SysManage);
        assert!(SysOp::UpdatesApply.streams());
        assert!(SysOp::UpdatesAutoremove.streams());
        assert!(SysOp::GitlabBackup.streams());
        assert!(SysOp::GitlabRestore.streams());
        assert!(!SysOp::Status.streams());
        assert!(!SysOp::Reboot.streams());
        assert!(!SysOp::UnitRestart.streams());
        assert_eq!(SysOp::GitlabBackup.as_str(), "gitlab_backup");
        assert_eq!(SysOp::GitlabRestore.as_str(), "gitlab_restore");
        assert_eq!(SysOp::UnitRestart.as_str(), "unit_restart");
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
        assert_eq!(GITLAB_CTL_BIN, "/opt/gitlab/bin/gitlab-ctl");
        assert_eq!(GITLAB_RESTORE_LIST_CAP, 50);
        assert_eq!(JOURNAL_UNITS.len(), 5);
        assert_eq!(SysOp::VlanAdd.as_str(), "vlan_add");
        assert_eq!(SysOp::WifiScan.as_str(), "wifi_scan");
        assert_eq!(SysOp::WifiJoin.as_str(), "wifi_join");
        assert_eq!(SysOp::SshPassword.as_str(), "ssh_password");
        assert!(SysOp::VlanAdd.mutating());
        assert!(!SysOp::VlanAdd.streams());
        assert!(!SysOp::WifiScan.streams());
        assert!(!SysOp::WifiJoin.streams());
        assert!(!SysOp::SshPassword.streams());
        assert!(SysOp::NetSet.needs_step_up());
        assert!(SysOp::VlanAdd.needs_step_up());
        assert!(SysOp::WifiJoin.needs_step_up());
        assert!(SysOp::SshPassword.needs_step_up());
        assert!(!SysOp::WifiScan.needs_step_up());
        assert!(SysOp::UnitRestart.needs_step_up());
        assert!(SysOp::GitlabRestore.needs_step_up());
        assert!(!SysOp::GitlabBackup.needs_step_up());
        for op in SysOp::iter() {
            let want = matches!(
                op,
                SysOp::NetSet
                    | SysOp::VlanAdd
                    | SysOp::WifiJoin
                    | SysOp::SshPassword
                    | SysOp::UnitRestart
                    | SysOp::GitlabRestore
            );
            assert_eq!(op.needs_step_up(), want, "{} step-up", op.as_str());
        }
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
                SysOp::VlanAdd => "/sys/vlan_add",
                SysOp::WifiJoin => "/sys/wifi_join",
                SysOp::SshPassword => "/sys/ssh_password",
                SysOp::GitlabBackup => "/sys/gitlab-backup",
                SysOp::GitlabRestore => "/sys/gitlab_restore",
                SysOp::Reboot => "/sys/reboot",
                SysOp::UnitRestart => "/sys/unit_restart",
                SysOp::Status | SysOp::UpdatesList | SysOp::Journal | SysOp::WifiScan => {
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
            ipv6_method: Ipv6Method::Auto,
            ipv6_address: String::new(),
            ipv6_prefix: 0,
            ipv6_gateway: String::new(),
            ipv6_dns: vec![],
        }
        .validate()
        .unwrap();
        let yaml = netplan_yaml(&req).unwrap();
        assert!(yaml.contains("eth0:"));
        assert!(yaml.contains("192.168.0.50/24"));
        assert!(yaml.contains("via: 192.168.0.1"));
        assert!(yaml.contains("dhcp6: true"));
        assert!(!yaml.contains(';'));
        let args = nmcli_modify_args(&req).unwrap();
        assert_eq!(args[0], "connection");
        assert!(args.contains(&"manual".into()));
        assert!(args.contains(&"192.168.0.50/24".into()));
        assert!(args.contains(&"ipv6.method".into()));
        assert!(args.contains(&"auto".into()));
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
            ipv6_method: Ipv6Method::Auto,
            ipv6_address: String::new(),
            ipv6_prefix: 0,
            ipv6_gateway: String::new(),
            ipv6_dns: vec![],
        }
        .validate()
        .unwrap();
        assert!(req.address.is_empty());
        let yaml = netplan_yaml(&req).unwrap();
        assert!(yaml.contains("dhcp4: true"));
        assert!(yaml.contains("dhcp6: true"));
        assert!(!yaml.contains("10.0.0.9"));
    }

    #[test]
    fn static_ipv6_round_trip_and_rejects_shell() {
        assert!(validate_ipv6("2001:db8::10").is_ok());
        assert!(validate_ipv6("fe80::1").is_ok());
        assert_eq!(
            validate_ipv6("2001:db8::10%eth0"),
            Err(SysError::Ipv6Address)
        );
        assert_eq!(validate_ipv6("2001:db8::10/64"), Err(SysError::Ipv6Address));
        assert_eq!(validate_ipv6("::1"), Err(SysError::Ipv6Address));
        assert_eq!(validate_ipv6("::"), Err(SysError::Ipv6Address));
        assert_eq!(validate_ipv6("ff02::1"), Err(SysError::Ipv6Address));
        assert_eq!(
            validate_ipv6("::ffff:192.168.0.1"),
            Err(SysError::Ipv6Address)
        );
        let req = NetSet {
            iface: "eth0".into(),
            method: NetMethod::Dhcp,
            address: String::new(),
            prefix: 0,
            gateway: String::new(),
            dns: vec![],
            ipv6_method: Ipv6Method::Static,
            ipv6_address: "2001:db8::10".into(),
            ipv6_prefix: 64,
            ipv6_gateway: "2001:db8::1".into(),
            ipv6_dns: vec!["2001:db8::53".into()],
        }
        .validate()
        .unwrap();
        let yaml = netplan_yaml(&req).unwrap();
        assert!(yaml.contains("dhcp4: true"));
        assert!(yaml.contains("dhcp6: false"));
        assert!(yaml.contains("2001:db8::10/64"));
        assert!(yaml.contains("via: 2001:db8::1"));
        assert!(yaml.contains("2001:db8::53"));
        assert!(!yaml.contains('%') && !yaml.contains(';'));
        let args = nmcli_modify_args(&req).unwrap();
        assert!(args.contains(&"ipv6.method".into()));
        assert!(args.contains(&"manual".into()));
        assert!(args.contains(&"2001:db8::10/64".into()));
        let bad = NetSet {
            iface: "eth0".into(),
            method: NetMethod::Dhcp,
            address: String::new(),
            prefix: 0,
            gateway: String::new(),
            dns: vec![],
            ipv6_method: Ipv6Method::Static,
            ipv6_address: "2001:db8::10;rm".into(),
            ipv6_prefix: 64,
            ipv6_gateway: "2001:db8::1".into(),
            ipv6_dns: vec![],
        };
        assert_eq!(bad.validate(), Err(SysError::Ipv6Address));
        let wide = NetSet {
            iface: "eth0".into(),
            method: NetMethod::Dhcp,
            address: String::new(),
            prefix: 0,
            gateway: String::new(),
            dns: vec![],
            ipv6_method: Ipv6Method::Static,
            ipv6_address: "2001:db8::10".into(),
            ipv6_prefix: 129,
            ipv6_gateway: "2001:db8::1".into(),
            ipv6_dns: vec![],
        };
        assert_eq!(wide.validate(), Err(SysError::Ipv6Prefix));
    }

    #[test]
    fn vlan_add_round_trip_and_rejects_shell() {
        let req = VlanAdd {
            iface: "eth0".into(),
            vlan: 10,
        }
        .validate()
        .unwrap();
        assert_eq!(req.iface_name(), "eth0.10");
        assert_eq!(parse_vlan_iface("eth0.10"), Some(("eth0".into(), 10)));
        assert_eq!(netplan_fragment_path("eth0").unwrap(), NETPLAN_KEYSTONE);
        assert_eq!(
            netplan_fragment_path("eth0.10").unwrap(),
            "/etc/netplan/99-keystone-vlan-eth0.10.yaml"
        );
        let yaml = netplan_vlan_yaml(&req).unwrap();
        assert!(yaml.contains("vlans:"));
        assert!(yaml.contains("eth0.10:"));
        assert!(yaml.contains("id: 10"));
        assert!(yaml.contains("link: eth0"));
        assert!(yaml.contains("dhcp4: true"));
        assert!(yaml.contains("dhcp6: true"));
        assert!(!yaml.contains("ethernets:"));
        assert!(!yaml.contains(';') && !yaml.contains('|'));
        let args = nmcli_vlan_add_args(&req).unwrap();
        assert_eq!(args[0], "connection");
        assert!(args.contains(&"vlan".into()));
        assert!(args.contains(&"eth0.10".into()));
        assert!(args.contains(&"eth0".into()));
        assert!(args.contains(&"10".into()));
        assert!(!args.iter().any(|a| a.contains(';') || a.contains('|')));
        assert_eq!(
            VlanAdd {
                iface: "eth0;rm".into(),
                vlan: 10,
            }
            .validate(),
            Err(SysError::Iface)
        );
        assert_eq!(
            VlanAdd {
                iface: "wlan0".into(),
                vlan: 10,
            }
            .validate(),
            Err(SysError::Iface)
        );
        assert_eq!(
            VlanAdd {
                iface: "eth0.10".into(),
                vlan: 20,
            }
            .validate(),
            Err(SysError::Iface)
        );
        assert_eq!(
            VlanAdd {
                iface: "eth0".into(),
                vlan: 0,
            }
            .validate(),
            Err(SysError::Vlan)
        );
        assert_eq!(
            VlanAdd {
                iface: "eth0".into(),
                vlan: 4095,
            }
            .validate(),
            Err(SysError::Vlan)
        );
        let addressed = NetSet {
            iface: "eth0.10".into(),
            method: NetMethod::Static,
            address: "192.168.10.50".into(),
            prefix: 24,
            gateway: "192.168.10.1".into(),
            dns: vec![],
            ipv6_method: Ipv6Method::Auto,
            ipv6_address: String::new(),
            ipv6_prefix: 0,
            ipv6_gateway: String::new(),
            ipv6_dns: vec![],
        }
        .validate()
        .unwrap();
        let net_yaml = netplan_yaml(&addressed).unwrap();
        assert!(net_yaml.contains("vlans:"));
        assert!(net_yaml.contains("id: 10"));
        assert!(net_yaml.contains("link: eth0"));
        assert!(net_yaml.contains("192.168.10.50/24"));
        assert!(!net_yaml.contains("ethernets:"));
        assert_eq!(
            NetSet {
                iface: "eth0.10.20".into(),
                method: NetMethod::Dhcp,
                address: String::new(),
                prefix: 0,
                gateway: String::new(),
                dns: vec![],
                ipv6_method: Ipv6Method::Auto,
                ipv6_address: String::new(),
                ipv6_prefix: 0,
                ipv6_gateway: String::new(),
                ipv6_dns: vec![],
            }
            .validate(),
            Err(SysError::Iface)
        );
    }

    #[test]
    fn wifi_join_round_trip_and_rejects_shell() {
        assert!(validate_wifi_iface("wlan0").is_ok());
        assert!(validate_wifi_iface("wlp3s0").is_ok());
        assert_eq!(validate_wifi_iface("eth0"), Err(SysError::Iface));
        assert_eq!(validate_wifi_iface("wlan0;rm"), Err(SysError::Iface));
        assert_eq!(validate_ssid("Home Lab"), Ok("Home Lab".into()));
        assert_eq!(validate_ssid("x;rm"), Ok("x;rm".into()));
        assert_eq!(validate_ssid("bad\"ssid"), Err(SysError::Ssid));
        assert_eq!(validate_ssid(""), Err(SysError::Ssid));
        assert_eq!(validate_psk("testpass1"), Ok("testpass1".into()));
        assert_eq!(validate_psk("short"), Err(SysError::Psk));
        assert_eq!(validate_psk("has\"quote"), Err(SysError::Psk));
        let req = WifiJoin {
            iface: "wlan0".into(),
            ssid: "Home Lab".into(),
            psk: "testpass1".into(),
        }
        .validate()
        .unwrap();
        let yaml = netplan_wifi_yaml(&req).unwrap();
        assert!(yaml.contains("wifis:"));
        assert!(yaml.contains("wlan0:"));
        assert!(yaml.contains("\"Home Lab\""));
        assert!(yaml.contains("password: \"testpass1\""));
        assert!(!yaml.contains("sh -c"));
        let args = nmcli_wifi_join_args(&req).unwrap();
        assert_eq!(args[0], "device");
        assert!(args.contains(&"connect".into()));
        assert!(args.contains(&"Home Lab".into()));
        assert!(args.contains(&"testpass1".into()));
        assert!(args.contains(&"wlan0".into()));
        assert!(!args.iter().any(|a| a.contains("sh -c")));
        let list = parse_nmcli_wifi_list("Home Lab\nHome Lab\n--\nLab\n");
        assert_eq!(list, vec!["Home Lab".to_string(), "Lab".to_string()]);
        assert!(ssid_listed("Home Lab", &list));
        assert!(!ssid_listed("Evil", &list));
        let iw = parse_iw_scan("BSS aa:bb\n\tSSID: Home Lab\n\tSSID:\n\tSSID: Lab\n");
        assert_eq!(iw, vec!["Home Lab".to_string(), "Lab".to_string()]);
        let raw = r#"{"iface":"wlan0","ssid":"Home Lab","psk":"testpass1"}"#;
        let redacted = audit_sys_target(SysOp::WifiJoin, raw);
        assert!(redacted.contains("Home Lab"));
        assert!(!redacted.contains("testpass1"));
        assert_eq!(
            audit_sys_target(SysOp::NetSet, r#"{"iface":"eth0"}"#),
            r#"{"iface":"eth0"}"#
        );
        let list_args = nmcli_wifi_list_args("wlan0").unwrap();
        assert!(list_args.contains(&"list".into()));
        assert!(!list_args.iter().any(|a| a.contains(';')));
        let iw_args = iw_scan_args("wlan0").unwrap();
        assert_eq!(iw_args, vec!["dev", "wlan0", "scan"]);
        assert_eq!(
            netplan_wifi_path("wlan0").unwrap(),
            "/etc/netplan/99-keystone-wifi-wlan0.yaml"
        );
        assert_eq!(
            WifiJoin {
                iface: "eth0".into(),
                ssid: "Home".into(),
                psk: "testpass1".into(),
            }
            .validate(),
            Err(SysError::Iface)
        );
    }

    #[test]
    fn ssh_password_dropin_and_parse_not_a_config_editor() {
        assert_eq!(parse_password_auth("yes"), Ok(true));
        assert_eq!(parse_password_auth("no"), Ok(false));
        assert_eq!(parse_password_auth("yes;rm"), Err(SysError::Op));
        assert_eq!(parse_password_auth("true"), Err(SysError::Op));
        assert_eq!(
            SshPassword::parse_json(r#"{"password_auth":false}"#).unwrap(),
            SshPassword {
                password_auth: false
            }
        );
        assert_eq!(
            SshPassword::parse_json(r#"{"password_auth":"yes;rm"}"#),
            Err(SysError::Op)
        );
        let allow = sshd_keystone_dropin(true);
        let refuse = sshd_keystone_dropin(false);
        assert!(allow.contains("PasswordAuthentication yes"));
        assert!(refuse.contains("PasswordAuthentication no"));
        for body in [&allow, &refuse] {
            assert!(!body.contains("PermitRootLogin"));
            assert!(!body.contains("Match"));
            assert!(!body.contains("sh -c"));
            assert!(!body.contains("Port "));
            assert!(!body.contains("useradd"));
        }
        assert_eq!(
            SSHD_KEYSTONE_DROPIN,
            "/etc/ssh/sshd_config.d/00-keystone.conf"
        );
        assert_eq!(SSHD_BIN, "/usr/sbin/sshd");
        assert_eq!(sshd_t_args(), vec!["-T"]);
        assert_eq!(sshd_test_args(), vec!["-t"]);
        let reload = ssh_reload_args("ssh.service").unwrap();
        assert_eq!(reload, vec!["reload", "--", "ssh.service"]);
        assert!(!reload.iter().any(|a| a.contains("sh -c")));
        assert_eq!(
            ssh_reload_args("sshd.service").unwrap(),
            vec!["reload", "--", "sshd.service"]
        );
        assert_eq!(ssh_reload_args("cron.service"), Err(SysError::Unit));
        assert_eq!(
            parse_sshd_t("port 22\npasswordauthentication no\npermitrootlogin prohibit-password\n"),
            Some(false)
        );
        assert_eq!(parse_sshd_t("passwordauthentication yes\n"), Some(true));
        assert_eq!(parse_sshd_t("port 22\n"), None);
        assert!(!SysOp::SshPassword.streams());
        assert!(SysOp::SshPassword.needs_step_up());
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
        assert_eq!(ifaces[0].ipv6, vec!["fe80::1/64"]);
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
        assert_eq!(
            "gitlab_restore".parse::<SysOp>().unwrap(),
            SysOp::GitlabRestore
        );
        assert_eq!("reboot".parse::<SysOp>().unwrap(), SysOp::Reboot);
        assert_eq!("journal".parse::<SysOp>().unwrap(), SysOp::Journal);
        assert_eq!("unit_restart".parse::<SysOp>().unwrap(), SysOp::UnitRestart);
        assert_eq!("vlan_add".parse::<SysOp>().unwrap(), SysOp::VlanAdd);
        assert_eq!("wifi_scan".parse::<SysOp>().unwrap(), SysOp::WifiScan);
        assert_eq!("wifi_join".parse::<SysOp>().unwrap(), SysOp::WifiJoin);
        assert_eq!("ssh_password".parse::<SysOp>().unwrap(), SysOp::SshPassword);
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
    fn restart_unit_is_listed_names_only() {
        assert_eq!(
            parse_restart_unit(r#"{"unit":"docker.service"}"#).unwrap(),
            "docker.service"
        );
        assert_eq!(
            parse_restart_unit(r#"{"unit":" docker.service "}"#).unwrap(),
            "docker.service"
        );
        assert_eq!(
            parse_restart_unit(r#"{"unit":"docker.service;rm"}"#),
            Err(SysError::Unit)
        );
        assert_eq!(
            parse_restart_unit(r#"{"unit":"../escape.service"}"#),
            Err(SysError::Unit)
        );
        assert_eq!(parse_restart_unit("{}"), Err(SysError::Unit));
        let leftovers = vec!["docker.service".into(), "ssh.service".into()];
        let failed = vec!["apparmor.service".into()];
        assert!(unit_listed_for_restart(
            "docker.service",
            &leftovers,
            &failed
        ));
        assert!(unit_listed_for_restart(
            "apparmor.service",
            &leftovers,
            &failed
        ));
        assert!(!unit_listed_for_restart(
            "cron.service",
            &leftovers,
            &failed
        ));
        assert!(!unit_listed_for_restart(
            "docker.service;rm",
            &leftovers,
            &failed
        ));
        assert!(sensitive_restart_unit("docker.service"));
        assert!(sensitive_restart_unit("ssh.service"));
        assert!(sensitive_restart_unit("keystone-server.service"));
        assert!(!sensitive_restart_unit("apparmor.service"));
        assert!(!sensitive_restart_unit("keystone-agent.service"));
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
        let name = "1712345678_2026_04_05_16.11.3_gitlab_backup.tar";
        assert_eq!(
            gitlab_backup_id(name),
            Some("1712345678_2026_04_05_16.11.3")
        );
        assert_eq!(
            parse_restore_backup(&format!(r#"{{"name":"{name}"}}"#)).unwrap(),
            name
        );
        assert_eq!(
            parse_restore_backup(r#"{"name":" ../escape_gitlab_backup.tar"}"#),
            Err(SysError::Backup)
        );
        assert_eq!(
            parse_restore_backup(r#"{"name":"foo_gitlab_backup.tar;rm"}"#),
            Err(SysError::Backup)
        );
        assert_eq!(parse_restore_backup("{}"), Err(SysError::Backup));
        assert_eq!(parse_restore_backup("not-json"), Err(SysError::Op));
        let dumps = vec![name.to_string(), "other_gitlab_backup.tar".into()];
        assert!(gitlab_restore_listed(name, &dumps));
        assert!(!gitlab_restore_listed("missing_gitlab_backup.tar", &dumps));
        assert!(!gitlab_restore_listed("foo_gitlab_backup.tar;rm", &dumps));
        let capped = gitlab_backups_for_restore(&[
            ("old_gitlab_backup.tar".into(), 100),
            ("new_gitlab_backup.tar".into(), 200),
            ("skip.txt".into(), 999),
        ]);
        assert_eq!(
            capped.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            vec!["new_gitlab_backup.tar", "old_gitlab_backup.tar"]
        );
        let many: Vec<(String, i64)> = (0..=GITLAB_RESTORE_LIST_CAP as i64)
            .map(|i| (format!("{i}_gitlab_backup.tar"), i))
            .collect();
        assert_eq!(
            gitlab_backups_for_restore(&many).len(),
            GITLAB_RESTORE_LIST_CAP
        );
        assert_eq!(
            gitlab_backups_for_restore(&many)[0].0,
            format!("{}_gitlab_backup.tar", GITLAB_RESTORE_LIST_CAP)
        );
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
