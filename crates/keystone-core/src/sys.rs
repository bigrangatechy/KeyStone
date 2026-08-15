// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Host system-admin ops (apt, IPv4). No I/O — the helper and agent run them.
//! Keep `docs/dev/src/system.md` in sync.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use thiserror::Error;

use crate::rbac::Permission;

/// Unix socket the opt-in root helper listens on (`0660 root:keystone`).
pub const SYS_SOCKET_PATH: &str = "/run/keystone/sys.sock";

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
    NetSet,
}

impl SysOp {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Status => "Host snapshot (addresses, reboot-needed, helper)",
            Self::UpdatesList => "List pending apt upgrades",
            Self::UpdatesApply => "Apply apt upgrades",
            Self::NetSet => "Set IPv4 DHCP or static on one interface",
        }
    }

    pub fn mutating(self) -> bool {
        matches!(self, Self::UpdatesApply | Self::NetSet)
    }

    pub fn permission(self) -> Permission {
        if self.mutating() {
            Permission::SysManage
        } else {
            Permission::SysView
        }
    }

    pub fn streams(self) -> bool {
        matches!(self, Self::UpdatesApply)
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

/// `Inst pkg [old] (new …)` lines from `apt-get -s upgrade`.
pub fn parse_apt_simulate(stdout: &str) -> Vec<Upgradable> {
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
        let joined = rest.to_string();
        if let Some(start) = joined.find('[') {
            if let Some(end) = joined[start + 1..].find(']') {
                from = joined[start + 1..start + 1 + end].trim().to_string();
            }
        }
        if let Some(start) = joined.find('(') {
            if let Some(ver) = joined[start + 1..].split_whitespace().next() {
                to = ver.trim().to_string();
            }
        }
        out.push(Upgradable {
            name: name.to_string(),
            from,
            to,
        });
        if out.len() >= 200 {
            break;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutating_ops_need_manage() {
        assert!(!SysOp::Status.mutating());
        assert!(!SysOp::UpdatesList.mutating());
        assert!(SysOp::UpdatesApply.mutating());
        assert!(SysOp::NetSet.mutating());
        assert_eq!(SysOp::Status.permission(), Permission::SysView);
        assert_eq!(SysOp::NetSet.permission(), Permission::SysManage);
        assert!(SysOp::UpdatesApply.streams());
        assert!(!SysOp::Status.streams());
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
    }
}
