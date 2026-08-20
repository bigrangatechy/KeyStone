// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;
use std::default::Default;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context};
use bollard::container::{
    InspectContainerOptions, KillContainerOptions, ListContainersOptions, LogsOptions,
    PruneContainersOptions, RemoveContainerOptions, StatsOptions,
};
use bollard::image::{ListImagesOptions, PruneImagesOptions, RemoveImageOptions};
use bollard::network::{CreateNetworkOptions, ListNetworksOptions, PruneNetworksOptions};
use bollard::volume::{CreateVolumeOptions, ListVolumesOptions, PruneVolumesOptions};
use bollard::Docker;
use futures_util::future::join_all;
use futures_util::StreamExt;
use keystone_core::config::DockerConfig;
use keystone_core::docker::DockerOp;
use keystone_core::sample::Sample;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::warn;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ComposeSpec {
    project: String,
    files: Vec<String>,
    working_dir: Option<String>,
    images: Vec<String>,
}

#[derive(Clone)]
pub struct DockerHandle {
    docker: Docker,
    cfg: Arc<Mutex<DockerConfig>>,
    known_compose: Arc<Mutex<HashMap<String, ComposeSpec>>>,
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
            known_compose: Arc::new(Mutex::new(HashMap::new())),
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
        let mut running = Vec::new();
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
            let is_running = c.state.as_deref() == Some("running");
            out.push(
                Sample::new("container_running", if is_running { 1.0 } else { 0.0 }, ts)
                    .with_label("id", &short)
                    .with_label("name", &name)
                    .with_label("compose_project", &project),
            );
            if is_running {
                running.push((id, short, name, project));
            }
        }
        out.extend(stats_samples(&self.docker, running, ts).await);
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
            DockerOp::ContainerPause => {
                let id = str_field(&payload, "id")?;
                self.docker.pause_container(id).await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::ContainerUnpause => {
                let id = str_field(&payload, "id")?;
                self.docker.unpause_container(id).await?;
                Ok(json!({"ok": true}))
            }
            DockerOp::ContainerPrune => {
                let report = self
                    .docker
                    .prune_containers(None::<PruneContainersOptions<String>>)
                    .await?;
                Ok(serde_json::to_value(report)?)
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
            DockerOp::VolumePrune => {
                let report = self
                    .docker
                    .prune_volumes(None::<PruneVolumesOptions<String>>)
                    .await?;
                Ok(serde_json::to_value(report)?)
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
            DockerOp::NetworkPrune => {
                let report = self
                    .docker
                    .prune_networks(None::<PruneNetworksOptions<String>>)
                    .await?;
                Ok(serde_json::to_value(report)?)
            }
            DockerOp::ComposePs
            | DockerOp::ComposeUp
            | DockerOp::ComposeStop
            | DockerOp::ComposeStart
            | DockerOp::ComposeRestart
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
                    "compose_project": compose_project,
                    "ports": format_summary_ports(c.ports.as_deref().unwrap_or(&[])),
                })
            })
            .collect();
        Ok(json!(rows))
    }

    async fn compose(&self, op: DockerOp, payload: &Value) -> anyhow::Result<Value> {
        if matches!(op, DockerOp::ComposePs) {
            let project = payload
                .get("project")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return self.compose_ps_merged(project).await;
        }
        let spec = self.resolve_compose(payload).await?;
        match op {
            DockerOp::ComposeUp => self.compose_up_spec(&spec).await,
            DockerOp::ComposeStop => self.run_compose_cli(&spec, &["stop"]).await,
            DockerOp::ComposeStart => self.run_compose_cli(&spec, &["start"]).await,
            DockerOp::ComposeRestart => self.run_compose_cli(&spec, &["restart"]).await,
            DockerOp::ComposeDown => self.compose_down_spec(&spec).await,
            DockerOp::ComposePull => self.compose_pull_spec(&spec).await,
            DockerOp::ComposeUpdate => {
                self.compose_pull_spec(&spec).await?;
                self.compose_up_spec(&spec).await
            }
            _ => unreachable!(),
        }
    }

    async fn compose_ps_merged(&self, project_filter: &str) -> anyhow::Result<Value> {
        let list = self
            .docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await?;
        let mut projects: HashMap<String, Vec<Value>> = HashMap::new();
        let mut seen: HashMap<String, ComposeSpec> = HashMap::new();
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
            remember_compose(
                &mut seen,
                &spec_from_container_labels(&project, &labels, c.image.as_deref()),
            );
            projects.entry(project).or_default().push(json!({
                "id": id.clone(),
                "id_short": short_id(&id),
                "name": name,
                "image": c.image,
                "state": c.state,
                "status": c.status,
                "service": labels.get("com.docker.compose.service"),
                "ports": format_summary_ports(c.ports.as_deref().unwrap_or(&[])),
            }));
        }
        for path in &self.policy().compose_paths {
            let spec = spec_from_compose_path(path);
            if !project_filter.is_empty() && spec.project != project_filter {
                continue;
            }
            remember_compose(&mut seen, &spec);
            projects.entry(spec.project.clone()).or_default();
        }
        {
            let mut known = self.known_compose.lock().unwrap_or_else(|e| e.into_inner());
            for spec in seen.values() {
                remember_compose(&mut known, spec);
            }
            for name in known.keys() {
                if !project_filter.is_empty() && name != project_filter {
                    continue;
                }
                projects.entry(name.clone()).or_default();
            }
        }
        Ok(json!(projects))
    }

    async fn resolve_compose(&self, payload: &Value) -> anyhow::Result<ComposeSpec> {
        let project = payload
            .get("project")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut spec = ComposeSpec {
            project: project.clone(),
            ..Default::default()
        };
        {
            let known = self.known_compose.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(prev) = known.get(&project) {
                spec = prev.clone();
                if spec.project.is_empty() {
                    spec.project = project.clone();
                }
            }
        }
        if let Some(engine) = self.compose_spec_from_engine(&project).await? {
            overlay_compose(&mut spec, &engine);
        }
        if let Some(f) = payload.get("file").and_then(|v| v.as_str()) {
            overlay_compose(&mut spec, &spec_from_compose_path(f));
        }
        if spec.files.is_empty() {
            if let Some(from_path) =
                spec_matching_compose_path(&self.policy().compose_paths, &project)
            {
                overlay_compose(&mut spec, &from_path);
            }
        }
        if spec.project.is_empty() {
            spec.project = project;
        }
        {
            let mut known = self.known_compose.lock().unwrap_or_else(|e| e.into_inner());
            remember_compose(&mut known, &spec);
        }
        Ok(spec)
    }

    async fn compose_spec_from_engine(&self, project: &str) -> anyhow::Result<Option<ComposeSpec>> {
        if project.is_empty() {
            return Ok(None);
        }
        let list = self
            .docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            }))
            .await?;
        let mut spec = ComposeSpec {
            project: project.to_string(),
            ..Default::default()
        };
        let mut found = false;
        for c in list {
            let labels = c.labels.clone().unwrap_or_default();
            let Some(p) = labels.get("com.docker.compose.project") else {
                continue;
            };
            if p != project {
                continue;
            }
            found = true;
            overlay_compose(
                &mut spec,
                &spec_from_container_labels(project, &labels, c.image.as_deref()),
            );
        }
        Ok(found.then_some(spec))
    }

    async fn compose_pull_spec(&self, spec: &ComposeSpec) -> anyhow::Result<Value> {
        if !spec.files.is_empty() {
            match self.run_compose_cli(spec, &["pull"]).await {
                Ok(v) => return Ok(v),
                Err(e) if spec.images.is_empty() => return Err(e),
                Err(e) => {
                    warn!("docker compose pull via file failed, pulling images: {e}");
                }
            }
        }
        if !spec.images.is_empty() {
            return self.pull_images(&spec.images).await;
        }
        anyhow::bail!(
            "no Compose file or images for project {}. Add the compose YAML path on this node's Settings (readable by user keystone).",
            spec.project
        )
    }

    async fn compose_up_spec(&self, spec: &ComposeSpec) -> anyhow::Result<Value> {
        if spec.files.is_empty() {
            anyhow::bail!(
                "no Compose file for project {}. Add the compose YAML path on this node's Settings so Up can recreate the stack after Down.",
                spec.project
            );
        }
        self.run_compose_cli(spec, &["up", "-d"]).await
    }

    async fn compose_down_spec(&self, spec: &ComposeSpec) -> anyhow::Result<Value> {
        self.run_compose_cli(spec, &["down"]).await
    }

    async fn run_compose_cli(&self, spec: &ComposeSpec, extra: &[&str]) -> anyhow::Result<Value> {
        let output = compose_command(spec, extra)
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

    async fn pull_images(&self, images: &[String]) -> anyhow::Result<Value> {
        let mut unique = Vec::new();
        for img in images {
            let img = img.trim();
            if img.is_empty() || img == "<none>" || img.contains("sha256:") {
                continue;
            }
            if !unique.iter().any(|u: &String| u == img) {
                unique.push(img.to_string());
            }
        }
        if unique.is_empty() {
            anyhow::bail!("no images to pull for this Compose project");
        }
        for name in &unique {
            let mut stream = self.docker.create_image(
                Some(bollard::image::CreateImageOptions {
                    from_image: name.as_str(),
                    ..Default::default()
                }),
                None,
                None,
            );
            while let Some(item) = stream.next().await {
                item.with_context(|| format!("pull {name}"))?;
            }
        }
        Ok(json!({"ok": true, "pulled": unique}))
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
        let spec = self.resolve_compose(payload).await?;
        let mut extra = vec!["logs".to_string(), "--tail".into(), tail_arg(payload)];
        if follow_arg(payload) {
            extra.push("-f".into());
        }
        let extra_ref: Vec<&str> = extra.iter().map(|s| s.as_str()).collect();
        let mut child = compose_command(&spec, &extra_ref)
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

fn remember_compose(map: &mut HashMap<String, ComposeSpec>, spec: &ComposeSpec) {
    if spec.project.is_empty() {
        return;
    }
    let entry = map
        .entry(spec.project.clone())
        .or_insert_with(|| ComposeSpec {
            project: spec.project.clone(),
            ..Default::default()
        });
    if !spec.files.is_empty() {
        entry.files = spec.files.clone();
    }
    if spec.working_dir.is_some() {
        entry.working_dir = spec.working_dir.clone();
    }
    for img in &spec.images {
        if !img.is_empty() && !entry.images.iter().any(|e| e == img) {
            entry.images.push(img.clone());
        }
    }
}

fn overlay_compose(base: &mut ComposeSpec, other: &ComposeSpec) {
    if base.project.is_empty() {
        base.project = other.project.clone();
    }
    if !other.files.is_empty() {
        base.files = other.files.clone();
    }
    if other.working_dir.is_some() {
        base.working_dir = other.working_dir.clone();
    }
    for img in &other.images {
        if !img.is_empty() && !base.images.iter().any(|e| e == img) {
            base.images.push(img.clone());
        }
    }
}

fn split_compose_config_files(raw: &str) -> Vec<String> {
    raw.split([',', '|'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn project_name_from_compose_path(path: &str) -> String {
    let p = Path::new(path);
    let looks_like_file = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e, "yml" | "yaml" | "YML" | "YAML"))
        .unwrap_or(false);
    let dir = if looks_like_file {
        p.parent().unwrap_or(p)
    } else {
        p
    };
    dir.file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("default")
        .to_string()
}

fn compose_project_name_from_yaml(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name:") {
            let rest = rest.trim().trim_matches('"').trim_matches('\'');
            if !rest.is_empty()
                && rest
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
            {
                return Some(rest.to_string());
            }
            return None;
        }
        if line.ends_with(':') && !line.starts_with('-') {
            return None;
        }
    }
    None
}

fn spec_from_compose_path(path: &str) -> ComposeSpec {
    let mut project = project_name_from_compose_path(path);
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Some(n) = compose_project_name_from_yaml(&text) {
            project = n;
        }
    }
    let working_dir = Path::new(path)
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    ComposeSpec {
        project,
        files: vec![path.to_string()],
        working_dir,
        images: Vec::new(),
    }
}

