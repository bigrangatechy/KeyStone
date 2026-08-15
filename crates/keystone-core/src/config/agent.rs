// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};

/// Agent configuration.
///
/// Required to find the server: `ingest_url`, `ingest_token`, `node_id`,
/// `buffer_dir`, and optional `docker.host`. Poll interval, Docker
/// enable/manage/exec, labels, and compose paths are node Settings once
/// connected; TOML is the fallback until then.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// gRPC ingest URL (`http://host:9100` / `https://host:9100`), or
    /// `mdns` to browse `_keystone._tcp.local.` on the LAN.
    pub ingest_url: String,
    /// Shared ingest token. This cannot call Docker manage APIs.
    #[serde(default)]
    pub ingest_token: String,
    /// Stable node id. Defaults to hostname when empty.
    #[serde(default)]
    pub node_id: String,
    /// Extra labels attached to every heartbeat. Fallback until the node's
    /// Settings labels are applied at runtime.
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    /// Push interval in seconds. Default 1. Fallback until the node's
    /// Settings poll interval is applied at runtime.
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    /// Directory for on-disk push buffer when the server is unreachable.
    #[serde(default = "default_buffer")]
    pub buffer_dir: String,
    /// PEM of a private CA (self-signed). Empty = web PKI (Let's Encrypt).
    /// Used only when `ingest_url` is `https://`.
    #[serde(default)]
    pub tls_ca_file: String,
    #[serde(default)]
    pub docker: DockerConfig,
}

fn default_interval() -> u64 {
    1
}

fn default_buffer() -> String {
    "/var/lib/keystone/agent-buffer".into()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            ingest_url: "http://127.0.0.1:9100".into(),
            ingest_token: String::new(),
            node_id: String::new(),
            labels: Default::default(),
            interval_secs: default_interval(),
            buffer_dir: default_buffer(),
            tls_ca_file: String::new(),
            docker: DockerConfig::default(),
        }
    }
}

/// Docker Engine access on this node.
///
/// Socket access is root-equivalent. `manage` is opt-in. `allow_exec` is a
/// further gate because exec is a root shell on the host namespaces.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DockerConfig {
    /// Observe Docker (list/inspect/stats/logs) via the engine socket.
    /// Fallback until node Settings `docker_enabled` is applied.
    #[serde(default)]
    pub enabled: bool,
    /// Allow mutating Docker operations. Opt-in. Fallback until Settings.
    #[serde(default)]
    pub manage: bool,
    /// Allow `docker exec`. Default false. Fallback until Settings.
    #[serde(default)]
    pub allow_exec: bool,
    /// Engine socket or TCP URL. Empty uses `/var/run/docker.sock`.
    /// Stays in this file (not a UI setting).
    #[serde(default)]
    pub host: String,
    /// Extra Compose file paths. Fallback until node Settings apply.
    #[serde(default)]
    pub compose_paths: Vec<String>,
}
