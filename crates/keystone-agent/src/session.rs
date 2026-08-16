// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use keystone_core::config::{ingest_tls_domain, AgentConfig, DockerConfig};
use keystone_core::docker::DockerOp;
use keystone_core::node::NodeIdentity;
use keystone_core::sample::{self, Label, Sample};
use keystone_core::sys::SysOp;
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
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint};
use tracing::{info, warn};

use crate::buffer::DiskBuffer;
use crate::collect::collect_host;
use crate::docker::DockerHandle;

struct AgentRuntimeState {
    docker: Mutex<Option<DockerHandle>>,
    labels: Mutex<BTreeMap<String, String>>,
    docker_host: String,
    docker_version: Mutex<Option<String>>,
    sys_enabled: Mutex<bool>,
    sys_manage: Mutex<bool>,
    /// Node-page lists (Docker + System status). push_tick skips
    /// `docker stats` while this is non-zero so those RPCs can finish
    /// inside the server's 8s budget.
    list_inflight: AtomicUsize,
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
            docker_version: Mutex::new(None),
            sys_enabled: Mutex::new(false),
            sys_manage: Mutex::new(false),
            list_inflight: AtomicUsize::new(0),
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
    let buffer = Arc::new(DiskBuffer::new(&cfg.buffer_dir)?);
    let _ = rustls::crypto::ring::default_provider().install_default();