fn spec_from_container_labels(
    project: &str,
    labels: &HashMap<String, String>,
    image: Option<&str>,
) -> ComposeSpec {
    let files = labels
        .get("com.docker.compose.project.config_files")
        .map(|s| split_compose_config_files(s))
        .unwrap_or_default();
    let working_dir = labels
        .get("com.docker.compose.project.working_dir")
        .cloned()
        .filter(|s| !s.is_empty());
    let mut images = Vec::new();
    if let Some(img) = image {
        if !img.is_empty() {
            images.push(img.to_string());
        }
    }
    ComposeSpec {
        project: project.to_string(),
        files,
        working_dir,
        images,
    }
}

fn spec_matching_compose_path(paths: &[String], project: &str) -> Option<ComposeSpec> {
    let specs: Vec<ComposeSpec> = paths.iter().map(|p| spec_from_compose_path(p)).collect();
    if let Some(spec) = specs.iter().find(|s| s.project == project) {
        return Some(spec.clone());
    }
    if !project.is_empty() {
        for path in paths {
            if Path::new(path)
                .components()
                .any(|c| c.as_os_str() == project)
            {
                return Some(spec_from_compose_path(path));
            }
        }
    }
    if specs.len() == 1 && (project.is_empty() || specs[0].project == project) {
        return specs.into_iter().next();
    }
    None
}

