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
            DockerOp::ContainerLogs => {
                let id = str_field(&payload, "id")?;
                let tail = payload
                    .get("tail")
                    .and_then(|v| v.as_str())
                    .unwrap_or("200");
                let mut stream = self.docker.logs(
                    id,
                    Some(LogsOptions::<String> {
                        stdout: true,
                        stderr: true,
                        tail: tail.to_string(),
                        ..Default::default()
                    }),
                );
                let mut text = String::new();
                while let Some(chunk) = stream.next().await {
                    if let Ok(output) = chunk {
                        text.push_str(&output.to_string());
                    }
                }
                Ok(json!({"logs": text}))
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
            DockerOp::ImageList => {
                let images = self
                    .docker
                    .list_images(Some(ListImagesOptions::<String> {
                        all: true,
                        ..Default::default()
                    }))
                    .await?;
                Ok(serde_json::to_value(images)?)
            }
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
            DockerOp::VolumeList => {
                let vols = self
                    .docker
                    .list_volumes(None::<ListVolumesOptions<String>>)
                    .await?;
                Ok(serde_json::to_value(vols)?)
            }
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
            DockerOp::NetworkList => {
                let nets = self
                    .docker
                    .list_networks(None::<ListNetworksOptions<String>>)
                    .await?;
                Ok(serde_json::to_value(nets)?)
            }
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
            | DockerOp::ComposeLogs
            | DockerOp::ComposePull => self.compose(op, &payload).await,
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
            DockerOp::ComposeLogs => {
                args.push("logs".into());
                args.push("--tail".into());
                args.push("200".into());
            }
            DockerOp::ComposePull => args.push("pull".into()),
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
            projects.entry(project).or_default().push(json!({
                "id": c.id,
                "names": c.names,
                "image": c.image,
                "state": c.state,
                "service": labels.get("com.docker.compose.service"),
            }));
        }
        Ok(json!(projects))
    }
}

fn str_field<'a>(payload: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("missing field {key}"))
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
