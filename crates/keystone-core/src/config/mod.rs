// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

mod agent;
mod server;

pub use agent::{AgentConfig, DockerConfig};
pub use server::{PrometheusScrape, ServerAuth, ServerConfig, SnmpScrape};

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
    }
}