fn compose_cli_args(spec: &ComposeSpec) -> Vec<String> {
    let mut args = vec!["compose".to_string()];
    for f in &spec.files {
        args.push("-f".into());
        args.push(f.clone());
    }
    if !spec.project.is_empty() {
        args.push("-p".into());
        args.push(spec.project.clone());
    }
    args
}

fn compose_command(spec: &ComposeSpec, extra: &[&str]) -> Command {
    let mut args = compose_cli_args(spec);
    args.extend(extra.iter().map(|s| (*s).to_string()));
    let mut cmd = Command::new("docker");
    if let Some(dir) = spec
        .working_dir
        .as_deref()
        .filter(|d| Path::new(d).is_dir())
    {
        cmd.current_dir(dir);
    } else if let Some(f) = spec.files.first() {
        if let Some(parent) = Path::new(f).parent() {
            if parent.is_dir() {
                cmd.current_dir(parent);
            }
        }
    }
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd
}

fn format_one_port(ip: Option<&str>, private: u16, public: Option<u16>, proto: &str) -> String {
    let proto = if proto.is_empty() { "tcp" } else { proto };
    match public {
        Some(host_port) => {
            let host = ip.filter(|s| !s.is_empty()).unwrap_or("0.0.0.0");
            format!("{host}:{host_port}->{private}/{proto}")
        }
        None => format!("{private}/{proto}"),
    }
}

