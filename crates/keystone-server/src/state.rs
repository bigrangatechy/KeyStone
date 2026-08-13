// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use keystone_core::config::ServerConfig;
use keystone_proto::Command;
use keystone_store::Stores;
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

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
}

impl AgentRegistry {
    pub fn connect(&self, node_id: String, cmd_tx: CommandTx) {
        self.inner.lock().insert(
            node_id,
            ConnectedAgent {
                cmd_tx,
                pending: HashMap::new(),
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

    pub fn nudge_poll_interval(&self, node_id: &str, secs: u64) {
        self.nudge(
            node_id,
            "set_interval",
            serde_json::json!({ "interval_secs": secs }).to_string(),
        );
    }

    pub fn complete(&self, node_id: &str, result: keystone_proto::CommandResult) {
        let mut inner = self.inner.lock();
        if let Some(agent) = inner.get_mut(node_id) {
            if let Some(tx) = agent.pending.remove(&result.request_id) {
                let _ = tx.send(result);
            }
        }
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
}

impl AppState {
    pub fn new(config: ServerConfig, stores: Stores) -> Self {
        Self {
            config: Arc::new(config),
            stores,
            agents: AgentRegistry::default(),
        }
    }
}
