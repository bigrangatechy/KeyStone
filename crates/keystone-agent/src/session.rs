// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use keystone_core::config::{AgentConfig, DockerConfig};
use keystone_core::docker::DockerOp;
use keystone_core::node::NodeIdentity;
use keystone_core::sample::{self, Label, Sample};
use keystone_core::{AgentRuntime, NodeSettings};
use keystone_proto::ingest_client::IngestClient;
use keystone_proto::{
    agent_to_server, server_to_agent, Ack, AgentToServer, CommandResult, Heartbeat, PushFrame,
    ServerToAgent, StreamChunk,
};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Endpoint;
use tracing::{info, warn};

use crate::buffer::DiskBuffer;
use crate::collect::collect_host;
use crate::docker::DockerHandle;

struct AgentRuntimeState {
    docker: Mutex<Option<DockerHandle>>,
    labels: Mutex<BTreeMap<String, String>>,
    docker_host: String,
}

impl AgentRuntimeState {
    fn from_config(cfg: &AgentConfig) -> Self {
        let docker = if cfg.docker.enabled {
            match DockerHandle::connect(&cfg.docker) {
                Ok(d) => Some(d),
                Err(e) => {
                    warn!("docker disabled: {e}");
                    None
                }
            }
        } else {
            None
        };
        Self {
            docker: Mutex::new(docker),
            labels: Mutex::new(cfg.labels.clone()),
            docker_host: cfg.docker.host.clone(),
        }
    }
}

