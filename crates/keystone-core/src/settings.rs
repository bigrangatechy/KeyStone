// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};

/// Per-node UI settings. Stored as JSON on the node row. Empty fields mean
/// “use the default” so a future editor can add keys without breaking old rows.
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
        }
    }
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
}
