// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{PrometheusScrape, ServerConfig, SnmpScrape};

/// Per-node UI settings. Stored as JSON on the node row. Empty fields mean
/// “use the default” so a future editor can add keys without breaking old rows.
///
/// Poll interval, Docker flags, labels, and compose paths are pushed to a
/// connected agent (`set_runtime`). `agent.toml` is the fallback until then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSettings {
    /// Shown instead of the agent hostname when set.
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub notes: String,
    /// Interface names included in network widgets. Empty = automatic.
    #[serde(default)]
    pub network_devices: Vec<String>,
    /// Agent push and UI refresh interval in seconds (1–60). Default 1.
    #[serde(default = "default_poll_secs")]
    pub poll_secs: u32,
    /// Observe Docker via the engine socket on this node.
    #[serde(default)]
    pub docker_enabled: bool,
    /// Allow mutating Docker operations. Opt-in.
    #[serde(default)]
    pub docker_manage: bool,
    /// Allow `docker exec`. Default false.
    #[serde(default)]
    pub docker_allow_exec: bool,
    /// Extra Compose file paths the agent may pass to `docker compose -f`.
    #[serde(default)]
    pub compose_paths: Vec<String>,
    /// Extra labels attached to every heartbeat (`key=value`).
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Observe host system-admin (apt list, addressing) via the opt-in helper.
    #[serde(default)]
    pub sys_enabled: bool,
    /// Allow applying apt upgrades and setting IPv4.
    #[serde(default)]
    pub sys_manage: bool,
}

fn default_poll_secs() -> u32 {
    1
}

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            display_name: String::new(),
            notes: String::new(),
            network_devices: Vec::new(),
            poll_secs: default_poll_secs(),
            docker_enabled: false,
            docker_manage: false,
            docker_allow_exec: false,
            compose_paths: Vec::new(),
            labels: BTreeMap::new(),
            sys_enabled: false,
            sys_manage: false,
        }
    }
}

/// Payload the server sends so a connected agent applies node Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRuntime {
    pub interval_secs: u64,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub docker_enabled: bool,
    #[serde(default)]
    pub docker_manage: bool,
    #[serde(default)]
    pub docker_allow_exec: bool,
    #[serde(default)]
    pub compose_paths: Vec<String>,
    #[serde(default)]
    pub sys_enabled: bool,
    #[serde(default)]
    pub sys_manage: bool,
}

impl NodeSettings {
    pub const POLL_SECS_MIN: u32 = 1;
    pub const POLL_SECS_MAX: u32 = 60;

    pub fn parse_or_default(json: Option<&str>) -> Self {
        let Some(raw) = json.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::default();
        };
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn clamp_poll_secs(secs: u32) -> u32 {
        secs.clamp(Self::POLL_SECS_MIN, Self::POLL_SECS_MAX)
    }

    /// Seconds the agent should push and the UI should refresh.
    pub fn poll_interval_secs(&self) -> u64 {
        Self::clamp_poll_secs(self.poll_secs) as u64
    }

    pub fn display_host<'a>(&'a self, hostname: &'a str) -> &'a str {
        if self.display_name.trim().is_empty() {
            hostname
        } else {
            self.display_name.trim()
        }
    }

    pub fn agent_runtime(&self) -> AgentRuntime {
        AgentRuntime {
            interval_secs: self.poll_interval_secs(),
            labels: self.labels.clone(),
            docker_enabled: self.docker_enabled,
            docker_manage: self.docker_manage,
            docker_allow_exec: self.docker_allow_exec,
            compose_paths: self.compose_paths.clone(),
            sys_enabled: self.sys_enabled,
            sys_manage: self.sys_enabled && self.sys_manage,
        }
    }

    pub fn agent_runtime_json(&self) -> String {
        serde_json::to_string(&self.agent_runtime()).unwrap_or_else(|_| "{}".into())
    }

    pub fn labels_text(&self) -> String {
        self.labels
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn parse_labels(text: &str) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            if !k.is_empty() {
                out.insert(k.to_string(), v.trim().to_string());
            }
        }
        out
    }

    pub fn parse_lines(text: &str) -> Vec<String> {
        text.lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .map(str::to_string)
            .collect()
    }
}

/// Operator settings stored in SQLite after first start. TOML values are the
/// seed (and development fallback) until this row exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerSettings {
    /// Series retention in hours (1–8760). Default 24.
    #[serde(default = "default_retention_hours")]
    pub retention_hours: u32,
    /// Shared ingest token. Agents must present this. Empty allows any token.
    #[serde(default)]
    pub ingest_token: String,
    #[serde(default)]
    pub prometheus_scrape: Vec<PrometheusScrape>,
    #[serde(default)]
    pub snmp_scrape: Vec<SnmpScrape>,
    /// Optional HTTP POST on alert transitions. Empty = off.
    #[serde(default)]
    pub alert_webhook_url: String,
}

