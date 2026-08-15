// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

use crate::rbac::Permission;

/// Docker operations the agent may perform. The UI, control RPC, and audit
/// log all use this enum. Keep `docs/dev/src/docker.md` in sync.
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
pub enum DockerOp {
    ContainerList,
    ContainerInspect,
    ContainerStart,
    ContainerStop,
    ContainerRestart,
    ContainerKill,
    ContainerRemove,
    ContainerLogs,
    ContainerStats,
    ContainerExec,
    ComposePs,
    ComposeUp,
    ComposeDown,
    ComposeLogs,
    ComposePull,
    ComposeUpdate,
    ImageList,
    ImageInspect,
    ImagePull,
    ImagePrune,
    ImageRemove,
    VolumeList,
    VolumeInspect,
    VolumeCreate,
    VolumeRemove,
    NetworkList,
    NetworkInspect,
    NetworkCreate,
    NetworkRemove,
}

impl DockerOp {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ContainerList => "List containers",
            Self::ContainerInspect => "Inspect a container",
            Self::ContainerStart => "Start a container",
            Self::ContainerStop => "Stop a container",
            Self::ContainerRestart => "Restart a container",
            Self::ContainerKill => "Kill a container",
            Self::ContainerRemove => "Remove a container",
            Self::ContainerLogs => "Stream container logs (on-demand)",
            Self::ContainerStats => "Stream live container stats (on-demand)",
            Self::ContainerExec => {
                "Exec a command in a container (disabled unless docker.allow_exec)"
            }
            Self::ComposePs => "List Compose project services",
            Self::ComposeUp => "Compose up",
            Self::ComposeDown => "Compose down",
            Self::ComposeLogs => "Compose logs",
            Self::ComposePull => "Compose pull",
            Self::ComposeUpdate => "Compose pull then up",
            Self::ImageList => "List images",
            Self::ImageInspect => "Inspect an image",
            Self::ImagePull => "Pull an image",
            Self::ImagePrune => "Prune unused images",
            Self::ImageRemove => "Remove an image",
            Self::VolumeList => "List volumes",
            Self::VolumeInspect => "Inspect a volume",
            Self::VolumeCreate => "Create a volume",
            Self::VolumeRemove => "Remove a volume",
            Self::NetworkList => "List networks",
            Self::NetworkInspect => "Inspect a network",
            Self::NetworkCreate => "Create a network",
            Self::NetworkRemove => "Remove a network",
        }
    }

    pub fn mutating(self) -> bool {
        matches!(
            self,
            Self::ContainerStart
                | Self::ContainerStop
                | Self::ContainerRestart
                | Self::ContainerKill
                | Self::ContainerRemove
                | Self::ContainerExec
                | Self::ComposeUp
                | Self::ComposeDown
                | Self::ComposePull
                | Self::ComposeUpdate
                | Self::ImagePull
                | Self::ImagePrune
                | Self::ImageRemove
                | Self::VolumeCreate
                | Self::VolumeRemove
                | Self::NetworkCreate
                | Self::NetworkRemove
        )
    }

    pub fn permission(self) -> Permission {
        match self {
            Self::ContainerExec => Permission::DockerExec,
            op if op.mutating() => Permission::DockerManage,
            _ => Permission::DockerView,
        }
    }

    /// Agent sends `StreamChunk`s then a `CommandResult` (logs).
    pub fn streams(self) -> bool {
        matches!(self, Self::ContainerLogs | Self::ComposeLogs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_update_is_mutating_pull_then_up() {
        assert_eq!(DockerOp::ComposeUpdate.as_str(), "compose_update");
        assert!(DockerOp::ComposeUpdate.mutating());
        assert_eq!(
            DockerOp::ComposeUpdate.permission(),
            Permission::DockerManage
        );
        assert!(!DockerOp::ComposeUpdate.streams());
        assert_eq!(
            DockerOp::ComposeUpdate.description(),
            "Compose pull then up"
        );
    }
}
