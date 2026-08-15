// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;
use std::default::Default;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context};
use bollard::container::{
    InspectContainerOptions, KillContainerOptions, ListContainersOptions, LogsOptions,
    RemoveContainerOptions, StatsOptions,
};
use bollard::image::{ListImagesOptions, PruneImagesOptions, RemoveImageOptions};
use bollard::network::{CreateNetworkOptions, ListNetworksOptions};
use bollard::volume::{CreateVolumeOptions, ListVolumesOptions};
use bollard::Docker;
use futures_util::StreamExt;
use keystone_core::config::DockerConfig;
use keystone_core::docker::DockerOp;
use keystone_core::sample::Sample;
use serde_json::{json, Value};
use tokio::process::Command;

#[derive(Clone)]
pub struct DockerHandle {
    docker: Docker,
    cfg: Arc<Mutex<DockerConfig>>,
}

impl DockerHandle {
    pub fn connect(cfg: &DockerConfig) -> anyhow::Result<Self> {
        let docker = if cfg.host.is_empty() || cfg.host.starts_with("unix://") {
            let path = if cfg.host.is_empty() {
                "/var/run/docker.sock"
            } else {
                cfg.host.trim_start_matches("unix://")
            };
            Docker::connect_with_unix(path, 120, bollard::API_DEFAULT_VERSION)
                .context("connect docker unix socket")?
        } else {
            Docker::connect_with_http(&cfg.host, 120, bollard::API_DEFAULT_VERSION)
                .context("connect docker http")?
        };
        Ok(Self {
            docker,
            cfg: Arc::new(Mutex::new(cfg.clone())),
        })
    }

    pub fn set_policy(&self, manage: bool, allow_exec: bool, compose_paths: Vec<String>) {
        let mut cfg = self.cfg.lock().unwrap_or_else(|e| e.into_inner());
        cfg.manage = manage;
        cfg.allow_exec = allow_exec;
        cfg.compose_paths = compose_paths;
    }

