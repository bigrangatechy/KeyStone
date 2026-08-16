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

/// CPU/memory from pushed container series, keyed by short container `id`.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct ContainerUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<f64>,
}

pub fn container_usage_by_id(
    samples: &[crate::Sample],
) -> std::collections::BTreeMap<String, ContainerUsage> {
    let mut out = std::collections::BTreeMap::<String, ContainerUsage>::new();
    for s in samples {
        let Some(id) = s
            .labels
            .iter()
            .find(|l| l.name == "id")
            .map(|l| l.value.as_str())
        else {
            continue;
        };
        let row = out.entry(id.to_string()).or_default();
        match s.metric.as_str() {
            "container_cpu_usage_ratio" => row.cpu_ratio = Some(s.value),
            "container_memory_usage_bytes" => row.memory_bytes = Some(s.value),
            _ => {}
        }
    }
    out.retain(|_, u| u.cpu_ratio.is_some() || u.memory_bytes.is_some());
    out
}

/// Join background container series onto a `container_list` row by short `id`.
pub fn merge_container_usage(list: &mut [serde_json::Value], samples: &[crate::Sample]) {
    let usage = container_usage_by_id(samples);
    for row in list {
        let Some(id) = row.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
            continue;
        };
        if let Some(u) = usage.get(&id) {
            if let Some(v) = u.cpu_ratio {
                row["cpu_ratio"] = serde_json::json!(v);
            }
            if let Some(v) = u.memory_bytes {
                row["memory_bytes"] = serde_json::json!(v);
            }
        }
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

    #[test]
    fn merge_container_usage_matches_short_id_only() {
        let samples = vec![
            crate::Sample::new("container_cpu_usage_ratio", 0.25, 1)
                .with_label("id", "abc123def456")
                .with_label("name", "web"),
            crate::Sample::new("container_memory_usage_bytes", 64.0 * 1024.0 * 1024.0, 1)
                .with_label("id", "abc123def456")
                .with_label("name", "web"),
            crate::Sample::new("node_cpu_usage_ratio", 0.9, 1),
        ];
        let mut list = vec![
            serde_json::json!({"id": "abc123def456", "names": ["/web"]}),
            serde_json::json!({"id": "other0000000", "names": ["/db"]}),
        ];
        merge_container_usage(&mut list, &samples);
        assert_eq!(list[0]["cpu_ratio"], 0.25);
        assert_eq!(list[0]["memory_bytes"], 64.0 * 1024.0 * 1024.0);
        assert!(list[1].get("cpu_ratio").is_none());
        assert!(list[1].get("memory_bytes").is_none());
        let map = container_usage_by_id(&samples);
        assert_eq!(
            map.get("abc123def456").and_then(|u| u.cpu_ratio),
            Some(0.25)
        );
        assert!(!map.contains_key("other0000000"));
    }

    #[test]
    fn mutating_ops_are_in_the_ui_except_reserved_exec() {
        use strum::IntoEnumIterator;
        let js = include_str!("../../keystone-server/src/static/app.js");
        let html = include_str!("../../keystone-server/templates/node.html");
        for op in DockerOp::iter() {
            if !op.mutating() {
                continue;
            }
            let name = op.as_str();
            if op == DockerOp::ContainerExec {
                assert!(
                    !js.contains(name) && !html.contains(name),
                    "container_exec must stay out of the UI"
                );
                continue;
            }
            assert!(
                js.contains(name) || html.contains(&format!("docker/{name}")),
                "mutating {name} must be reachable from the Docker UI"
            );
        }
    }
}
