// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use keystone_core::config::AgentConfig;
use keystone_core::docker::DockerOp;
use keystone_core::node::NodeIdentity;
use keystone_core::sample::{self, Label, Sample};
use keystone_core::NodeSettings;
use keystone_proto::ingest_client::IngestClient;
use keystone_proto::{
    agent_to_server, server_to_agent, Ack, AgentToServer, CommandResult, Heartbeat, PushFrame,
    ServerToAgent,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Endpoint;
use tracing::{info, warn};

use crate::buffer::DiskBuffer;
use crate::collect::collect_host;
use crate::docker::DockerHandle;

pub async fn run(cfg: AgentConfig) -> anyhow::Result<()> {
    let node_id = if cfg.node_id.is_empty() {
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".into())
    } else {
        cfg.node_id.clone()
    };

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

    let buffer = DiskBuffer::new(&cfg.buffer_dir)?;
    let endpoint = Endpoint::from_shared(cfg.ingest_url.clone())?
        .connect_timeout(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(30));

    loop {
        match connect_session(&cfg, &node_id, docker.as_ref(), &buffer, endpoint.clone()).await {
            Ok(()) => info!("ingest session ended, reconnecting"),
            Err(e) => warn!("ingest session error: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn connect_session(
    cfg: &AgentConfig,
    node_id: &str,
    docker: Option<&DockerHandle>,
    buffer: &DiskBuffer,
    endpoint: Endpoint,
) -> anyhow::Result<()> {
    let channel = endpoint.connect().await.context("connect ingest")?;
    let mut client = IngestClient::new(channel);
    let (tx, rx) = mpsc::channel::<AgentToServer>(64);
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
    let docker_push = docker.is_some();

    let mut interval = tokio::time::interval(Duration::from_secs(cfg.interval_secs.max(1)));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let mut samples = collect_host(cfg);
                if docker_push {
                    if let Some(d) = docker {
                        samples.extend(d.collect_container_metrics().await);
                    }
                }
                let (kept, dropped) = sample::allowlist(samples);
                if dropped > 0 {
                    warn!("dropped {dropped} unknown metrics");
                }
                let frame = build_push(cfg, &node_id_owned, docker, &kept).await;
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
                        let result = if cmd.op == "set_interval" {
                            match parse_set_interval(&cmd.payload_json) {
                                Ok(secs) => {
                                    interval = tokio::time::interval(Duration::from_secs(secs));
                                    interval.set_missed_tick_behavior(
                                        tokio::time::MissedTickBehavior::Delay,
                                    );
                                    info!("push interval set to {secs}s");
                                    Ok(serde_json::json!({ "interval_secs": secs }))
                                }
                                Err(e) => Err(e),
                            }
                        } else {
                            handle_command(docker, &cmd.op, &cmd.payload_json).await
                        };
                        let body = match result {
                            Ok(payload) => CommandResult {
                                request_id: cmd.request_id,
                                ok: true,
                                payload_json: payload.to_string(),
                                error: String::new(),
                            },
                            Err(e) => CommandResult {
                                request_id: cmd.request_id,
                                ok: false,
                                payload_json: String::new(),
                                error: e.to_string(),
                            },
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

fn handle_ack(ack: &Ack) {
    if !ack.ok {
        warn!("ingest nack: {}", ack.error);
    }
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

async fn handle_command(
    docker: Option<&DockerHandle>,
    op: &str,
    payload_json: &str,
) -> anyhow::Result<serde_json::Value> {
    let Some(docker) = docker else {
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
    let labels: Vec<keystone_proto::Label> = cfg
        .labels
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
}
