// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

pub mod buffer;
pub mod cli;
pub mod collect;
pub mod docker;
pub mod mdns;
pub mod session;

pub use cli::AgentCli;

use anyhow::Context;
use keystone_core::config::AgentConfig;
use std::path::Path;

/// Load `agent.toml`. Missing or unreadable is an error — never dial
/// `127.0.0.1` because `/etc/keystone` was `0700`/`0750` root:root.
pub fn load_runtime_config(path: &Path) -> anyhow::Result<AgentConfig> {
    keystone_core::config::load_toml(path).with_context(|| {
        format!(
            "read {} (must exist and be readable by this user; /etc/keystone should be root:keystone mode 0750)",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn packaged_main_does_not_default_ingest() {
        let main = include_str!("main.rs");
        assert!(
            !main.contains("localhost ingest"),
            "missing config must not fall back to 127.0.0.1"
        );
        assert!(
            !main.contains("AgentConfig::default()"),
            "main must load toml, not Default"
        );
    }

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ks-agent-cfg-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_config_is_an_error_not_localhost() {
        let err = load_runtime_config(Path::new("/no/such/keystone-agent.toml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("/no/such/keystone-agent.toml"), "{err}");
        assert!(!err.contains("127.0.0.1"), "{err}");
        assert!(!err.to_ascii_lowercase().contains("localhost"), "{err}");
    }

    #[test]
    fn readable_config_parses() {
        let dir = scratch();
        let path = dir.join("agent.toml");
        fs::write(
            &path,
            "ingest_url = \"http://192.168.0.188:9100\"\ningest_token = \"lab\"\nbuffer_dir = \"/tmp\"\n",
        )
        .unwrap();
        let cfg = load_runtime_config(&path).unwrap();
        assert_eq!(cfg.ingest_url, "http://192.168.0.188:9100");
        assert_eq!(cfg.ingest_token, "lab");
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_config_is_an_error_not_localhost() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        let hidden = dir.join("hidden");
        fs::create_dir(&hidden).unwrap();
        let path = hidden.join("agent.toml");
        fs::write(
            &path,
            "ingest_url = \"http://192.168.0.188:9100\"\nbuffer_dir = \"/tmp\"\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&hidden).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&hidden, perms).unwrap();
        let result = load_runtime_config(&path);
        perms = fs::metadata(&hidden).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hidden, perms).unwrap();
        match result {
            Ok(_) => {
                // euid 0 can traverse mode 000; the packaged agent is not root.
            }
            Err(e) => {
                let err = e.to_string();
                assert!(!err.contains("127.0.0.1"), "{err}");
                assert!(!err.to_ascii_lowercase().contains("localhost"), "{err}");
            }
        }
        let _ = fs::remove_dir_all(dir);
    }
}
