// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// UI/API permissions. The ingest token is not a permission and cannot
/// grant any of these.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    EnumIter,
    IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum Permission {
    /// View node list, heartbeat, and host metrics.
    NodesView,
    /// List Docker objects on a node (read-only).
    DockerView,
    /// Mutate containers, Compose, images, volumes, and networks.
    DockerManage,
    /// `docker exec` into a container (off by default on the agent).
    DockerExec,
    /// View host system-admin snapshot (apt list, journals, NTP, unattended-upgrades, addresses).
    SysView,
    /// Apply apt upgrades, autoremove, reboot, and set IPv4 (opt-in root helper).
    SysManage,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::NodesView => "View node list, heartbeat, and host metrics",
            Self::DockerView => "List and inspect Docker objects on a node",
            Self::DockerManage => {
                "Start/stop/remove containers, Compose, images, volumes, and networks"
            }
            Self::DockerExec => "Execute a process inside a container (root-equivalent)",
            Self::SysView => {
                "View host updates, journals, NTP, unattended-upgrades, and addressing on a node"
            }
            Self::SysManage => "Apply apt upgrades, autoremove, reboot, and set IPv4 on a node",
        }
    }

    /// First-slice admin role: every permission.
    pub fn admin_all() -> &'static [Permission] {
        &[
            Self::NodesView,
            Self::DockerView,
            Self::DockerManage,
            Self::DockerExec,
            Self::SysView,
            Self::SysManage,
        ]
    }
}
