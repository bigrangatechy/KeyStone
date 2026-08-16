// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::pin::Pin;

use keystone_core::node::NodeIdentity;
use keystone_core::sample::{self, Label, Sample};
use keystone_core::NodeSettings;
use keystone_proto::ingest_server::{Ingest, IngestServer};
use keystone_proto::{
    agent_to_server, server_to_agent, Ack, AgentToServer, Heartbeat, PushFrame, ServerToAgent,
};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
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
        let (out_tx, out_rx) = mpsc::channel::<Result<ServerToAgent, Status>>(128);
        let (cmd_tx, mut cmd_rx) = mpsc::channel(128);
        // Commands/Acks must not share the select with inbound. Awaiting
        // gRPC write here meant we stopped reading Results; pipelined
        // Docker/System lists then saw every oneshot dropped.
        let (to_agent_tx, mut to_agent_rx) = mpsc::channel::<ServerToAgent>(128);
        let (writer_dead_tx, mut writer_dead_rx) = tokio::sync::oneshot::channel::<()>();
        let state = self.state.clone();

        tokio::spawn(async move {
            while let Some(msg) = to_agent_rx.recv().await {
                if out_tx.send(Ok(msg)).await.is_err() {
                    break;
                }
            }
            let _ = writer_dead_tx.send(());
        });

        tokio::spawn(async move {
            let mut node_id: Option<String> = None;
            let mut session_gen: Option<u64> = None;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut writer_dead_rx => break,
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(command) => {
                                if !queue_to_agent(&to_agent_tx, ServerToAgent {
                                    body: Some(server_to_agent::Body::Command(command)),
                                }) {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                    msg = inbound.next() => {
                        match msg {
                            Some(Ok(AgentToServer { body: Some(agent_to_server::Body::Push(frame)) })) => {
                                match handle_push(&state, &frame) {
                                    Ok(id) => {
                                        if node_id.as_deref() != Some(id.as_str()) {
                                            if let (Some(old), Some(gen)) = (node_id.take(), session_gen.take()) {
                                                if state.agents.disconnect(&old, gen) {
                                                    let _ = state.stores.metadata.set_online(&old, false);
                                                }
                                            }
                                            let gen = state.agents.connect(id.clone(), cmd_tx.clone());
                                            session_gen = Some(gen);
                                            let settings = NodeSettings::parse_or_default(
                                                state
                                                    .stores
                                                    .metadata
                                                    .node_settings_json(&id)
                                                    .ok()
                                                    .flatten()
                                                    .as_deref(),
                                            );
                                            state.agents.nudge_runtime(&id, &settings);
                                            node_id = Some(id.clone());
                                            info!("agent session {id}");
                                        }
                                        if !queue_to_agent(&to_agent_tx, ack(true, String::new())) {
                                            break;
                                        }
                                        while let Ok(command) = cmd_rx.try_recv() {
                                            if !queue_to_agent(&to_agent_tx, ServerToAgent {
                                                body: Some(server_to_agent::Body::Command(command)),
                                            }) {
                                                break;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("push rejected: {e}");
                                        if !queue_to_agent(&to_agent_tx, ack(false, e.to_string())) {
                                            break;
                                        }
                                    }
                                }
                            }
                            Some(Ok(AgentToServer { body: Some(agent_to_server::Body::Result(result)) })) => {
                                if let Some(id) = &node_id {
                                    state.agents.complete(id, result);
                                }
                            }
                            Some(Ok(AgentToServer { body: Some(agent_to_server::Body::Chunk(chunk)) })) => {
                                if let Some(id) = &node_id {
                                    state.agents.push_chunk(id, chunk);
                                }
                            }
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                warn!("agent stream error: {e}");
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
            if let (Some(id), Some(gen)) = (node_id, session_gen) {
                if state.agents.disconnect(&id, gen) {
                    let _ = state.stores.metadata.set_online(&id, false);
                    info!("agent disconnected {id}");
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(out_rx))))
    }
}

fn ack(ok: bool, error: String) -> ServerToAgent {
    ServerToAgent {
        body: Some(server_to_agent::Body::Ack(Ack { ok, error })),
    }
}

/// Never `.await` outbound on the inbound select. A full gRPC window used to
/// stall Result reads so every node-page RPC hit the 8s timeout.
fn queue_to_agent(tx: &mpsc::Sender<ServerToAgent>, msg: ServerToAgent) -> bool {
    match tx.try_send(msg) {
        Ok(()) => true,
        Err(TrySendError::Closed(_)) => false,
        Err(TrySendError::Full(msg)) => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(msg).await;
            });
            true
        }
    }
}

fn handle_push(state: &AppState, frame: &PushFrame) -> anyhow::Result<String> {
    let token = state.ingest_token();
    if !token.is_empty() && frame.ingest_token != token {
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
    crate::alerts::note_samples(state, &hb.node_id, &kept);
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

#[cfg(test)]
mod tests {
    use super::*;
    use keystone_core::config::ServerConfig;
    use keystone_proto::ingest_client::IngestClient;
    use keystone_store::Stores;
    use tokio::net::TcpListener;
    use tokio::time::{sleep, Duration};
    use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
    use tokio_stream::StreamExt;
    use tonic::transport::Server;

    fn scratch(token: &str) -> (std::path::PathBuf, AppState) {
        let dir = std::env::temp_dir().join(format!("ks-ing-{}", uuid::Uuid::new_v4()));
        let stores = Stores::open(&dir, 24).unwrap();
        let cfg = ServerConfig {
            data_dir: dir.to_string_lossy().into(),
            ingest_token: token.into(),
            ..ServerConfig::default()
        };
        let state = AppState::for_test(cfg, stores);
        state.seed_server_settings().unwrap();
        (dir, state)
    }

    fn push(node: &str, token: &str, samples: Vec<keystone_proto::Sample>) -> PushFrame {
        PushFrame {
            heartbeat: Some(Heartbeat {
                node_id: node.into(),
                hostname: node.into(),
                agent_version: "test".into(),
                os: "linux".into(),
                kernel: "test".into(),
                docker_version: String::new(),
                labels: vec![],
            }),
            samples,
            ingest_token: token.into(),
        }
    }

    fn cpu_sample() -> keystone_proto::Sample {
        keystone_proto::Sample {
            metric: "node_cpu_usage_ratio".into(),
            labels: vec![],
            value: 0.42,
            timestamp_unix_ms: 1,
        }
    }

    #[test]
    fn add_node_then_matching_push_clears_awaiting() {
        let (dir, state) = scratch("lab-token");
        state
            .stores
            .metadata
            .register_node("lab-pi", "lab-pi", "[]")
            .unwrap();
        assert!(state
            .stores
            .metadata
            .get_node("lab-pi")
            .unwrap()
            .unwrap()
            .awaiting_agent());
        handle_push(&state, &push("lab-pi", "lab-token", vec![cpu_sample()])).unwrap();
        let node = state.stores.metadata.get_node("lab-pi").unwrap().unwrap();
        assert!(!node.awaiting_agent());
        let latest = state.stores.series.latest_samples("lab-pi").unwrap();
        assert!(latest.iter().any(|s| s.metric == "node_cpu_usage_ratio"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_agent_with_token_enrolls_without_form() {
        let (dir, state) = scratch("lab-token");
        handle_push(&state, &push("ubuntu-box", "lab-token", vec![cpu_sample()])).unwrap();
        let node = state
            .stores
            .metadata
            .get_node("ubuntu-box")
            .unwrap()
            .expect("auto-enroll");
        assert!(!node.awaiting_agent());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn wrong_token_does_not_enroll() {
        let (dir, state) = scratch("lab-token");
        let err = handle_push(&state, &push("stranger", "nope", vec![cpu_sample()])).unwrap_err();
        assert!(err.to_string().contains("invalid ingest token"));
        assert!(state
            .stores
            .metadata
            .get_node("stranger")
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn catalog_keeps_allowlisted_and_drops_unknown() {
        let (dir, state) = scratch("lab-token");
        handle_push(
            &state,
            &push(
                "lab-pi",
                "lab-token",
                vec![
                    cpu_sample(),
                    keystone_proto::Sample {
                        metric: "totally_fake_metric".into(),
                        labels: vec![],
                        value: 9.0,
                        timestamp_unix_ms: 1,
                    },
                ],
            ),
        )
        .unwrap();
        let latest = state.stores.series.latest_samples("lab-pi").unwrap();
        assert!(latest.iter().any(|s| s.metric == "node_cpu_usage_ratio"));
        assert!(!latest.iter().any(|s| s.metric == "totally_fake_metric"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Loopback gRPC session: what a LAN agent does after it has an ingest URL.
    #[tokio::test]
    async fn grpc_session_enrolls_on_matching_token() {
        let (dir, state) = scratch("lab-token");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let incoming = TcpListenerStream::new(listener);
        let serve_state = state.clone();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(service(serve_state))
                .serve_with_incoming(incoming)
                .await
        });
        let mut client = None;
        for _ in 0..50 {
            if let Ok(c) = IngestClient::connect(format!("http://{addr}")).await {
                client = Some(c);
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        let mut client = client.expect("dial ingest");
        let (tx, rx) = mpsc::channel(4);
        let mut inbound = client
            .session(ReceiverStream::new(rx))
            .await
            .expect("session")
            .into_inner();
        tx.send(AgentToServer {
            body: Some(agent_to_server::Body::Push(push(
                "grpc-node",
                "lab-token",
                vec![cpu_sample()],
            ))),
        })
        .await
        .unwrap();
        let msg = inbound.next().await.expect("ack").expect("ok status");
        match msg.body {
            Some(server_to_agent::Body::Ack(ack)) => assert!(ack.ok, "{}", ack.error),
            other => panic!("expected ack, got {other:?}"),
        }
        drop(tx);
        let node = state
            .stores
            .metadata
            .get_node("grpc-node")
            .unwrap()
            .expect("enrolled over gRPC");
        assert!(!node.awaiting_agent());
        server.abort();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ingest_writes_commands_off_the_inbound_select() {
        let src = include_str!("ingest.rs");
        assert!(
            src.contains("to_agent_tx"),
            "Acks/Commands must not await the gRPC sink on the inbound select"
        );
        assert!(
            src.contains("to_agent_rx.recv()"),
            "a side writer must drain Commands so Results still complete"
        );
        assert!(
            src.contains("queue_to_agent") && src.contains("try_send"),
            "inbound select must not await Ack/Command enqueue"
        );
        assert!(
            src.contains("biased"),
            "Commands must win over a burst of Pushes so the node page does not wait 8s"
        );
        assert!(
            src.contains("writer_dead"),
            "a dead gRPC sink must drop the session instead of queueing Commands into the void"
        );
        assert!(
            src.contains("try_recv"),
            "replayed Commands after connect must flush before the next Push"
        );
    }

    #[tokio::test]
    async fn grpc_session_roundtrips_a_command() {
        let (dir, state) = scratch("lab-token");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let incoming = TcpListenerStream::new(listener);
        let serve_state = state.clone();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(service(serve_state))
                .serve_with_incoming(incoming)
                .await
        });
        let mut client = None;
        for _ in 0..50 {
            if let Ok(c) = IngestClient::connect(format!("http://{addr}")).await {
                client = Some(c);
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        let mut client = client.expect("dial ingest");
        let (tx, rx) = mpsc::channel(8);
        let mut inbound = client
            .session(ReceiverStream::new(rx))
            .await
            .expect("session")
            .into_inner();
        tx.send(AgentToServer {
            body: Some(agent_to_server::Body::Push(push(
                "grpc-cmd",
                "lab-token",
                vec![cpu_sample()],
            ))),
        })
        .await
        .unwrap();
        let ack = inbound.next().await.expect("ack").expect("ok status");
        match ack.body {
            Some(server_to_agent::Body::Ack(ack)) => assert!(ack.ok, "{}", ack.error),
            other => panic!("expected ack, got {other:?}"),
        }
        let wait = tokio::spawn({
            let state = state.clone();
            async move {
                state
                    .agents
                    .call_timeout(
                        "grpc-cmd",
                        "container_list",
                        "{}".into(),
                        Duration::from_secs(2),
                    )
                    .await
            }
        });
        let started = std::time::Instant::now();
        loop {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "command must reach the agent well inside the page wait"
            );
            let msg = tokio::time::timeout(Duration::from_secs(2), inbound.next())
                .await
                .expect("command within 2s")
                .expect("stream")
                .expect("ok status");
            let Some(server_to_agent::Body::Command(cmd)) = msg.body else {
                continue;
            };
            tx.send(AgentToServer {
                body: Some(agent_to_server::Body::Result(
                    keystone_proto::CommandResult {
                        request_id: cmd.request_id,
                        ok: true,
                        payload_json: "[]".into(),
                        error: String::new(),
                    },
                )),
            })
            .await
            .unwrap();
            if cmd.op == "container_list" {
                break;
            }
        }
        let result = wait.await.expect("join").expect("oneshot");
        assert!(result.ok, "{}", result.error);
        drop(tx);
        server.abort();
        let _ = std::fs::remove_dir_all(dir);
    }
}