    loop {
        let ingest_url = match resolve_ingest_url(&cfg).await {
            Ok(u) => u,
            Err(e) => {
                warn!("{e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let endpoint = match ingest_endpoint(&cfg, &ingest_url) {
            Ok(e) => e,
            Err(e) => {
                warn!("ingest endpoint: {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        match connect_session(
            &cfg,
            &ingest_url,
            &node_id,
            runtime.clone(),
            buffer.clone(),
            endpoint,
        )
        .await
        {
            Ok(()) => info!("ingest session ended, reconnecting"),
            Err(e) => warn!("ingest session error: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn resolve_ingest_url(cfg: &AgentConfig) -> anyhow::Result<String> {
    if keystone_core::wants_mdns(&cfg.ingest_url) {
        let url = crate::mdns::discover_ingest_url().await?;
        info!("mDNS found ingest at {url}");
        Ok(url)
    } else {
        Ok(cfg.ingest_url.clone())
    }
}

fn ingest_endpoint(cfg: &AgentConfig, ingest_url: &str) -> anyhow::Result<Endpoint> {
    if keystone_core::wants_mdns(ingest_url) {
        anyhow::bail!("ingest_url is mDNS; resolve to http(s):// first");
    }
    let mut endpoint = Endpoint::from_shared(ingest_url.to_string())?
        .connect_timeout(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(30));
    if ingest_url.starts_with("https://") {
        let domain = ingest_tls_domain(ingest_url).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut tls = ClientTlsConfig::new().domain_name(domain);
        if cfg.tls_ca_file.trim().is_empty() {
            tls = tls.with_webpki_roots();
        } else {
            let pem = std::fs::read(cfg.tls_ca_file.trim())
                .with_context(|| format!("tls_ca_file {}", cfg.tls_ca_file))?;
            tls = tls.ca_certificate(Certificate::from_pem(pem));
        }
        endpoint = endpoint.tls_config(tls).context("ingest TLS")?;
    } else if !cfg.tls_ca_file.trim().is_empty() {
        warn!("tls_ca_file is set but ingest_url is not https://; ignoring CA file");
    }
    Ok(endpoint)
}

async fn push_tick(
    cfg: &AgentConfig,
    node_id: &str,
    runtime: &AgentRuntimeState,
    push_tx: &mpsc::Sender<AgentToServer>,
    buffer: &DiskBuffer,
) -> anyhow::Result<()> {
    let mut samples = collect_host(cfg);
    let docker = runtime.docker.lock().await.clone();
    if let Some(d) = docker.as_ref() {
        if runtime.list_inflight.load(Ordering::SeqCst) == 0 {
            samples.extend(d.collect_container_metrics().await);
        }
    }
    let (kept, dropped) = sample::allowlist(samples);
    if dropped > 0 {
        warn!("dropped {dropped} unknown metrics");
    }
    let labels = runtime.labels.lock().await.clone();
    let docker_version = cached_engine_version(runtime, docker.as_ref()).await;
    let frame = build_push(cfg, node_id, docker_version, &labels, &kept);
    if push_tx
        .send(AgentToServer {
            body: Some(agent_to_server::Body::Push(frame.clone())),
        })
        .await
        .is_err()
    {
        buffer.push(&frame)?;
    }
    Ok(())
}

async fn connect_session(
    cfg: &AgentConfig,
    ingest_url: &str,
    node_id: &str,
    runtime: Arc<AgentRuntimeState>,
    buffer: Arc<DiskBuffer>,
    endpoint: Endpoint,
) -> anyhow::Result<()> {
    let channel = endpoint.connect().await.context("connect ingest")?;
    let mut client = IngestClient::new(channel);
    let (tx, rx) = mpsc::channel::<AgentToServer>(256);
    let response = client.session(ReceiverStream::new(rx)).await?;
    let mut inbound = response.into_inner();

    info!("connected to ingest at {ingest_url}");

    if let Ok(frames) = buffer.drain() {
        let buf_tx = tx.clone();
        tokio::spawn(async move {
            for frame in frames {
                if buf_tx
                    .send(AgentToServer {
                        body: Some(agent_to_server::Body::Push(frame)),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    let push_tx = tx.clone();
    let node_id_owned = node_id.to_string();
    let cancels: Arc<std::sync::Mutex<HashMap<String, AbortHandle>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let pushing = Arc::new(AtomicBool::new(false));

    let mut interval = tokio::time::interval(Duration::from_secs(cfg.interval_secs.max(1)));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                // Collect off the session loop. Per-container docker stats
                // can take seconds; blocking here means the agent never
                // reads Commands and the node page waits until restart.
                if pushing
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    continue;
                }
                let pushing = pushing.clone();
                let runtime = runtime.clone();
                let push_tx = push_tx.clone();
                let buffer = buffer.clone();
                let cfg = cfg.clone();
                let node_id_owned = node_id_owned.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        push_tick(&cfg, &node_id_owned, &runtime, &push_tx, &buffer).await
                    {
                        warn!("push tick failed: {e}");
                    }
                    pushing.store(false, Ordering::SeqCst);
                });
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
                            reply(
                                &tx,
                                CommandResult {
                                    request_id: cmd.request_id,
                                    ok: true,
                                    payload_json: "{}".into(),
                                    error: String::new(),
                                },
                            );
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
                            reply(&tx, body);
                            continue;
                        }
                        if cmd.op == "set_runtime" {
                            match parse_set_runtime(&cmd.payload_json) {
                                Ok(rt) => {
                                    apply_interval(&mut interval, rt.interval_secs);
                                    spawn_set_runtime(
                                        runtime.clone(),
                                        tx.clone(),
                                        cmd.request_id,
                                        rt,
                                    );
                                }
                                Err(e) => reply(&tx, command_err(cmd.request_id, e)),
                            }
                            continue;
                        }
                        let streaming = DockerOp::from_str(&cmd.op)
                            .ok()
                            .is_some_and(DockerOp::streams)
                            || SysOp::from_str(&cmd.op).ok().is_some_and(SysOp::streams);
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
                        spawn_rpc_command(
                            runtime.clone(),
                            tx.clone(),
                            cmd.request_id,
                            cmd.op,
                            cmd.payload_json,
                        );
                    }
                    Ok(Some(other)) => {
                        warn!("ignored server message: {:?}", other.body);
                    }
                    Ok(None) => break,
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

fn reply(tx: &mpsc::Sender<AgentToServer>, body: CommandResult) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let _ = tx
            .send(AgentToServer {
                body: Some(agent_to_server::Body::Result(body)),
            })
            .await;
    });
}

fn cancel_target(payload_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(payload_json)
        .ok()
        .and_then(|v| v.get("request_id")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Ops the node page waits 8s for. Must reply sooner than that even if
/// Docker or `ip` is slow, otherwise the UI shows agent command timed out.
fn is_page_list_op(op: &str) -> bool {
    matches!(
        op,
        "container_list" | "compose_ps" | "image_list" | "volume_list" | "network_list" | "status"
    )
}

const PAGE_RPC_BUDGET: Duration = Duration::from_secs(6);

fn spawn_rpc_command(
    runtime: Arc<AgentRuntimeState>,
    tx: mpsc::Sender<AgentToServer>,
    request_id: String,
    op: String,
    payload_json: String,
) {
    tokio::spawn(async move {
        let page_list = is_page_list_op(&op);
        if page_list {
            runtime.list_inflight.fetch_add(1, Ordering::SeqCst);
        }
        let result = if page_list {
            match tokio::time::timeout(
                PAGE_RPC_BUDGET,
                handle_command(&runtime, &op, &payload_json),
            )
            .await
            {
                Ok(r) => r,
                Err(_) => {
                    warn!("{op} exceeded {PAGE_RPC_BUDGET:?} on the agent");
                    Err(anyhow::anyhow!("{op} timed out on the agent"))
                }
            }
        } else {
            handle_command(&runtime, &op, &payload_json).await
        };
        if page_list {
            runtime.list_inflight.fetch_sub(1, Ordering::SeqCst);
        }
        let body = match result {
            Ok(payload) => CommandResult {
                request_id: request_id.clone(),
                ok: true,
                payload_json: payload.to_string(),
                error: String::new(),
            },
            Err(e) => command_err(request_id, e),
        };
        let _ = tx
            .send(AgentToServer {
                body: Some(agent_to_server::Body::Result(body)),
            })
            .await;
    });
}

fn spawn_set_runtime(
    runtime: Arc<AgentRuntimeState>,
    tx: mpsc::Sender<AgentToServer>,
    request_id: String,
    rt: AgentRuntime,
) {
    tokio::spawn(async move {
        let body = match apply_runtime_state(&runtime, rt).await {
            Ok(v) => CommandResult {
                request_id: request_id.clone(),
                ok: true,
                payload_json: v.to_string(),
                error: String::new(),
            },
            Err(e) => command_err(request_id, e),
        };
        let _ = tx
            .send(AgentToServer {
                body: Some(agent_to_server::Body::Result(body)),
            })
            .await;
    });
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
    let result = if let Ok(sys_op) = SysOp::from_str(op) {
        run_sys_streaming(runtime, sys_op, payload, chunk_tx).await
    } else {
        let docker = runtime
            .docker
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow::anyhow!("docker is not enabled on this agent"))?;
        let docker_op =
            DockerOp::from_str(op).map_err(|_| anyhow::anyhow!("unknown docker op {op}"))?;
        docker.execute_streaming(docker_op, payload, chunk_tx).await
    };
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

async fn run_sys_streaming(
    runtime: &AgentRuntimeState,
    op: SysOp,
    payload: serde_json::Value,
    chunk_tx: mpsc::Sender<Vec<u8>>,
) -> anyhow::Result<serde_json::Value> {
    if !*runtime.sys_enabled.lock().await {
        anyhow::bail!("system observe is off on this node");
    }
    if op.mutating() && !*runtime.sys_manage.lock().await {
        anyhow::bail!("system manage is disabled on this agent");
    }
    crate::sys::stream(op, payload, chunk_tx).await
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

async fn apply_runtime_state(
    runtime: &AgentRuntimeState,
    rt: AgentRuntime,
) -> anyhow::Result<serde_json::Value> {
    {
        let mut labels = runtime.labels.lock().await;
        *labels = rt.labels.clone();
    }
    if rt.docker_enabled {
        let already = runtime.docker.lock().await.is_some();
        if already {
            if let Some(handle) = runtime.docker.lock().await.as_ref() {
                handle.set_policy(
                    rt.docker_manage,
                    rt.docker_allow_exec,
                    rt.compose_paths.clone(),
                );
            }
        } else {
            let cfg = DockerConfig {
                enabled: true,
                manage: rt.docker_manage,
                allow_exec: rt.docker_allow_exec,
                host: runtime.docker_host.clone(),
                compose_paths: rt.compose_paths.clone(),
            };
            // Connect without holding the mutex so container_list can run.
            let connected = DockerHandle::connect(&cfg);
            let mut docker = runtime.docker.lock().await;
            match connected {
                Ok(handle) => {
                    if docker.is_none() {
                        info!("docker enabled from Settings");
                        *docker = Some(handle);
                        *runtime.docker_version.lock().await = None;
                    } else if let Some(h) = docker.as_ref() {
                        h.set_policy(
                            rt.docker_manage,
                            rt.docker_allow_exec,
                            rt.compose_paths.clone(),
                        );
                    }
                }
                Err(e) => {
                    warn!("docker enable failed: {e}");
                }
            }
        }
    } else {
        let mut docker = runtime.docker.lock().await;
        if docker.is_some() {
            info!("docker disabled from Settings");
            *docker = None;
            *runtime.docker_version.lock().await = None;
        }
    }
    {
        let mut enabled = runtime.sys_enabled.lock().await;
        *enabled = rt.sys_enabled;
        let mut manage = runtime.sys_manage.lock().await;
        *manage = rt.sys_enabled && rt.sys_manage;
    }
    Ok(serde_json::to_value(&rt)?)
}

async fn handle_command(
    runtime: &AgentRuntimeState,
    op: &str,
    payload_json: &str,
) -> anyhow::Result<serde_json::Value> {
    let payload = if payload_json.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(payload_json).context("payload json")?
    };
    if let Ok(sys_op) = SysOp::from_str(op) {
        return handle_sys(runtime, sys_op, payload).await;
    }
    let docker = runtime.docker.lock().await.clone();
    let Some(docker) = docker else {
        anyhow::bail!("docker is not enabled on this agent");
    };
    let op = DockerOp::from_str(op).map_err(|_| anyhow::anyhow!("unknown docker op {op}"))?;
    docker.execute(op, payload).await
}

async fn handle_sys(
    runtime: &AgentRuntimeState,
    op: SysOp,
    payload: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    if !*runtime.sys_enabled.lock().await {
        anyhow::bail!("system observe is off on this node");
    }
    if op.mutating() && !*runtime.sys_manage.lock().await {
        anyhow::bail!("system manage is disabled on this agent");
    }
    match op {
        SysOp::Status => {
            let helper_fut = async {
                if crate::sys::socket_present() {
                    Some(crate::sys::call(SysOp::Status, serde_json::json!({})).await)
                } else {
                    None
                }
            };
            let (mut local, helper) = tokio::join!(crate::sys::local_status(), helper_fut);
            match helper {
                Some(Ok(helper)) => {
                    if let Some(obj) = local.as_object_mut() {
                        if let Some(h) = helper.as_object() {
                            for (k, v) in h {
                                obj.insert(k.clone(), v.clone());
                            }
                        }
                        obj.insert("helper_running".into(), serde_json::json!(true));
                    }
                }
                Some(Err(e)) => {
                    if let Some(obj) = local.as_object_mut() {
                        obj.insert("helper_error".into(), serde_json::json!(e.to_string()));
                        obj.insert("helper_running".into(), serde_json::json!(false));
                    }
                }
                None => {}
            }
            Ok(local)
        }
        SysOp::UpdatesList | SysOp::NetSet => crate::sys::call(op, payload).await,
        SysOp::UpdatesApply => anyhow::bail!("updates_apply is streamed from the apply page"),
    }
}

async fn cached_engine_version(
    runtime: &AgentRuntimeState,
    docker: Option<&DockerHandle>,
) -> String {
    {
        let cached = runtime.docker_version.lock().await;
        if let Some(v) = cached.as_ref() {
            return v.clone();
        }
    }
    if runtime.list_inflight.load(Ordering::SeqCst) > 0 {
        return String::new();
    }
    let Some(d) = docker else {
        return String::new();
    };
    let v = d.engine_version().await.unwrap_or_default();
    *runtime.docker_version.lock().await = Some(v.clone());
    v
}

fn build_push(
    cfg: &AgentConfig,
    node_id: &str,
    docker_version: String,
    labels: &BTreeMap<String, String>,
    samples: &[Sample],
) -> PushFrame {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| node_id.to_string());
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
        assert!(!rt.sys_enabled);
        let clamped = parse_set_runtime(r#"{"interval_secs":99}"#).unwrap();
        assert_eq!(clamped.interval_secs, 60);
    }

    #[tokio::test]
    async fn sys_observe_off_refuses() {
        let runtime = AgentRuntimeState {
            docker: Mutex::new(None),
            labels: Mutex::new(BTreeMap::new()),
            docker_host: String::new(),
            docker_version: Mutex::new(None),
            sys_enabled: Mutex::new(false),
            sys_manage: Mutex::new(false),
            list_inflight: AtomicUsize::new(0),
        };
        let err = handle_sys(&runtime, SysOp::Status, serde_json::json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("observe"), "{err}");
    }

    #[tokio::test]
    async fn sys_manage_off_refuses_mutating() {
        let runtime = AgentRuntimeState {
            docker: Mutex::new(None),
            labels: Mutex::new(BTreeMap::new()),
            docker_host: String::new(),
            docker_version: Mutex::new(None),
            sys_enabled: Mutex::new(true),
            sys_manage: Mutex::new(false),
            list_inflight: AtomicUsize::new(0),
        };
        let err = handle_sys(&runtime, SysOp::NetSet, serde_json::json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("manage"), "{err}");
    }

    #[test]
    fn sys_ops_are_not_docker_ops() {
        for op in ["status", "updates_list", "updates_apply", "net_set"] {
            assert!(
                DockerOp::from_str(op).is_err(),
                "sys op {op} must not parse as DockerOp"
            );
        }
        assert!(DockerOp::from_str("updates_apply").is_err());
        assert!(SysOp::from_str("compose_update").is_err());
        assert!(SysOp::from_str("compose_pull").is_err());
    }

    #[test]
    fn mdns_sentinel_is_not_a_grpc_endpoint() {
        let cfg = AgentConfig::default();
        assert!(
            ingest_endpoint(&cfg, "mdns").is_err(),
            "must resolve mDNS to http(s):// before building an Endpoint"
        );
        assert!(ingest_endpoint(&cfg, "http://127.0.0.1:9100").is_ok());
    }

    #[test]
    fn push_tick_is_off_the_session_loop() {
        let src = include_str!("session.rs");
        assert!(
            src.contains("push_tick("),
            "host/container collect must not run inside select! tick"
        );
        assert!(
            src.contains("compare_exchange"),
            "skip overlapping collect so Commands stay readable"
        );
    }

    #[test]
    fn rpc_execute_is_off_the_session_loop() {
        let src = include_str!("session.rs");
        let loop_src = src
            .split("async fn connect_session")
            .nth(1)
            .expect("connect_session")
            .split("fn command_err")
            .next()
            .expect("loop body");
        assert!(
            !loop_src.contains("handle_command("),
            "Docker/System execute must not await on the ingest select loop"
        );
        assert!(
            loop_src.contains("spawn_rpc_command("),
            "non-streaming Commands must be spawned like logs"
        );
        assert!(
            !loop_src.contains("apply_runtime_state("),
            "set_runtime docker connect must not await on the ingest select loop"
        );
        assert!(
            loop_src.contains("spawn_set_runtime("),
            "set_runtime must be spawned so lists still run after reconnect"
        );
        assert!(
            !loop_src.contains("tx.send("),
            "Result sends must not block reading the next Command"
        );
        assert!(
            !loop_src.contains("Ok(Some(_)) | Ok(None) => break"),
            "an empty ServerToAgent must not tear down the ingest session"
        );
    }

    #[test]
    fn buffer_drain_does_not_block_command_reads() {
        let src = include_str!("session.rs");
        let fn_src = src
            .split("async fn connect_session")
            .nth(1)
            .expect("connect_session")
            .split("fn command_err")
            .next()
            .expect("loop body");
        let drain_at = fn_src
            .find("buffer.drain()")
            .expect("drain buffered pushes");
        let inbound_at = fn_src
            .find("inbound.message()")
            .expect("must read Commands");
        let between = &fn_src[drain_at..inbound_at];
        assert!(
            between.contains("tokio::spawn"),
            "replaying the disk buffer must not await Push sends before inbound.message()"
        );
    }

    #[test]
    fn page_list_rpcs_budget_beats_the_node_page_wait() {
        assert!(
            PAGE_RPC_BUDGET <= Duration::from_secs(6),
            "agent must reply to node-page lists before the server's 8s wait"
        );
        assert!(is_page_list_op("container_list"));
        assert!(is_page_list_op("status"));
        assert!(!is_page_list_op("image_pull"));
        let src = include_str!("session.rs");
        assert!(
            src.contains("list_inflight"),
            "docker stats must yield the socket while the node page lists run"
        );
        assert!(
            src.contains("tokio::join!(crate::sys::local_status()"),
            "System status must not run ip then the helper in series past 8s"
        );
    }
}
