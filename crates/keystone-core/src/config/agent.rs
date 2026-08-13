// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Agent configuration. Source of truth for `docs/src/generated/agent-config.md`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentConfig {
    /// gRPC ingest URL, for example `http://keystone.example:9100`.
    pub ingest_url: String,
    /// Shared ingest token. This cannot call Docker manage APIs.
    #[serde(default)]
    pub ingest_token: String,
    /// Stable node id. Defaults to hostname when empty.
    #[serde(default)]
    pub node_id: String,
    /// Extra labels attached to every sample (`key=value` in TOML map).
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    /// Push interval in seconds.
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    /// Directory for on-disk push buffer when the server is unreachable.
    #[serde(default = "default_buffer")]
    pub buffer_dir: String,
    #[serde(default)]
    pub docker: DockerConfig,
}

fn default_interval() -> u64 {
    15
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
            docker: DockerConfig::default(),
        }
    }
}

/// Docker Engine access on this node.
///
/// Socket access is root-equivalent. `manage` is opt-in. `allow_exec` is a
/// further gate because exec is a root shell on the host namespaces.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DockerConfig {
    /// Observe Docker (list/inspect/stats/logs) via the engine socket.
    #[serde(default)]
    pub enabled: bool,
    /// Allow mutating Docker operations. Opt-in.
    #[serde(default)]
    pub manage: bool,
    /// Allow `docker exec`. Default false.
    #[serde(default)]
    pub allow_exec: bool,
    /// Engine socket or TCP URL. Empty uses `/var/run/docker.sock`.
    #[serde(default)]
    pub host: String,
    /// Extra Compose file paths to manage (in addition to project labels).
    #[serde(default)]
    pub compose_paths: Vec<String>,
}
