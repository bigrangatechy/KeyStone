// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! `/etc/os-release` family for one tree across headless Linux servers.
//!
//! This is observe + packaging contract, not a per-distro application fork.
//! Apt mutations stay Debian-family. Fedora `dnf` and openSUSE `zypper`
//! apply are later (see `docs/dev/src/os-support.md`). Tests parse fixture
//! strings; they must not require a live `/etc/os-release`.

use serde_json::{json, Value};

/// Where this host sits for packaging and System Apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsFamily {
    Debian,
    Fedora,
    Suse,
    Other,
}

impl OsFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debian => "debian",
            Self::Fedora => "fedora",
            Self::Suse => "suse",
            Self::Other => "other",
        }
    }
}

/// Native package manager for this family. Apply uses this, not "is apt in PATH".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Apt,
    Dnf,
    Zypper,
    Unknown,
}

impl PackageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Zypper => "zypper",
            Self::Unknown => "unknown",
        }
    }

    fn from_family(family: OsFamily) -> Self {
        match family {
            OsFamily::Debian => Self::Apt,
            OsFamily::Fedora => Self::Dnf,
            OsFamily::Suse => Self::Zypper,
            OsFamily::Other => Self::Unknown,
        }
    }
}

/// Parsed `os-release` fields the agent and helper already send in `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostOs {
    pub id: String,
    pub id_like: Vec<String>,
    pub version_id: String,
    pub pretty_name: String,
    pub family: OsFamily,
    pub package: PackageKind,
}

impl HostOs {
    /// Read `/etc/os-release`, then `/usr/lib/os-release`. Empty file → Other.
    pub fn from_host() -> Self {
        let text = std::fs::read_to_string("/etc/os-release")
            .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
            .unwrap_or_default();
        Self::parse(&text)
    }

    /// Parse an os-release document. Fixtures in tests; no live file required.
    pub fn parse(text: &str) -> Self {
        let map = parse_os_release_map(text);
        let id = map.get("ID").cloned().unwrap_or_default();
        let id_like: Vec<String> = map
            .get("ID_LIKE")
            .map(|s| s.split_whitespace().map(|t| t.to_string()).collect())
            .unwrap_or_default();
        let version_id = map.get("VERSION_ID").cloned().unwrap_or_default();
        let pretty_name = map.get("PRETTY_NAME").cloned().unwrap_or_default();
        let family = family_from(&id, &id_like);
        let package = PackageKind::from_family(family);
        Self {
            id,
            id_like,
            version_id,
            pretty_name,
            family,
            package,
        }
    }

    pub fn apt_sys_manage(&self) -> bool {
        self.package == PackageKind::Apt
    }

    /// Apt Apply / list / autoremove / unattended-upgrades. Not dnf/zypper.
    pub fn require_apt(&self) -> Result<(), String> {
        if self.apt_sys_manage() {
            return Ok(());
        }
        let name = if self.pretty_name.is_empty() {
            if self.id.is_empty() {
                self.family.as_str()
            } else {
                self.id.as_str()
            }
        } else {
            self.pretty_name.as_str()
        };
        Err(format!(
            "System Apply is apt on this version ({name}). Fedora dnf and openSUSE zypper are later."
        ))
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "version_id": self.version_id,
            "pretty": self.pretty_name,
            "family": self.family.as_str(),
            "package": self.package.as_str(),
        })
    }
}

fn parse_os_release_map(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        map.insert(key.to_string(), unquote(raw.trim()));
    }
    map
}

