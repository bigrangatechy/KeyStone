// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keystone_core::config::ServerConfig;
use keystone_core::{AlertSnapshot, NodeSettings, ServerSettings};
use keystone_proto::{Command, StreamChunk};
use keystone_store::Stores;
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::auth;

pub type CommandTx = mpsc::Sender<Command>;

/// Wait budget for Docker/System tables when rendering a node page.
/// Pull/compose mutate still use 180s via [`AgentRegistry::call`].
pub const PAGE_LIST_TIMEOUT: Duration = Duration::from_secs(8);

struct PendingCall {
    cmd: Command,
    tx: oneshot::Sender<keystone_proto::CommandResult>,
}

struct ConnectedAgent {
    gen: u64,
    cmd_tx: CommandTx,
    pending: HashMap<String, PendingCall>,
    streams: HashMap<String, mpsc::Sender<StreamChunk>>,
}

#[derive(Clone, Default)]
pub struct AgentRegistry {
    inner: Arc<Mutex<HashMap<String, ConnectedAgent>>>,
    epoch: Arc<AtomicU64>,
}

impl AgentRegistry {
    /// Register a live command channel. Returns a generation that
    /// [`disconnect`] must present so a replaced session cannot wipe the new one.
    pub fn connect(&self, node_id: String, cmd_tx: CommandTx) -> u64 {
        let mut inner = self.inner.lock();
        if let Some(existing) = inner.get(&node_id) {
            if existing.cmd_tx.same_channel(&cmd_tx) {
                return existing.gen;
            }
        }
        let gen = self.epoch.fetch_add(1, Ordering::Relaxed);
        let (pending, streams) = match inner.remove(&node_id) {
            Some(old) => {
                for p in old.pending.values() {
                    let _ = cmd_tx.try_send(p.cmd.clone());
                }
                (old.pending, old.streams)
            }
            None => (HashMap::new(), HashMap::new()),
        };
        inner.insert(
            node_id,
            ConnectedAgent {
                gen,
                cmd_tx,
                pending,
                streams,
            },
        );
        gen
    }

