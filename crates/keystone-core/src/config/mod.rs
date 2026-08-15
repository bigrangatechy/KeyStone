// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

mod agent;
mod server;

pub use agent::{AgentConfig, DockerConfig};
pub use server::{PrometheusScrape, ServerAuth, ServerConfig, SnmpScrape, TlsConfig};

use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("read config {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parse config {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
}

pub fn load_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.display().to_string(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// Host used for TLS SNI. `ingest_url` must be `http://` or `https://`.
pub fn ingest_tls_domain(ingest_url: &str) -> Result<String, String> {
    let rest = ingest_url
        .strip_prefix("https://")
        .or_else(|| ingest_url.strip_prefix("http://"))
        .ok_or_else(|| "ingest_url must start with http:// or https://".to_string())?;
    let hostport = rest.split('/').next().unwrap_or(rest);
    let host = if let Some(rest) = hostport.strip_prefix('[') {
        rest.split(']')
            .next()
            .ok_or_else(|| "ingest_url IPv6 host is missing ']'".to_string())?
            .to_string()
    } else {
        hostport.split(':').next().unwrap_or(hostport).to_string()
    };
    if host.is_empty() {
        Err("ingest_url has no host".into())
    } else {
        Ok(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_agent_toml_parses() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/agent.toml");
        let cfg: AgentConfig = load_toml(&path).expect("agent.toml");
        assert!(!cfg.ingest_url.is_empty());
        assert!(!cfg.docker.manage);
        assert!(!cfg.docker.allow_exec);
    }

    #[test]
    fn example_server_toml_parses() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/server.toml");
        let cfg: ServerConfig = load_toml(&path).expect("server.toml");
        assert!(cfg.http_listen.contains(':'));
        assert!(cfg.grpc_listen.contains(':'));
        assert!(!cfg.tls.ui_https());
        assert!(cfg.tls.pem_paths().unwrap().is_none());
    }

    #[test]
    fn packaged_tomls_parse() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let server: ServerConfig =
            load_toml(&root.join("packaging/deb/server/server.toml")).expect("packaged server");
        assert!(!server.tls.ui_https());
        let agent: AgentConfig =
            load_toml(&root.join("packaging/deb/agent/agent.toml")).expect("packaged agent");
        assert!(crate::wants_mdns(&agent.ingest_url));
        assert_eq!(agent.ingest_token, "change-me");
        assert!(agent.tls_ca_file.is_empty());
    }

    #[test]
    fn tls_section_and_agent_https() {
        let cfg: ServerConfig = toml::from_str(
            r#"
            [tls]
            cert_file = "/etc/keystone/tls/fullchain.pem"
            key_file = "/etc/keystone/tls/privkey.pem"
            "#,
        )
        .unwrap();
        assert!(cfg.tls.ui_https());
        assert!(cfg.tls.ingest_https());
        let off: ServerConfig = toml::from_str(
            r#"
            [tls]
            cert_file = "/c.pem"
            key_file = "/k.pem"
            ingest = false
            "#,
        )
        .unwrap();
        assert!(off.tls.ui_https());
        assert!(!off.tls.ingest_https());
        assert!(TlsConfig {
            cert_file: "/c.pem".into(),
            key_file: String::new(),
            ingest: true,
        }
        .pem_paths()
        .is_err());
        let agent: AgentConfig = toml::from_str(
            r#"
            ingest_url = "https://keystone.home.arpa:9100"
            tls_ca_file = "/etc/keystone/ca.pem"
            buffer_dir = "/tmp"
            "#,
        )
        .unwrap();
        assert_eq!(
            ingest_tls_domain(&agent.ingest_url).unwrap(),
            "keystone.home.arpa"
        );
        assert_eq!(agent.tls_ca_file, "/etc/keystone/ca.pem");
    }

    #[test]
    fn ingest_tls_domain_parses_hosts() {
        assert_eq!(
            ingest_tls_domain("https://keystone.home.arpa:9100").unwrap(),
            "keystone.home.arpa"
        );
        assert_eq!(ingest_tls_domain("https://[::1]:9100").unwrap(), "::1");
        assert_eq!(
            ingest_tls_domain("http://127.0.0.1:9100").unwrap(),
            "127.0.0.1"
        );
        assert!(ingest_tls_domain("keystone.home.arpa:9100").is_err());
    }
}