fn unquote(raw: &str) -> String {
    let b = raw.as_bytes();
    if b.len() >= 2 {
        let (first, last) = (b[0], b[b.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return raw[1..raw.len() - 1].to_string();
        }
    }
    raw.to_string()
}

fn family_from(id: &str, id_like: &[String]) -> OsFamily {
    let id_l = id.to_ascii_lowercase();
    if debian_id(&id_l) {
        return OsFamily::Debian;
    }
    if fedora_id(&id_l) {
        return OsFamily::Fedora;
    }
    if suse_id(&id_l) {
        return OsFamily::Suse;
    }
    for token in id_like {
        let t = token.to_ascii_lowercase();
        if debian_like(&t) {
            return OsFamily::Debian;
        }
        if fedora_like(&t) {
            return OsFamily::Fedora;
        }
        if suse_like(&t) {
            return OsFamily::Suse;
        }
    }
    OsFamily::Other
}

fn debian_id(id: &str) -> bool {
    matches!(
        id,
        "debian"
            | "ubuntu"
            | "raspbian"
            | "raspios"
            | "linuxmint"
            | "pop"
            | "elementary"
            | "zorin"
            | "neon"
            | "kali"
            | "devuan"
    )
}

fn fedora_id(id: &str) -> bool {
    matches!(
        id,
        "fedora"
            | "rhel"
            | "centos"
            | "centos-stream"
            | "rocky"
            | "almalinux"
            | "ol"
            | "amzn"
            | "nobara"
    )
}

fn suse_id(id: &str) -> bool {
    matches!(
        id,
        "opensuse"
            | "opensuse-leap"
            | "opensuse-tumbleweed"
            | "opensuse-slowroll"
            | "sles"
            | "sled"
            | "sle-micro"
            | "microos"
    )
}

fn debian_like(token: &str) -> bool {
    token == "debian" || token == "ubuntu"
}

fn fedora_like(token: &str) -> bool {
    matches!(token, "fedora" | "rhel" | "centos" | "fedora-asahi-remix")
}

fn suse_like(token: &str) -> bool {
    matches!(token, "suse" | "opensuse" | "sles")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ubuntu_is_debian_apt() {
        let os = HostOs::parse(
            r#"
PRETTY_NAME="Ubuntu 26.04 LTS"
NAME="Ubuntu"
ID=ubuntu
ID_LIKE=debian
VERSION_ID="26.04"
"#,
        );
        assert_eq!(os.id, "ubuntu");
        assert_eq!(os.version_id, "26.04");
        assert_eq!(os.pretty_name, "Ubuntu 26.04 LTS");
        assert_eq!(os.family, OsFamily::Debian);
        assert_eq!(os.package, PackageKind::Apt);
        assert!(os.apt_sys_manage());
        assert!(os.require_apt().is_ok());
        assert_eq!(os.to_json()["package"], json!("apt"));
        assert_eq!(os.to_json()["family"], json!("debian"));
    }

    #[test]
    fn raspberry_pi_os_is_debian_apt() {
        let os = HostOs::parse("PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian\n");
        assert_eq!(os.family, OsFamily::Debian);
        assert_eq!(os.package, PackageKind::Apt);
    }

    #[test]
    fn fedora_server_is_dnf_not_apt() {
        let os = HostOs::parse(
            r#"
NAME="Fedora Linux"
VERSION="42 (Server Edition)"
ID=fedora
VERSION_ID=42
PRETTY_NAME="Fedora Linux 42 (Server Edition)"
"#,
        );
        assert_eq!(os.id, "fedora");
        assert_eq!(os.version_id, "42");
        assert_eq!(os.family, OsFamily::Fedora);
        assert_eq!(os.package, PackageKind::Dnf);
        assert!(!os.apt_sys_manage());
        let err = os.require_apt().unwrap_err();
        assert!(err.contains("apt on this version"), "{err}");
        assert!(err.contains("Fedora dnf"), "{err}");
        assert!(err.contains("zypper are later"), "{err}");
    }

    #[test]
    fn rocky_id_like_is_fedora_dnf() {
        let os = HostOs::parse("ID=rocky\nID_LIKE=\"rhel centos fedora\"\n");
        assert_eq!(os.family, OsFamily::Fedora);
        assert_eq!(os.package, PackageKind::Dnf);
    }

    #[test]
    fn opensuse_leap_is_zypper() {
        let os = HostOs::parse(
            r#"
ID="opensuse-leap"
ID_LIKE="suse opensuse"
PRETTY_NAME="openSUSE Leap 15.6"
"#,
        );
        assert_eq!(os.family, OsFamily::Suse);
        assert_eq!(os.package, PackageKind::Zypper);
        assert!(os.require_apt().unwrap_err().contains("openSUSE Leap 15.6"));
    }

    #[test]
    fn sles_is_suse() {
        let os = HostOs::parse("ID=sles\nID_LIKE=suse\n");
        assert_eq!(os.family, OsFamily::Suse);
        assert_eq!(os.package, PackageKind::Zypper);
    }

    #[test]
    fn mint_id_is_debian_before_unknown_like() {
        let os = HostOs::parse("ID=linuxmint\nID_LIKE=debian\n");
        assert_eq!(os.family, OsFamily::Debian);
    }

    #[test]
    fn empty_os_release_is_other() {
        let os = HostOs::parse("");
        assert_eq!(os.family, OsFamily::Other);
        assert_eq!(os.package, PackageKind::Unknown);
        assert!(!os.apt_sys_manage());
        assert!(os
            .require_apt()
            .unwrap_err()
            .contains("apt on this version"));
    }

    #[test]
    fn comments_and_single_quotes() {
        let os = HostOs::parse("# comment\nID='ubuntu'\n#ID=fedora\n");
        assert_eq!(os.id, "ubuntu");
        assert_eq!(os.family, OsFamily::Debian);
    }

    #[test]
    fn unknown_id_uses_id_like() {
        let os = HostOs::parse("ID=weirdlab\nID_LIKE=debian\n");
        assert_eq!(os.family, OsFamily::Debian);
        let fedora_like = HostOs::parse("ID=weirdlab\nID_LIKE=fedora\n");
        assert_eq!(fedora_like.family, OsFamily::Fedora);
    }

    #[test]
    fn parse_does_not_read_live_file() {
        let src = include_str!("os.rs");
        let parse = src
            .split("pub fn parse(")
            .nth(1)
            .expect("parse")
            .split("pub fn apt_sys_manage")
            .next()
            .expect("parse body");
        assert!(
            !parse.contains("os-release"),
            "parse() must stay fixture-only; from_host() reads the files"
        );
    }
}