fn default_retention_hours() -> u32 {
    24
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            retention_hours: default_retention_hours(),
            ingest_token: String::new(),
            prometheus_scrape: Vec::new(),
            snmp_scrape: Vec::new(),
            alert_webhook_url: String::new(),
        }
    }
}

impl ServerSettings {
    pub const RETENTION_HOURS_MIN: u32 = 1;
    pub const RETENTION_HOURS_MAX: u32 = 24 * 365;
    pub const KV_KEY: &'static str = "server";

    pub fn parse_or_default(json: Option<&str>) -> Self {
        let Some(raw) = json.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::default();
        };
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn from_config(cfg: &ServerConfig) -> Self {
        Self {
            retention_hours: Self::clamp_retention_hours(cfg.retention_hours),
            ingest_token: cfg.ingest_token.clone(),
            prometheus_scrape: cfg.prometheus_scrape.clone(),
            snmp_scrape: cfg.snmp_scrape.clone(),
            alert_webhook_url: String::new(),
        }
    }

    /// Empty is off. Otherwise `http://` or `https://` with no whitespace.
    pub fn parse_webhook_url(raw: &str) -> Result<String, String> {
        let t = raw.trim();
        if t.is_empty() {
            return Ok(String::new());
        }
        let lower = t.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Err("alert webhook URL must start with http:// or https://".into());
        }
        if t.chars().any(char::is_whitespace) {
            return Err("alert webhook URL must not contain spaces".into());
        }
        Ok(t.to_string())
    }

    pub fn clamp_retention_hours(hours: u32) -> u32 {
        hours.clamp(Self::RETENTION_HOURS_MIN, Self::RETENTION_HOURS_MAX)
    }

    pub fn format_prometheus_lines(jobs: &[PrometheusScrape]) -> String {
        jobs.iter()
            .map(|j| {
                format!(
                    "{} | {} | {} | {}",
                    j.name, j.url, j.interval_secs, j.node_id
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn format_snmp_lines(jobs: &[SnmpScrape]) -> String {
        jobs.iter()
            .map(|j| {
                format!(
                    "{} | {} | {} | {} | {}",
                    j.name, j.target, j.community, j.interval_secs, j.node_id
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `name | url | interval_secs | node_id` — interval and node_id optional.
    pub fn parse_prometheus_lines(text: &str) -> Result<Vec<PrometheusScrape>, String> {
        let mut jobs = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
                return Err(format!(
                    "prometheus line {}: expected `name | url | interval_secs | node_id`",
                    i + 1
                ));
            }
            let interval_secs = parse_optional_u64(parts.get(2).copied().unwrap_or(""), 30)
                .map_err(|e| format!("prometheus line {}: {e}", i + 1))?;
            jobs.push(PrometheusScrape {
                name: parts[0].to_string(),
                url: parts[1].to_string(),
                interval_secs,
                node_id: parts.get(3).unwrap_or(&"").to_string(),
            });
        }
        Ok(jobs)
    }

    /// `name | target | community | interval_secs | node_id` — community,
    /// interval, and node_id optional.
    pub fn parse_snmp_lines(text: &str) -> Result<Vec<SnmpScrape>, String> {
        let mut jobs = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
                return Err(format!(
                    "snmp line {}: expected `name | target | community | interval_secs | node_id`",
                    i + 1
                ));
            }
            let community = {
                let c = parts.get(2).copied().unwrap_or("");
                if c.is_empty() {
                    "public".into()
                } else {
                    c.to_string()
                }
            };
            let interval_secs = parse_optional_u64(parts.get(3).copied().unwrap_or(""), 30)
                .map_err(|e| format!("snmp line {}: {e}", i + 1))?;
            jobs.push(SnmpScrape {
                name: parts[0].to_string(),
                target: parts[1].to_string(),
                community,
                interval_secs,
                node_id: parts.get(4).unwrap_or(&"").to_string(),
            });
        }
        Ok(jobs)
    }
}

fn parse_optional_u64(raw: &str, default: u64) -> Result<u64, String> {
    if raw.is_empty() {
        return Ok(default);
    }
    raw.parse::<u64>()
        .map(|n| n.max(5))
        .map_err(|_| format!("invalid interval `{raw}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_poll_secs_defaults_to_one() {
        let s = NodeSettings::parse_or_default(Some("{}"));
        assert_eq!(s.poll_interval_secs(), 1);
        assert_eq!(NodeSettings::default().poll_interval_secs(), 1);
        assert_eq!(NodeSettings::parse_or_default(None).poll_interval_secs(), 1);
        assert!(!s.docker_enabled);
        assert!(s.labels.is_empty());
    }

    #[test]
    fn poll_secs_clamps() {
        let lo = NodeSettings {
            poll_secs: 0,
            ..Default::default()
        };
        assert_eq!(lo.poll_interval_secs(), 1);
        let hi = NodeSettings {
            poll_secs: 999,
            ..Default::default()
        };
        assert_eq!(hi.poll_interval_secs(), 60);
        assert_eq!(NodeSettings::clamp_poll_secs(5), 5);
    }

    #[test]
    fn labels_round_trip() {
        let text = "role=homelab\n# skip\n env = prod \n";
        let map = NodeSettings::parse_labels(text);
        assert_eq!(map.get("role").map(String::as_str), Some("homelab"));
        assert_eq!(map.get("env").map(String::as_str), Some("prod"));
        let s = NodeSettings {
            labels: map,
            ..Default::default()
        };
        assert!(s.labels_text().contains("role=homelab"));
    }

    #[test]
    fn server_settings_default_retention_is_24h() {
        let s = ServerSettings::parse_or_default(Some("{}"));
        assert_eq!(s.retention_hours, 24);
        assert_eq!(s.alert_webhook_url, "");
        assert_eq!(ServerSettings::clamp_retention_hours(0), 1);
        assert_eq!(ServerSettings::clamp_retention_hours(99_000), 24 * 365);
    }

    #[test]
    fn missing_webhook_field_defaults_empty() {
        let s =
            ServerSettings::parse_or_default(Some(r#"{"retention_hours":48,"ingest_token":"x"}"#));
        assert_eq!(s.retention_hours, 48);
        assert_eq!(s.alert_webhook_url, "");
    }

    #[test]
    fn webhook_url_must_be_http() {
        assert_eq!(ServerSettings::parse_webhook_url("  ").unwrap(), "");
        assert_eq!(
            ServerSettings::parse_webhook_url("https://hooks.example/ks").unwrap(),
            "https://hooks.example/ks"
        );
        assert!(ServerSettings::parse_webhook_url("javascript:alert(1)").is_err());
        assert!(ServerSettings::parse_webhook_url("file:///etc/passwd").is_err());
        assert!(ServerSettings::parse_webhook_url("https://ex ample").is_err());
    }

    #[test]
    fn prometheus_lines_parse() {
        let jobs = ServerSettings::parse_prometheus_lines(
            "# comment\nexporter | http://127.0.0.1:9100/metrics | 15 | box\n",
        )
        .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "exporter");
        assert_eq!(jobs[0].interval_secs, 15);
        assert_eq!(jobs[0].node_id, "box");
        let text = ServerSettings::format_prometheus_lines(&jobs);
        let again = ServerSettings::parse_prometheus_lines(&text).unwrap();
        assert_eq!(jobs, again);
    }

    #[test]
    fn snmp_lines_parse() {
        let jobs = ServerSettings::parse_snmp_lines("sw | 192.0.2.1 | private | 60 |\n").unwrap();
        assert_eq!(jobs[0].community, "private");
        assert_eq!(jobs[0].interval_secs, 60);
        let text = ServerSettings::format_snmp_lines(&jobs);
        assert_eq!(ServerSettings::parse_snmp_lines(&text).unwrap(), jobs);
    }

    #[test]
    fn agent_runtime_json_includes_docker() {
        let s = NodeSettings {
            docker_enabled: true,
            docker_manage: true,
            poll_secs: 2,
            ..Default::default()
        };
        let rt: AgentRuntime = serde_json::from_str(&s.agent_runtime_json()).unwrap();
        assert!(rt.docker_enabled);
        assert!(rt.docker_manage);
        assert!(!rt.sys_enabled);
        assert_eq!(rt.interval_secs, 2);
    }

    #[test]
    fn agent_runtime_sys_manage_requires_observe() {
        let off = NodeSettings {
            sys_manage: true,
            ..Default::default()
        };
        assert!(!off.agent_runtime().sys_enabled);
        assert!(!off.agent_runtime().sys_manage);
        let on = NodeSettings {
            sys_enabled: true,
            sys_manage: true,
            ..Default::default()
        };
        assert!(on.agent_runtime().sys_enabled);
        assert!(on.agent_runtime().sys_manage);
    }
}