    fn policy(&self) -> DockerConfig {
        self.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub async fn engine_version(&self) -> Option<String> {
        self.docker.version().await.ok().and_then(|v| v.version)
    }

    pub async fn collect_container_metrics(&self) -> Vec<Sample> {
        let ts = crate::collect::now_ms();
        let mut out = Vec::new();
        let Ok(list) = self
            .docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await
        else {
            return out;
        };
        for c in list {
            let id = c.id.clone().unwrap_or_default();
            let short = id.chars().take(12).collect::<String>();
            let name = c
                .names
                .clone()
                .unwrap_or_default()
                .into_iter()
                .next()
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_string();
            let project = c
                .labels
                .as_ref()
                .and_then(|l| l.get("com.docker.compose.project"))
                .cloned()
                .unwrap_or_default();
            let running = c.state.as_deref() == Some("running");
            out.push(
                Sample::new("container_running", if running { 1.0 } else { 0.0 }, ts)
                    .with_label("id", &short)
                    .with_label("name", &name)
                    .with_label("compose_project", &project),
            );
            if running {
                let mut stream = self.docker.stats(
                    &id,
                    Some(StatsOptions {
                        stream: false,
                        one_shot: true,
                    }),
                );
                if let Some(Ok(s)) = stream.next().await {
                    let mem = s.memory_stats.usage.unwrap_or(0) as f64;
                    out.push(
                        Sample::new("container_memory_usage_bytes", mem, ts)
                            .with_label("id", &short)
                            .with_label("name", &name)
                            .with_label("compose_project", &project),
                    );
                    let cpu = cpu_ratio(&s);
                    out.push(
                        Sample::new("container_cpu_usage_ratio", cpu, ts)
                            .with_label("id", &short)
                            .with_label("name", &name)
                            .with_label("compose_project", &project),
                    );
                }
            }
        }
        out
    }

    pub async fn execute(&self, op: DockerOp, payload: Value) -> anyhow::Result<Value> {
        let policy = self.policy();
        if op.mutating() && !policy.manage {
            anyhow::bail!("docker.manage is disabled on this agent");
        }
        if op == DockerOp::ContainerExec && !policy.allow_exec {
            anyhow::bail!("docker.allow_exec is disabled on this agent");
        }
        match op {
            DockerOp::ContainerList => self.container_list().await,
            DockerOp::ContainerInspect => {
                let id = str_field(&payload, "id")?;
                let info = self
                    .docker
                    .inspect_container(id, None::<InspectContainerOptions>)
                    .await?;
                Ok(serde_json::to_value(info)?)
            }
            DockerOp::ContainerStart => {
                let id = str_field(&payload, "id")?;
                self.docker
                    .start_container(
                        id,
                        None::<bollard::container::StartContainerOptions<String>>,
                    )
                    .await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::ContainerStop => {
                let id = str_field(&payload, "id")?;
                self.docker.stop_container(id, None).await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::ContainerRestart => {
                let id = str_field(&payload, "id")?;
                self.docker.restart_container(id, None).await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::ContainerKill => {
                let id = str_field(&payload, "id")?;
                self.docker
                    .kill_container(id, None::<KillContainerOptions<String>>)
                    .await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::ContainerRemove => {
                let id = str_field(&payload, "id")?;
                self.docker
                    .remove_container(
                        id,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::ContainerLogs | DockerOp::ComposeLogs => {
                Err(anyhow!("{} must be streamed (StreamChunk)", op.as_str()))
            }
            DockerOp::ContainerStats => {
                let id = str_field(&payload, "id")?;
                let mut stream = self.docker.stats(
                    id,
                    Some(StatsOptions {
                        stream: false,
                        one_shot: true,
                    }),
                );
                let stats = stream.next().await.ok_or_else(|| anyhow!("no stats"))??;
                Ok(serde_json::to_value(stats)?)
            }
            DockerOp::ContainerExec => Err(anyhow!(
                "interactive exec is not exposed over this RPC; enable a future streaming exec"
            )),
            DockerOp::ImageList => self.image_list().await,
            DockerOp::ImageInspect => {
                let name = str_field(&payload, "name")?;
                let info = self.docker.inspect_image(name).await?;
                Ok(serde_json::to_value(info)?)
            }
            DockerOp::ImagePull => {
                let name = str_field(&payload, "name")?;
                let mut stream = self.docker.create_image(
                    Some(bollard::image::CreateImageOptions {
                        from_image: name,
                        ..Default::default()
                    }),
                    None,
                    None,
                );
                while let Some(item) = stream.next().await {
                    item?;
                }
                Ok(json!({"ok": true}))
            }
            DockerOp::ImagePrune => {
                let report = self
                    .docker
                    .prune_images(None::<PruneImagesOptions<String>>)
                    .await?;
                Ok(serde_json::to_value(report)?)
            }
            DockerOp::ImageRemove => {
                let name = str_field(&payload, "name")?;
                let r = self
                    .docker
                    .remove_image(name, None::<RemoveImageOptions>, None)
                    .await?;
                Ok(serde_json::to_value(r)?)
            }
            DockerOp::VolumeList => self.volume_list().await,
            DockerOp::VolumeInspect => {
                let name = str_field(&payload, "name")?;
                let v = self.docker.inspect_volume(name).await?;
                Ok(serde_json::to_value(v)?)
            }
            DockerOp::VolumeCreate => {
                let name = str_field(&payload, "name")?;
                let v = self
                    .docker
                    .create_volume(CreateVolumeOptions {
                        name: name.to_string(),
                        ..Default::default()
                    })
                    .await?;
                Ok(serde_json::to_value(v)?)
            }
            DockerOp::VolumeRemove => {
                let name = str_field(&payload, "name")?;
                self.docker.remove_volume(name, None).await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::NetworkList => self.network_list().await,
            DockerOp::NetworkInspect => {
                let id = str_field(&payload, "id")?;
                let n = self.docker.inspect_network::<String>(id, None).await?;
                Ok(serde_json::to_value(n)?)
            }
            DockerOp::NetworkCreate => {
                let name = str_field(&payload, "name")?;
                let n = self
                    .docker
                    .create_network(CreateNetworkOptions {
                        name: name.to_string(),
                        ..Default::default()
                    })
                    .await?;
                Ok(serde_json::to_value(n)?)
            }
            DockerOp::NetworkRemove => {
                let id = str_field(&payload, "id")?;
                self.docker.remove_network(id).await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::ComposePs
            | DockerOp::ComposeUp
            | DockerOp::ComposeDown
            | DockerOp::ComposePull
            | DockerOp::ComposeUpdate => self.compose(op, &payload).await,
        }
    }

    async fn container_list(&self) -> anyhow::Result<Value> {
        let list = self
            .docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await?;
        let rows: Vec<Value> = list
            .into_iter()
            .map(|c| {
                let id = c.id.clone().unwrap_or_default();
                let labels = c.labels.unwrap_or_default();
                let compose_project = labels.get("com.docker.compose.project").cloned();
                json!({
                    "id": id.chars().take(12).collect::<String>(),
                    "id_full": id,
                    "names": c.names.unwrap_or_default(),
                    "image": c.image,
                    "state": c.state,
                    "status": c.status,
                    "labels": labels,
                    "compose_project": compose_project,
                })
            })
            .collect();
        Ok(json!(rows))
    }

    async fn compose(&self, op: DockerOp, payload: &Value) -> anyhow::Result<Value> {
        let project = payload
            .get("project")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let file = payload.get("file").and_then(|v| v.as_str());
        let paths = self.policy().compose_paths;
        if matches!(op, DockerOp::ComposePs) && file.is_none() {
            return self.compose_ps_from_labels(project).await;
        }
        let mut args = vec!["compose".to_string()];
        if let Some(f) = file {
            args.push("-f".into());
            args.push(f.into());
        } else if let Some(f) = paths.first() {
            args.push("-f".into());
            args.push(f.clone());
        }
        if !project.is_empty() {
            args.push("-p".into());
            args.push(project.into());
        }
        match op {
            DockerOp::ComposeUp => {
                args.push("up".into());
                args.push("-d".into());
            }
            DockerOp::ComposeDown => args.push("down".into()),
            DockerOp::ComposePs => args.push("ps".into()),
            DockerOp::ComposePull => args.push("pull".into()),
            DockerOp::ComposeUpdate => {
                let mut pull = args.clone();
                pull.push("pull".into());
                let output = Command::new("docker")
                    .args(&pull)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .await
                    .context("docker compose pull")?;
                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                if !output.status.success() {
                    anyhow::bail!("docker compose pull failed: {stderr}{stdout}");
                }
                args.push("up".into());
                args.push("-d".into());
            }
            _ => unreachable!(),
        }
        let output = Command::new("docker")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("docker compose")?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !output.status.success() {
            anyhow::bail!("docker compose failed: {stderr}{stdout}");
        }
        Ok(json!({"stdout": stdout, "stderr": stderr}))
    }

    async fn compose_ps_from_labels(&self, project_filter: &str) -> anyhow::Result<Value> {
        let list = self
            .docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await?;
        let mut projects: HashMap<String, Vec<Value>> = HashMap::new();
        for c in list {
            let labels = c.labels.clone().unwrap_or_default();
            let Some(project) = labels.get("com.docker.compose.project").cloned() else {
                continue;
            };
            if !project_filter.is_empty() && project != project_filter {
                continue;
            }
            let id = c.id.clone().unwrap_or_default();
            let name = c
                .names
                .clone()
                .unwrap_or_default()
                .into_iter()
                .next()
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_string();
            projects.entry(project).or_default().push(json!({
                "id": id.clone(),
                "id_short": short_id(&id),
                "name": name,
                "image": c.image,
                "state": c.state,
                "status": c.status,
                "service": labels.get("com.docker.compose.service"),
            }));
        }
        Ok(json!(projects))
    }

    pub async fn execute_streaming(
        &self,
        op: DockerOp,
        payload: Value,
        chunk_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    ) -> anyhow::Result<Value> {
        let policy = self.policy();
        if op.mutating() && !policy.manage {
            anyhow::bail!("docker.manage is disabled on this agent");
        }
        match op {
            DockerOp::ContainerLogs => self.stream_container_logs(&payload, chunk_tx).await,
            DockerOp::ComposeLogs => self.stream_compose_logs(&payload, chunk_tx).await,
            other => anyhow::bail!("{} is not a streaming op", other.as_str()),
        }
    }

    async fn stream_container_logs(
        &self,
        payload: &Value,
        chunk_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    ) -> anyhow::Result<Value> {
        let id = str_field(payload, "id")?;
        let tail = tail_arg(payload);
        let follow = follow_arg(payload);
        let mut stream = self.docker.logs(
            id,
            Some(LogsOptions::<String> {
                stdout: true,
                stderr: true,
                follow,
                tail,
                ..Default::default()
            }),
        );
        while let Some(item) = stream.next().await {
            let output = item?;
            if chunk_tx
                .send(output.to_string().into_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
        Ok(json!({"ok": true}))
    }

    async fn stream_compose_logs(
        &self,
        payload: &Value,
        chunk_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    ) -> anyhow::Result<Value> {
        let project = payload
            .get("project")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let file = payload.get("file").and_then(|v| v.as_str());
        let paths = self.policy().compose_paths;
        let mut args = vec!["compose".to_string()];
        if let Some(f) = file {
            args.push("-f".into());
            args.push(f.into());
        } else if let Some(f) = paths.first() {
            args.push("-f".into());
            args.push(f.clone());
        }
        if !project.is_empty() {
            args.push("-p".into());
            args.push(project.into());
        }
        args.push("logs".into());
        args.push("--tail".into());
        args.push(tail_arg(payload));
        if follow_arg(payload) {
            args.push("-f".into());
        }
        let mut child = Command::new("docker")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("docker compose logs")?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("compose logs stdout"))?;
        let mut buf = vec![0u8; 4096];
        loop {
            let n = tokio::io::AsyncReadExt::read(&mut stdout, &mut buf).await?;
            if n == 0 {
                break;
            }
            if chunk_tx.send(buf[..n].to_vec()).await.is_err() {
                break;
            }
        }
        let status = child.wait().await?;
        if !status.success() && !follow_arg(payload) {
            anyhow::bail!("docker compose logs exited {}", status);
        }
        Ok(json!({"ok": true}))
    }

    async fn image_list(&self) -> anyhow::Result<Value> {
        let images = self
            .docker
            .list_images(Some(ListImagesOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await?;
        let rows: Vec<Value> = images
            .into_iter()
            .map(|img| {
                let id = img.id.clone();
                json!({
                    "id": id,
                    "id_short": short_id(&id),
                    "tags": img.repo_tags,
                    "size": img.size,
                })
            })
            .collect();
        Ok(json!(rows))
    }

    async fn volume_list(&self) -> anyhow::Result<Value> {
        let vols = self
            .docker
            .list_volumes(None::<ListVolumesOptions<String>>)
            .await?;
        let rows: Vec<Value> = vols
            .volumes
            .unwrap_or_default()
            .into_iter()
            .map(|v| {
                json!({
                    "name": v.name,
                    "driver": v.driver,
                    "mountpoint": v.mountpoint,
                })
            })
            .collect();
        Ok(json!(rows))
    }

    async fn network_list(&self) -> anyhow::Result<Value> {
        let nets = self
            .docker
            .list_networks(None::<ListNetworksOptions<String>>)
            .await?;
        let rows: Vec<Value> = nets
            .into_iter()
            .map(|n| {
                let id = n.id.clone().unwrap_or_default();
                json!({
                    "id": id,
                    "id_short": short_id(&id),
                    "name": n.name.unwrap_or_default(),
                    "driver": n.driver.unwrap_or_default(),
                    "scope": n.scope.unwrap_or_default(),
                })
            })
            .collect();
        Ok(json!(rows))
    }
}

fn str_field<'a>(payload: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("missing field {key}"))
}

fn short_id(id: &str) -> String {
    let s = id.trim_start_matches("sha256:");
    s.chars().take(12).collect()
}

fn tail_arg(payload: &Value) -> String {
    payload
        .get("tail")
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "200".into())
}

fn follow_arg(payload: &Value) -> bool {
    payload
        .get("follow")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

fn cpu_ratio(stats: &bollard::container::Stats) -> f64 {
    let cpu = stats.cpu_stats.cpu_usage.total_usage;
    let precpu = stats.precpu_stats.cpu_usage.total_usage;
    let system = stats.cpu_stats.system_cpu_usage.unwrap_or(0);
    let presystem = stats.precpu_stats.system_cpu_usage.unwrap_or(0);
    let delta = cpu.saturating_sub(precpu) as f64;
    let sys_delta = system.saturating_sub(presystem) as f64;
    if sys_delta <= 0.0 {
        return 0.0;
    }
    let ncpu = stats
        .cpu_stats
        .online_cpus
        .or(stats
            .cpu_stats
            .cpu_usage
            .percpu_usage
            .as_ref()
            .map(|v| v.len() as u64))
        .unwrap_or(1) as f64;
    (delta / sys_delta) * ncpu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_strips_sha256_prefix() {
        assert_eq!(short_id("sha256:0123456789abcdef"), "0123456789ab");
        assert_eq!(short_id("abcdef0123456789"), "abcdef012345");
    }

    #[test]
    fn tail_and_follow_defaults() {
        let empty = json!({});
        assert_eq!(tail_arg(&empty), "200");
        assert!(follow_arg(&empty));
        assert_eq!(tail_arg(&json!({"tail": 50})), "50");
        assert!(!follow_arg(&json!({"follow": false})));
    }

    #[test]
    fn streaming_ops_are_logs_only() {
        assert!(DockerOp::ContainerLogs.streams());
        assert!(DockerOp::ComposeLogs.streams());
        assert!(!DockerOp::ContainerList.streams());
        assert!(!DockerOp::ComposeUp.streams());
        assert!(!DockerOp::ComposeUpdate.streams());
    }

    #[test]
    fn compose_update_is_pull_then_up() {
        let src = include_str!("docker.rs");
        let idx = src
            .find("DockerOp::ComposeUpdate => {")
            .expect("compose_update arm");
        let arm = &src[idx..src.len().min(idx + 1200)];
        assert!(arm.contains("\"pull\""), "must pull first");
        assert!(arm.contains("\"up\""), "then compose up");
        assert!(!arm.contains("watchtower"));
        assert!(!arm.contains("apt-get"));
    }
}