fn format_summary_ports(ports: &[bollard::models::Port]) -> String {
    ports
        .iter()
        .map(|p| {
            format_one_port(
                p.ip.as_deref(),
                p.private_port,
                p.public_port,
                &p.typ.as_ref().map(|t| t.to_string()).unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
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

/// One-shot Engine stats wait ~1s per container if done in series, which
/// starves `container_list` on the same socket. Run them together and cap
/// the wait so a push tick cannot eat the node-page RPC budget.
async fn stats_samples(
    docker: &Docker,
    running: Vec<(String, String, String, String)>,
    ts: i64,
) -> Vec<Sample> {
    let futs = running.into_iter().map(|(id, short, name, project)| {
        let docker = docker.clone();
        async move {
            let mut stream = docker.stats(
                &id,
                Some(StatsOptions {
                    stream: false,
                    one_shot: true,
                }),
            );
            let s = match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
                Ok(Some(Ok(s))) => s,
                _ => return Vec::new(),
            };
            let mem = s.memory_stats.usage.unwrap_or(0) as f64;
            vec![
                Sample::new("container_memory_usage_bytes", mem, ts)
                    .with_label("id", &short)
                    .with_label("name", &name)
                    .with_label("compose_project", &project),
                Sample::new("container_cpu_usage_ratio", cpu_ratio(&s), ts)
                    .with_label("id", &short)
                    .with_label("name", &name)
                    .with_label("compose_project", &project),
            ]
        }
    });
    match tokio::time::timeout(Duration::from_secs(3), join_all(futs)).await {
        Ok(parts) => parts.into_iter().flatten().collect(),
        Err(_) => Vec::new(),
    }
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
    fn container_list_omits_full_label_maps() {
        let src = include_str!("docker.rs");
        let fn_src = src
            .split("async fn container_list")
            .nth(1)
            .expect("container_list");
        assert!(
            fn_src.contains("compose_project"),
            "Containers tab still needs the compose project"
        );
        assert!(
            fn_src.contains("ports"),
            "Containers tab must show published ports"
        );
        assert!(
            !fn_src.contains("\"labels\": labels"),
            "full label maps on every container blow the ingest Result past the page wait"
        );
    }

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
    fn container_stats_collect_is_bounded_and_parallel() {
        let src = include_str!("docker.rs");
        let fn_src = src
            .split("async fn stats_samples")
            .nth(1)
            .expect("stats_samples");
        assert!(
            fn_src.contains("join_all"),
            "one-shot stats must not run one container after another"
        );
        assert!(
            fn_src.contains("timeout"),
            "stats collect must not wait forever on docker.sock"
        );
    }

    #[test]
    fn compose_update_is_pull_then_up() {
        let src = include_str!("docker.rs");
        let idx = src
            .find("DockerOp::ComposeUpdate => {")
            .expect("compose_update arm");
        let arm = &src[idx..src.len().min(idx + 1200)];
        assert!(arm.contains("compose_pull_spec"), "must pull first");
        assert!(arm.contains("compose_up_spec"), "then compose up");
        assert!(!arm.contains("watchtower"));
        assert!(!arm.contains("apt-get"));
    }

    #[test]
    fn compose_pull_does_not_use_the_first_settings_path() {
        let src = include_str!("docker.rs");
        let needle = format!("{}{}", "paths.", "first(");
        assert!(
            !src.contains(&needle),
            "every compose command used the first Settings path, so Pull/Up hit the wrong stack"
        );
        assert!(src.contains("com.docker.compose.project.config_files"));
        assert!(src.contains("com.docker.compose.project.working_dir"));
        assert!(src.contains("compose_ps_merged"));
        assert!(src.contains("pull_images"));
    }

    #[test]
    fn compose_path_project_name() {
        assert_eq!(
            project_name_from_compose_path("/home/user/tunnel/compose.yaml"),
            "tunnel"
        );
        assert_eq!(
            compose_project_name_from_yaml("# comment\nname: cloudflared\nservices:\n  app:\n"),
            Some("cloudflared".into())
        );
        assert_eq!(
            compose_project_name_from_yaml("services:\n  web:\n    image: nginx\n"),
            None
        );
        assert_eq!(
            split_compose_config_files("/opt/a/compose.yml,/opt/a/compose.override.yml"),
            vec![
                "/opt/a/compose.yml".to_string(),
                "/opt/a/compose.override.yml".to_string()
            ]
        );
        let spec = spec_matching_compose_path(
            &[
                "/opt/stacks/alpha/compose.yaml".into(),
                "/opt/stacks/beta/compose.yaml".into(),
            ],
            "beta",
        )
        .expect("beta");
        assert_eq!(spec.project, "beta");
        assert_eq!(spec.files, vec!["/opt/stacks/beta/compose.yaml"]);
        assert!(spec_matching_compose_path(
            &[
                "/opt/stacks/alpha/compose.yaml".into(),
                "/opt/stacks/beta/compose.yaml".into(),
            ],
            "gamma",
        )
        .is_none());
        let args = compose_cli_args(&ComposeSpec {
            project: "beta".into(),
            files: vec!["/opt/stacks/beta/compose.yaml".into()],
            working_dir: Some("/opt/stacks/beta".into()),
            images: vec![],
        });
        assert_eq!(
            args,
            vec![
                "compose",
                "-f",
                "/opt/stacks/beta/compose.yaml",
                "-p",
                "beta"
            ]
        );
        assert_eq!(
            compose_cli_args(&ComposeSpec {
                project: "beta".into(),
                ..Default::default()
            }),
            vec!["compose", "-p", "beta"]
        );
        assert_eq!(
            project_name_from_compose_path("/opt/stacks/pihole/compose.yml"),
            "pihole"
        );
        assert_eq!(
            compose_project_name_from_yaml("name: \"cf-tunnel\"\nservices:\n  app:\n"),
            Some("cf-tunnel".into())
        );
        assert_eq!(
            compose_project_name_from_yaml("name: 'cf-tunnel'\nservices:\n"),
            Some("cf-tunnel".into())
        );
    }

    #[test]
    fn compose_stop_start_restart_keep_containers() {
        let src = include_str!("docker.rs");
        let compose = src
            .split("async fn compose(")
            .nth(1)
            .expect("compose")
            .split("async fn compose_ps_merged")
            .next()
            .expect("compose body");
        assert!(compose.contains("ComposeStop") && compose.contains("\"stop\""));
        assert!(compose.contains("ComposeStart") && compose.contains("\"start\""));
        assert!(compose.contains("ComposeRestart") && compose.contains("\"restart\""));
        assert!(
            compose.contains("run_compose_cli(&spec, &[\"stop\"])")
                || compose.contains("&[\"stop\"]"),
            "Stop must be docker compose stop, not down"
        );
        let up = src
            .split("async fn compose_up_spec")
            .nth(1)
            .expect("compose_up_spec");
        assert!(up.contains("files.is_empty()"));
        let down = src
            .split("async fn compose_down_spec")
            .nth(1)
            .expect("compose_down_spec")
            .split("async fn run_compose_cli")
            .next()
            .expect("down body");
        assert!(
            !down.contains("files.is_empty()"),
            "Down must still run with only -p so a label-discovered stack can be removed"
        );
    }

    #[test]
    fn remember_compose_keeps_files_when_later_row_has_only_images() {
        let mut map = HashMap::new();
        remember_compose(
            &mut map,
            &ComposeSpec {
                project: "p".into(),
                files: vec!["/a.yml".into()],
                working_dir: Some("/a".into()),
                images: vec![],
            },
        );
        remember_compose(
            &mut map,
            &ComposeSpec {
                project: "p".into(),
                images: vec!["nginx:1".into()],
                ..Default::default()
            },
        );
        let spec = map.get("p").expect("p");
        assert_eq!(spec.files, vec!["/a.yml"]);
        assert_eq!(spec.working_dir.as_deref(), Some("/a"));
        assert_eq!(spec.images, vec!["nginx:1"]);
        let mut base = spec.clone();
        overlay_compose(
            &mut base,
            &ComposeSpec {
                project: "p".into(),
                ..Default::default()
            },
        );
        assert_eq!(base.files, vec!["/a.yml"]);
    }

    #[test]
    fn compose_paths_keep_a_project_with_no_containers() {
        let mut projects: HashMap<String, Vec<Value>> = HashMap::new();
        let spec = spec_from_compose_path("/opt/stacks/tunnel/compose.yaml");
        projects.entry(spec.project.clone()).or_default();
        assert!(projects.get("tunnel").expect("tunnel").is_empty());
    }

    #[test]
    fn published_ports_format_host_and_unpublished() {
        assert_eq!(
            format_one_port(Some("0.0.0.0"), 80, Some(8080), "tcp"),
            "0.0.0.0:8080->80/tcp"
        );
        assert_eq!(
            format_one_port(None, 53, Some(53), "udp"),
            "0.0.0.0:53->53/udp"
        );
        assert_eq!(format_one_port(None, 443, None, ""), "443/tcp");
    }
}
