// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};

/// Per-node UI settings. Stored as JSON on the node row. Empty fields mean
/// “use the default” so a future editor can add keys without breaking old rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSettings {
    /// Shown instead of the agent hostname when set.
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub notes: String,
    /// Interface names included in network widgets. Empty = automatic.
    #[serde(default)]
    pub network_devices: Vec<String>,
}

impl NodeSettings {
    pub fn parse_or_default(json: Option<&str>) -> Self {
        let Some(raw) = json.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::default();
        };
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn display_host<'a>(&'a self, hostname: &'a str) -> &'a str {
        if self.display_name.trim().is_empty() {
            hostname
        } else {
            self.display_name.trim()
        }
    }
}