    /// Drop this session only. Returns true when this generation was still live.
    pub fn disconnect(&self, node_id: &str, gen: u64) -> bool {
        let mut inner = self.inner.lock();
        match inner.get(node_id) {
            Some(agent) if agent.gen == gen => {
                inner.remove(node_id);
                true
            }
            _ => false,
        }
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
            if let Some(p) = agent.pending.remove(&result.request_id) {
                let _ = p.tx.send(result);
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
        self.call_timeout(node_id, op, payload_json, Duration::from_secs(180))
            .await
    }

    /// Same as [`call`] with an explicit wait. Node page listings must not
    /// use the 180s pull budget or opening a node hangs until restart.
    pub async fn call_timeout(
        &self,
        node_id: &str,
        op: &str,
        payload_json: String,
        timeout: Duration,
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
            agent.pending.insert(
                request_id.clone(),
                PendingCall {
                    cmd: cmd.clone(),
                    tx,
                },
            );
            if agent.cmd_tx.try_send(cmd).is_err() {
                agent.pending.remove(&request_id);
                anyhow::bail!("agent command queue full");
            }
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => anyhow::bail!("agent dropped command"),
            Err(_) => {
                if let Some(agent) = self.inner.lock().get_mut(node_id) {
                    agent.pending.remove(&request_id);
                }
                anyhow::bail!("agent command timed out")
            }
        }
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn stale_disconnect_does_not_drop_the_live_session() {
        let registry = AgentRegistry::default();
        let (tx_old, mut rx_old) = mpsc::channel(4);
        let (tx_live, mut rx_live) = mpsc::channel(4);
        let old = registry.connect("ranga".into(), tx_old);
        let live = registry.connect("ranga".into(), tx_live);
        assert!(registry.is_connected("ranga"));
        assert!(
            !registry.disconnect("ranga", old),
            "the replaced session must not clear the new command channel"
        );
        assert!(registry.is_connected("ranga"));
        registry.nudge("ranga", "container_list", "{}".into());
        assert!(
            rx_live.try_recv().is_ok(),
            "Docker ops must reach the live session"
        );
        assert!(
            rx_old.try_recv().is_err(),
            "replaced session must not still receive commands"
        );
        assert!(registry.disconnect("ranga", live));
        assert!(!registry.is_connected("ranga"));
    }

    #[tokio::test]
    async fn same_session_connect_does_not_drop_pending() {
        let registry = AgentRegistry::default();
        let (tx, mut rx) = mpsc::channel(4);
        registry.connect("ranga".into(), tx.clone());
        let wait = tokio::spawn({
            let registry = registry.clone();
            async move {
                registry
                    .call_timeout(
                        "ranga",
                        "container_list",
                        "{}".into(),
                        Duration::from_secs(2),
                    )
                    .await
            }
        });
        let cmd = rx.recv().await.expect("command queued");
        registry.connect("ranga".into(), tx.clone());
        registry.complete(
            "ranga",
            keystone_proto::CommandResult {
                request_id: cmd.request_id,
                ok: true,
                payload_json: "[]".into(),
                error: String::new(),
            },
        );
        let result = wait.await.expect("join").expect("oneshot");
        assert!(
            result.ok,
            "re-registering the same session must keep waiters"
        );
    }

    #[tokio::test]
    async fn new_session_replays_pending_instead_of_dropping() {
        let registry = AgentRegistry::default();
        let (tx_old, mut rx_old) = mpsc::channel(4);
        let (tx_new, mut rx_new) = mpsc::channel(4);
        registry.connect("ranga".into(), tx_old);
        let wait = tokio::spawn({
            let registry = registry.clone();
            async move {
                registry
                    .call_timeout(
                        "ranga",
                        "container_list",
                        "{}".into(),
                        Duration::from_secs(2),
                    )
                    .await
            }
        });
        let first = rx_old.recv().await.expect("queued on old session");
        registry.connect("ranga".into(), tx_new);
        let replayed = rx_new.recv().await.expect("replayed on new session");
        assert_eq!(replayed.request_id, first.request_id);
        registry.complete(
            "ranga",
            keystone_proto::CommandResult {
                request_id: replayed.request_id,
                ok: true,
                payload_json: "[]".into(),
                error: String::new(),
            },
        );
        let result = wait.await.expect("join").expect("oneshot");
        assert!(
            result.ok,
            "a replaced ingest session must not drop in-flight Docker/System waits"
        );
    }

    #[tokio::test]
    async fn page_list_timeout_does_not_wait_for_image_pull() {
        let registry = AgentRegistry::default();
        let (tx, _rx) = mpsc::channel(4);
        registry.connect("ranga".into(), tx);
        let start = std::time::Instant::now();
        let err = registry
            .call_timeout(
                "ranga",
                "container_list",
                "{}".into(),
                Duration::from_millis(80),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "listing timeout must fail fast, waited {:?}",
            start.elapsed()
        );
        assert!(err.contains("timed out"), "{err}");
        assert!(
            PAGE_LIST_TIMEOUT <= Duration::from_secs(15),
            "node page must not use the 180s pull budget"
        );
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub stores: Stores,
    pub agents: AgentRegistry,
    pub http: reqwest::Client,
    pub alert_state: Arc<Mutex<BTreeMap<String, AlertSnapshot>>>,
    pub login_gate: Arc<Mutex<LoginGate>>,
    pub stream_arms: Arc<Mutex<StreamArms>>,
    scrape_epoch: Arc<AtomicU64>,
    env_ingest_token: Option<String>,
}

/// Failed password / TOTP attempts per username. Existing installs keep
/// working; this only slows guessing if the UI is on the internet.
#[derive(Default)]
pub struct LoginGate {
    fails: HashMap<String, Vec<i64>>,
}

impl LoginGate {
    const WINDOW_SECS: i64 = 15 * 60;
    const MAX_FAILS: usize = 8;

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn prune(&mut self, user: &str, now: i64) {
        if let Some(v) = self.fails.get_mut(user) {
            v.retain(|t| now - *t < Self::WINDOW_SECS);
            if v.is_empty() {
                self.fails.remove(user);
            }
        }
    }

    pub fn locked(&mut self, username: &str) -> bool {
        let now = Self::now();
        self.prune(username, now);
        self.fails
            .get(username)
            .map(|v| v.len() >= Self::MAX_FAILS)
            .unwrap_or(false)
    }

    pub fn record_fail(&mut self, username: &str) {
        let now = Self::now();
        self.prune(username, now);
        self.fails
            .entry(username.to_string())
            .or_default()
            .push(now);
    }

    pub fn clear(&mut self, username: &str) {
        self.fails.remove(username);
    }
}

/// One-shot arm after confirm (+ step-up) so a streaming mutate cannot
/// start from a GET of the follow page alone.
#[derive(Clone, Debug)]
pub struct StreamArm {
    pub payload_json: String,
}

#[derive(Default)]
pub struct StreamArms {
    inner: HashMap<(String, String, String), (StreamArm, i64)>,
}

impl StreamArms {
    pub const TTL_SECS: i64 = 120;

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    pub fn arm(&mut self, user: &str, node: &str, op: &str, payload_json: String) {
        self.inner.insert(
            (user.to_string(), node.to_string(), op.to_string()),
            (StreamArm { payload_json }, Self::now() + Self::TTL_SECS),
        );
    }

    pub fn take(&mut self, user: &str, node: &str, op: &str) -> Option<StreamArm> {
        let now = Self::now();
        match self
            .inner
            .remove(&(user.to_string(), node.to_string(), op.to_string()))
        {
            Some((arm, exp)) if exp > now => Some(arm),
            _ => None,
        }
    }
}

impl AppState {
    pub fn new(config: ServerConfig, stores: Stores) -> Self {
        let env_ingest_token = std::env::var("KEYSTONE_INGEST_TOKEN")
            .ok()
            .filter(|s| !s.is_empty());
        Self::from_parts(config, stores, env_ingest_token)
    }

    /// Tests must not inherit `KEYSTONE_INGEST_TOKEN` from the developer’s shell.
    #[cfg(test)]
    pub(crate) fn for_test(config: ServerConfig, stores: Stores) -> Self {
        Self::from_parts(config, stores, None)
    }

    fn from_parts(config: ServerConfig, stores: Stores, env_ingest_token: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest");
        let alert_state = Arc::new(Mutex::new(crate::alerts::load_alert_state(&stores)));
        Self {
            config: Arc::new(config),
            stores,
            agents: AgentRegistry::default(),
            http,
            alert_state,
            login_gate: Arc::new(Mutex::new(LoginGate::default())),
            stream_arms: Arc::new(Mutex::new(StreamArms::default())),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_gate_locks_then_clears() {
        let mut g = LoginGate::default();
        assert!(!g.locked("admin"));
        for _ in 0..LoginGate::MAX_FAILS {
            assert!(!g.locked("admin"));
            g.record_fail("admin");
        }
        assert!(g.locked("admin"));
        g.clear("admin");
        assert!(!g.locked("admin"));
    }

    #[test]
    fn stream_arms_are_one_shot_and_scoped() {
        let mut arms = StreamArms::default();
        arms.arm(
            "admin",
            "ranga",
            "gitlab_restore",
            r#"{"name":"a_gitlab_backup.tar"}"#.into(),
        );
        assert!(arms.take("other", "ranga", "gitlab_restore").is_none());
        assert!(arms.take("admin", "other", "gitlab_restore").is_none());
        assert!(arms.take("admin", "ranga", "gitlab_backup").is_none());
        let got = arms
            .take("admin", "ranga", "gitlab_restore")
            .expect("armed");
        assert!(got.payload_json.contains("a_gitlab_backup.tar"));
        assert!(
            arms.take("admin", "ranga", "gitlab_restore").is_none(),
            "SSE reconnect must not start a second restore"
        );
    }
}