pub async fn run(cfg: AgentConfig) -> anyhow::Result<()> {
    let node_id = if cfg.node_id.is_empty() {
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".into())
    } else {
        cfg.node_id.clone()
    };

    let runtime = Arc::new(AgentRuntimeState::from_config(&cfg));
    let buffer = DiskBuffer::new(&cfg.buffer_dir)?;
    let endpoint = Endpoint::from_shared(cfg.ingest_url.clone())?
        .connect_timeout(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(30));

    loop {
        match connect_session(&cfg, &node_id, runtime.clone(), &buffer, endpoint.clone()).await {
            Ok(()) => info!("ingest session ended, reconnecting"),
            Err(e) => warn!("ingest session error: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn connect_session(
    cfg: &AgentConfig,
    node_id: &str,
    runtime: Arc<AgentRuntimeState>,
    buffer: &DiskBuffer,
    endpoint: Endpoint,
) -> anyhow::Result<()> {
    let channel = endpoint.connect().await.context("connect ingest")?;
    let mut client = IngestClient::new(channel);
    let (tx, rx) = mpsc::channel::<AgentToServer>(256);
    let response = client.session(ReceiverStream::new(rx)).await?;
    let mut inbound = response.into_inner();

    info!("connected to ingest at {}", cfg.ingest_url);

    if let Ok(frames) = buffer.drain() {
        for frame in frames {
            let _ = tx
                .send(AgentToServer {
                    body: Some(agent_to_server::Body::Push(frame)),
                })
                .await;
        }
    }

    let push_tx = tx.clone();
    let node_id_owned = node_id.to_string();
    let cancels: Arc<std::sync::Mutex<HashMap<String, AbortHandle>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));

    let mut interval = tokio::time::interval(Duration::from_secs(cfg.interval_secs.max(1)));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let mut samples = collect_host(cfg);
                let docker = runtime.docker.lock().await.clone();
                if let Some(d) = docker.as_ref() {
                    samples.extend(d.collect_container_metrics().await);
                }
                let (kept, dropped) = sample::allowlist(samples);
                if dropped > 0 {
                    warn!("dropped {dropped} unknown metrics");
                }
                let labels = runtime.labels.lock().await.clone();
                let frame = build_push(cfg, &node_id_owned, docker.as_ref(), &labels, &kept).await;
                if push_tx.send(AgentToServer {
                    body: Some(agent_to_server::Body::Push(frame.clone())),
                }).await.is_err() {
                    buffer.push(&frame)?;
                    break;
                }
            }
            msg = inbound.message() => {
                match msg {
                    Ok(Some(ServerToAgent { body: Some(server_to_agent::Body::Ack(ack)) })) => {
                        handle_ack(&ack);
                    }
                    Ok(Some(ServerToAgent { body: Some(server_to_agent::Body::Command(cmd)) })) => {
                        if cmd.op == "cancel" {
                            let target = cancel_target(&cmd.payload_json);
                            if let Some(handle) = cancels
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .remove(&target)
                            {
                                handle.abort();
                            }
                            if tx.send(AgentToServer {
                                body: Some(agent_to_server::Body::Result(CommandResult {
                                    request_id: cmd.request_id,
                                    ok: true,
                                    payload_json: "{}".into(),
                                    error: String::new(),
                                })),
                            }).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        if cmd.op == "set_interval" {
                            let body = match parse_set_interval(&cmd.payload_json) {
                                Ok(secs) => {
                                    apply_interval(&mut interval, secs);
                                    CommandResult {
                                        request_id: cmd.request_id,
                                        ok: true,
                                        payload_json: serde_json::json!({ "interval_secs": secs }).to_string(),
                                        error: String::new(),
                                    }
                                }
                                Err(e) => command_err(cmd.request_id, e),
                            };
                            if tx.send(AgentToServer {
                                body: Some(agent_to_server::Body::Result(body)),
                            }).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        if cmd.op == "set_runtime" {
                            let body = match apply_runtime(&runtime, &mut interval, &cmd.payload_json).await {
                                Ok(v) => CommandResult {
                                    request_id: cmd.request_id,
                                    ok: true,
                                    payload_json: v.to_string(),
                                    error: String::new(),
                                },
                                Err(e) => command_err(cmd.request_id, e),
                            };
                            if tx.send(AgentToServer {
                                body: Some(agent_to_server::Body::Result(body)),
                            }).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        let streaming = DockerOp::from_str(&cmd.op).ok().is_some_and(DockerOp::streams);
                        if streaming {
                            spawn_streaming_command(
                                runtime.clone(),
                                tx.clone(),
                                cancels.clone(),
                                cmd.request_id,
                                cmd.op,
                                cmd.payload_json,
                            );
                            continue;
                        }
                        let body = match handle_command(&runtime, &cmd.op, &cmd.payload_json).await {
                            Ok(payload) => CommandResult {
                                request_id: cmd.request_id,
                                ok: true,
                                payload_json: payload.to_string(),
                                error: String::new(),
                            },
                            Err(e) => command_err(cmd.request_id, e),
                        };
                        if tx.send(AgentToServer {
                            body: Some(agent_to_server::Body::Result(body)),
                        }).await.is_err() {
                            break;
                        }
                    }
                    Ok(Some(_)) | Ok(None) => break,
                    Err(e) => {
                        warn!("stream closed: {e}");
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

fn command_err(request_id: String, e: impl std::fmt::Display) -> CommandResult {
    CommandResult {
        request_id,
        ok: false,
        payload_json: String::new(),
        error: e.to_string(),
    }
}

fn cancel_target(payload_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(payload_json)
        .ok()
        .and_then(|v| v.get("request_id")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn spawn_streaming_command(
    runtime: Arc<AgentRuntimeState>,
    tx: mpsc::Sender<AgentToServer>,
    cancels: Arc<std::sync::Mutex<HashMap<String, AbortHandle>>>,
    request_id: String,
    op: String,
    payload_json: String,
) {
    let rid = request_id.clone();
    let cancels_done = cancels.clone();
    let handle = tokio::spawn(async move {
        let result = run_streaming(&runtime, &op, &payload_json, tx.clone(), &request_id).await;
        let body = match result {
            Ok(payload) => CommandResult {
                request_id: request_id.clone(),
                ok: true,
                payload_json: payload.to_string(),
                error: String::new(),
            },
            Err(e) => command_err(request_id.clone(), e),
        };
        let _ = tx
            .send(AgentToServer {
                body: Some(agent_to_server::Body::Result(body)),
            })
            .await;
        cancels_done
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&request_id);
    });
    cancels
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(rid, handle.abort_handle());
}

async fn run_streaming(
    runtime: &AgentRuntimeState,
    op: &str,
    payload_json: &str,
    tx: mpsc::Sender<AgentToServer>,
    request_id: &str,
) -> anyhow::Result<serde_json::Value> {
    let docker = runtime
        .docker
        .lock()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("docker is not enabled on this agent"))?;
    let op = DockerOp::from_str(op).map_err(|_| anyhow::anyhow!("unknown docker op {op}"))?;
    let payload = if payload_json.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(payload_json).context("payload json")?
    };
    let (chunk_tx, mut chunk_rx) = mpsc::channel::<Vec<u8>>(128);
    let tx_chunks = tx.clone();
    let rid = request_id.to_string();
    let forward = tokio::spawn(async move {
        while let Some(data) = chunk_rx.recv().await {
            if tx_chunks
                .send(AgentToServer {
                    body: Some(agent_to_server::Body::Chunk(StreamChunk {
                        request_id: rid.clone(),
                        data,
                        eof: false,
                    })),
                })
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let result = docker.execute_streaming(op, payload, chunk_tx).await;
    let _ = forward.await;
    let _ = tx
        .send(AgentToServer {
            body: Some(agent_to_server::Body::Chunk(StreamChunk {
                request_id: request_id.to_string(),
                data: Vec::new(),
                eof: true,
            })),
        })
        .await;
    result
}

fn handle_ack(ack: &Ack) {
    if !ack.ok {
        warn!("ingest nack: {}", ack.error);
    }
}

fn apply_interval(interval: &mut tokio::time::Interval, secs: u64) {
    *interval = tokio::time::interval(Duration::from_secs(secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    info!("push interval set to {secs}s");
}

fn parse_set_interval(payload_json: &str) -> anyhow::Result<u64> {
    let v: serde_json::Value = if payload_json.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(payload_json).context("set_interval payload")?
    };
    let secs = v.get("interval_secs").and_then(|x| x.as_u64()).unwrap_or(1);
    Ok(NodeSettings::clamp_poll_secs(secs as u32) as u64)
}

fn parse_set_runtime(payload_json: &str) -> anyhow::Result<AgentRuntime> {
    let raw = if payload_json.trim().is_empty() {
        "{}"
    } else {
        payload_json
    };
    let mut rt: AgentRuntime = serde_json::from_str(raw).context("set_runtime payload")?;
    rt.interval_secs = NodeSettings::clamp_poll_secs(rt.interval_secs as u32) as u64;
    Ok(rt)
}

async fn apply_runtime(
    runtime: &AgentRuntimeState,
    interval: &mut tokio::time::Interval,
    payload_json: &str,
) -> anyhow::Result<serde_json::Value> {
    let rt = parse_set_runtime(payload_json)?;
    apply_interval(interval, rt.interval_secs);
    {
        let mut labels = runtime.labels.lock().await;
        *labels = rt.labels.clone();
    }
    {
        let mut docker = runtime.docker.lock().await;
        if rt.docker_enabled {
            if let Some(handle) = docker.as_ref() {
                handle.set_policy(
                    rt.docker_manage,
                    rt.docker_allow_exec,
                    rt.compose_paths.clone(),
                );
            } else {
                let cfg = DockerConfig {
                    enabled: true,
                    manage: rt.docker_manage,
                    allow_exec: rt.docker_allow_exec,
                    host: runtime.docker_host.clone(),
                    compose_paths: rt.compose_paths.clone(),
                };
                match DockerHandle::connect(&cfg) {
                    Ok(handle) => {
                        info!("docker enabled from Settings");
                        *docker = Some(handle);
                    }
                    Err(e) => {
                        warn!("docker enable failed: {e}");
                        *docker = None;
                    }
                }
            }
        } else if docker.is_some() {
            info!("docker disabled from Settings");
            *docker = None;
        }
    }
    Ok(serde_json::to_value(&rt)?)
}

async fn handle_command(
    runtime: &AgentRuntimeState,
    op: &str,
    payload_json: &str,
) -> anyhow::Result<serde_json::Value> {
    let docker = runtime.docker.lock().await;
    let Some(docker) = docker.as_ref() else {
        anyhow::bail!("docker is not enabled on this agent");
    };
    let op = DockerOp::from_str(op).map_err(|_| anyhow::anyhow!("unknown docker op {op}"))?;
    let payload = if payload_json.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(payload_json).context("payload json")?
    };
    docker.execute(op, payload).await
}

async fn build_push(
    cfg: &AgentConfig,
    node_id: &str,
    docker: Option<&DockerHandle>,
    labels: &BTreeMap<String, String>,
    samples: &[Sample],
) -> PushFrame {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| node_id.to_string());
    let docker_version = match docker {
        Some(d) => d.engine_version().await.unwrap_or_default(),
        None => String::new(),
    };
    let labels: Vec<keystone_proto::Label> = labels
        .iter()
        .map(|(k, v)| keystone_proto::Label {
            name: k.clone(),
            value: v.clone(),
        })
        .collect();
    PushFrame {
        heartbeat: Some(Heartbeat {
            node_id: node_id.to_string(),
            hostname,
            agent_version: keystone_core::VERSION.to_string(),
            os: std::env::consts::OS.to_string(),
            kernel: sysinfo::System::kernel_version().unwrap_or_default(),
            docker_version,
            labels,
        }),
        samples: samples
            .iter()
            .map(|s| keystone_proto::Sample {
                metric: s.metric.clone(),
                labels: s
                    .labels
                    .iter()
                    .map(|l: &Label| keystone_proto::Label {
                        name: l.name.clone(),
                        value: l.value.clone(),
                    })
                    .collect(),
                value: s.value,
                timestamp_unix_ms: s.timestamp_unix_ms,
            })
            .collect(),
        ingest_token: cfg.ingest_token.clone(),
    }
}

pub fn identity_from_heartbeat(hb: &Heartbeat) -> NodeIdentity {
    NodeIdentity {
        node_id: hb.node_id.clone(),
        hostname: hb.hostname.clone(),
        agent_version: hb.agent_version.clone(),
        os: hb.os.clone(),
        kernel: hb.kernel.clone(),
        docker_version: if hb.docker_version.is_empty() {
            None
        } else {
            Some(hb.docker_version.clone())
        },
        labels: hb
            .labels
            .iter()
            .map(|l| Label {
                name: l.name.clone(),
                value: l.value.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_interval_clamps() {
        assert_eq!(parse_set_interval(r#"{"interval_secs":1}"#).unwrap(), 1);
        assert_eq!(parse_set_interval(r#"{"interval_secs":0}"#).unwrap(), 1);
        assert_eq!(parse_set_interval(r#"{"interval_secs":99}"#).unwrap(), 60);
        assert_eq!(parse_set_interval("{}").unwrap(), 1);
        assert_eq!(parse_set_interval("").unwrap(), 1);
    }

    #[test]
    fn set_runtime_parses() {
        let rt = parse_set_runtime(
            r#"{"interval_secs":2,"docker_enabled":true,"docker_manage":true,"labels":{"role":"lab"}}"#,
        )
        .unwrap();
        assert_eq!(rt.interval_secs, 2);
        assert!(rt.docker_enabled);
        assert!(rt.docker_manage);
        assert_eq!(rt.labels.get("role").map(String::as_str), Some("lab"));
        let clamped = parse_set_runtime(r#"{"interval_secs":99}"#).unwrap();
        assert_eq!(clamped.interval_secs, 60);
    }
}
