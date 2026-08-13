// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};

use crate::sample::Label;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub node_id: String,
    pub hostname: String,
    pub agent_version: String,
    pub os: String,
    pub kernel: String,
    pub docker_version: Option<String>,
    pub labels: Vec<Label>,
}
