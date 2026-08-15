// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use keystone_core::config::ServerConfig;
use keystone_core::{NodeSettings, ServerSettings};
use keystone_proto::{Command, StreamChunk};
use keystone_store::Stores;
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::auth;

pub type CommandTx = mpsc::Sender<Command>;

pub struct Pending {
    pub tx: oneshot::Sender<keystone_proto::CommandResult>,
}

#[derive(Clone, Default)]
pub struct AgentRegistry {
    inner: Arc<Mutex<HashMap<String, ConnectedAgent>>>,
}

struct ConnectedAgent {
    cmd_tx: CommandTx,
    pending: HashMap<String, oneshot::Sender<keystone_proto::CommandResult>>,
    streams: HashMap<String, mpsc::Sender<StreamChunk>>,
}

impl AgentRegistry {
    pub fn connect(&self, node_id: String, cmd_tx: CommandTx) {
        self.inner.lock().insert(
            node_id,
            ConnectedAgent {
                cmd_tx,
                pending: HashMap::new(),
                streams: HashMap::new(),
            },
        );
    }

    pub fn disconnect(&self, node_id: &str) {
        self.inner.lock().remove(node_id);
    }

    pub fn is_connected(&self, node_id: &str) -> bool {
        self.inner.lock().contains_key(node_id)
    }

    /// Queue a command without waiting for a result. No-op if the agent is offline.
    pub fn nudge(&self, node_id: &str, op: &str, payload_json: String) {
        let cmd = Command {
            request_id: Uuid::new_v4().to_string(),
            op: op.to_string(),
            payload_json,
        };
        if let Some(agent) = self.inner.lock().get_mut(node_id) {
            let _ = agent.cmd_tx.try_send(cmd);
        }
    }

    pub fn nudge_runtime(&self, node_id: &str, settings: &NodeSettings) {
        self.nudge(node_id, "set_runtime", settings.agent_runtime_json());
    }

    pub fn complete(&self, node_id: &str, result: keystone_proto::CommandResult) {
        let mut inner = self.inner.lock();
        if let Some(agent) = inner.get_mut(node_id) {
            if let Some(tx) = agent.pending.remove(&result.request_id) {
                let _ = tx.send(result);
            }
        }
    }

    pub fn push_chunk(&self, node_id: &str, chunk: StreamChunk) {
        let mut inner = self.inner.lock();
        if let Some(agent) = inner.get_mut(node_id) {
            let eof = chunk.eof;
            let id = chunk.request_id.clone();
            if let Some(tx) = agent.streams.get(&id) {
                let _ = tx.try_send(chunk);
            }
            if eof {
                agent.streams.remove(&id);
            }
        }
    }

    pub fn cancel_stream(&self, node_id: &str, request_id: &str) {
        let mut inner = self.inner.lock();
        if let Some(agent) = inner.get_mut(node_id) {
            agent.streams.remove(request_id);
            let cmd = Command {
                request_id: Uuid::new_v4().to_string(),
                op: "cancel".into(),
                payload_json: serde_json::json!({ "request_id": request_id }).to_string(),
            };
            let _ = agent.cmd_tx.try_send(cmd);
        }
    }

    /// Follow-style ops: chunks arrive on the returned receiver until eof or cancel.
    pub fn stream(
        &self,
        node_id: &str,
        op: &str,
        payload_json: String,
    ) -> anyhow::Result<(String, mpsc::Receiver<StreamChunk>)> {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(256);
        let cmd = Command {
            request_id: request_id.clone(),
            op: op.to_string(),
            payload_json,
        };
        {
            let mut inner = self.inner.lock();
            let agent = inner
                .get_mut(node_id)
                .ok_or_else(|| anyhow::anyhow!("agent {node_id} is not connected"))?;
            agent.streams.insert(request_id.clone(), tx);
            agent
                .cmd_tx
                .try_send(cmd)
                .map_err(|_| anyhow::anyhow!("agent command queue full"))?;
        }
        Ok((request_id, rx))
    }

    pub async fn call(
        &self,
        node_id: &str,
        op: &str,
        payload_json: String,
    ) -> anyhow::Result<keystone_proto::CommandResult> {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        let cmd = Command {
            request_id: request_id.clone(),
            op: op.to_string(),
            payload_json,
        };
        {
            let mut inner = self.inner.lock();
            let agent = inner
                .get_mut(node_id)
                .ok_or_else(|| anyhow::anyhow!("agent {node_id} is not connected"))?;
            agent.pending.insert(request_id, tx);
            agent
                .cmd_tx
                .try_send(cmd)
                .map_err(|_| anyhow::anyhow!("agent command queue full"))?;
        }
        match tokio::time::timeout(Duration::from_secs(120), rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => anyhow::bail!("agent dropped command"),
            Err(_) => anyhow::bail!("agent command timed out"),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub stores: Stores,
    pub agents: AgentRegistry,
    scrape_epoch: Arc<AtomicU64>,
    env_ingest_token: Option<String>,
}

impl AppState {
    pub fn new(config: ServerConfig, stores: Stores) -> Self {
        let env_ingest_token = std::env::var("KEYSTONE_INGEST_TOKEN")
            .ok()
            .filter(|s| !s.is_empty());
        Self {
            config: Arc::new(config),
            stores,
            agents: AgentRegistry::default(),
            scrape_epoch: Arc::new(AtomicU64::new(0)),
            env_ingest_token,
        }
    }

    pub fn stored_server_settings(&self) -> ServerSettings {
        ServerSettings::parse_or_default(
            self.stores
                .metadata
                .kv_get(ServerSettings::KV_KEY)
                .ok()
                .flatten()
                .as_deref(),
        )
    }

    /// Settings used at runtime. `KEYSTONE_INGEST_TOKEN` overlays the stored token.
    pub fn server_settings(&self) -> ServerSettings {
        let mut s = self.stored_server_settings();
        if let Some(t) = &self.env_ingest_token {
            s.ingest_token = t.clone();
        }
        s
    }

    pub fn ingest_token(&self) -> String {
        self.server_settings().ingest_token
    }

    pub fn ingest_token_env_override(&self) -> bool {
        self.env_ingest_token.is_some()
    }

    pub fn seed_server_settings(&self) -> anyhow::Result<()> {
        if self
            .stores
            .metadata
            .kv_get(ServerSettings::KV_KEY)?
            .is_none()
        {
            let mut s = ServerSettings::from_config(&self.config);
            if let Some(t) = &self.env_ingest_token {
                s.ingest_token = t.clone();
            }
            if s.ingest_token.is_empty() {
                s.ingest_token = auth::generate_ingest_token();
                tracing::info!("generated ingest token (shown in Settings)");
            }
            s.retention_hours = ServerSettings::clamp_retention_hours(s.retention_hours);
            self.stores
                .metadata
                .kv_set(ServerSettings::KV_KEY, &serde_json::to_string(&s)?)?;
        }
        let s = self.stored_server_settings();
        self.stores.series.set_retention_hours(s.retention_hours);
        Ok(())
    }

    pub fn save_server_settings(&self, settings: &ServerSettings) -> anyhow::Result<()> {
        let mut s = settings.clone();
        s.retention_hours = ServerSettings::clamp_retention_hours(s.retention_hours);
        self.stores
            .metadata
            .kv_set(ServerSettings::KV_KEY, &serde_json::to_string(&s)?)?;
        self.stores.series.set_retention_hours(s.retention_hours);
        self.scrape_epoch.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn scrape_epoch(&self) -> u64 {
        self.scrape_epoch.load(Ordering::Relaxed)
    }
}
