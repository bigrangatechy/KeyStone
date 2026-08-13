// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::pin::Pin;

use keystone_core::node::NodeIdentity;
use keystone_core::sample::{self, Label, Sample};
use keystone_proto::ingest_server::{Ingest, IngestServer};
use keystone_proto::{
    agent_to_server, server_to_agent, Ack, AgentToServer, Heartbeat, PushFrame, ServerToAgent,
};
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tracing::{info, warn};

use crate::state::AppState;

pub fn service(state: AppState) -> IngestServer<IngestSvc> {
    IngestServer::new(IngestSvc { state })
}

pub struct IngestSvc {
    state: AppState,
}

#[tonic::async_trait]
impl Ingest for IngestSvc {
    type SessionStream = Pin<Box<dyn Stream<Item = Result<ServerToAgent, Status>> + Send>>;

    async fn session(
        &self,
        request: Request<Streaming<AgentToServer>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        let mut inbound = request.into_inner();
        let (out_tx, out_rx) = mpsc::channel::<Result<ServerToAgent, Status>>(64);
        let (cmd_tx, mut cmd_rx) = mpsc::channel(32);
        let state = self.state.clone();

        tokio::spawn(async move {
            let mut node_id: Option<String> = None;
            loop {
                tokio::select! {
                    msg = inbound.next() => {
                        match msg {
                            Some(Ok(AgentToServer { body: Some(agent_to_server::Body::Push(frame)) })) => {
                                match handle_push(&state, &frame) {
                                    Ok(id) => {
                                        if node_id.as_deref() != Some(id.as_str()) {
                                            if let Some(old) = node_id.take() {
                                                state.agents.disconnect(&old);
                                                let _ = state.stores.metadata.set_online(&old, false);
                                            }
                                            state.agents.connect(id.clone(), cmd_tx.clone());
                                            node_id = Some(id.clone());
                                            info!("agent session {id}");
                                        }
                                        let _ = out_tx.send(Ok(ServerToAgent {
                                            body: Some(server_to_agent::Body::Ack(Ack {
                                                ok: true,
                                                error: String::new(),
                                            })),
                                        })).await;
                                    }
                                    Err(e) => {
                                        warn!("push rejected: {e}");
                                        let _ = out_tx.send(Ok(ServerToAgent {
                                            body: Some(server_to_agent::Body::Ack(Ack {
                                                ok: false,
                                                error: e.to_string(),
                                            })),
                                        })).await;
                                    }
                                }
                            }
                            Some(Ok(AgentToServer { body: Some(agent_to_server::Body::Result(result)) })) => {
                                if let Some(id) = &node_id {
                                    state.agents.complete(id, result);
                                }
                            }
                            Some(Ok(AgentToServer { body: Some(agent_to_server::Body::Chunk(_)) })) => {}
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                warn!("agent stream error: {e}");
                                break;
                            }
                            None => break,
                        }
                    }
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(command) => {
                                if out_tx.send(Ok(ServerToAgent {
                                    body: Some(server_to_agent::Body::Command(command)),
                                })).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            if let Some(id) = node_id {
                state.agents.disconnect(&id);
                let _ = state.stores.metadata.set_online(&id, false);
                info!("agent disconnected {id}");
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(out_rx))))
    }
}

fn handle_push(state: &AppState, frame: &PushFrame) -> anyhow::Result<String> {
    if !state.config.ingest_token.is_empty() && frame.ingest_token != state.config.ingest_token {
        anyhow::bail!("invalid ingest token");
    }
    let hb = frame
        .heartbeat
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing heartbeat"))?;
    if hb.node_id.is_empty() {
        anyhow::bail!("empty node_id");
    }
    let identity = identity_from_heartbeat(hb);
    state.stores.metadata.upsert_heartbeat(&identity, true)?;
    let samples: Vec<Sample> = frame
        .samples
        .iter()
        .map(|s| Sample {
            metric: s.metric.clone(),
            labels: s
                .labels
                .iter()
                .map(|l| Label {
                    name: l.name.clone(),
                    value: l.value.clone(),
                })
                .collect(),
            value: s.value,
            timestamp_unix_ms: s.timestamp_unix_ms,
        })
        .collect();
    let (kept, dropped) = sample::allowlist(samples);
    if dropped > 0 {
        tracing::debug!("dropped {dropped} unknown metrics from {}", hb.node_id);
    }
    state.stores.series.write_samples(&hb.node_id, &kept)?;
    Ok(hb.node_id.clone())
}

fn identity_from_heartbeat(hb: &Heartbeat) -> NodeIdentity {
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
