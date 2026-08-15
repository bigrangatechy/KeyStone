// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Evaluate fleet-chip alerts after series writes. Persist previous firing
//! map so a restart does not re-POST. Webhook POSTs are spawned; ingest
//! does not wait on the remote.

use std::collections::BTreeMap;

use keystone_core::{
    apply_node_alerts, AlertSnapshot, AlertTransition, NodeSettings, Sample, ALERTS_STATE_KV_KEY,
};
use tracing::warn;

use crate::state::AppState;

pub fn load_alert_state(stores: &keystone_store::Stores) -> BTreeMap<String, AlertSnapshot> {
    match stores.metadata.kv_get(ALERTS_STATE_KV_KEY) {
        Ok(Some(raw)) if !raw.trim().is_empty() => match serde_json::from_str(&raw) {
            Ok(map) => map,
            Err(e) => {
                warn!("alerts_state unreadable: {e}");
                BTreeMap::new()
            }
        },
        _ => BTreeMap::new(),
    }
}

fn display_hostname(state: &AppState, node_id: &str) -> String {
    let rec = state.stores.metadata.get_node(node_id).ok().flatten();
    let settings = NodeSettings::parse_or_default(
        state
            .stores
            .metadata
            .node_settings_json(node_id)
            .ok()
            .flatten()
            .as_deref(),
    );
    match rec {
        Some(n) => settings.display_host(&n.hostname).to_string(),
        None => node_id.to_string(),
    }
}

/// Diff this node's samples against the persisted firing map. Persist on
/// change. POST the webhook (if configured) off the ingest path.
pub fn note_samples(state: &AppState, node_id: &str, samples: &[Sample]) {
    let mut map = state.alert_state.lock();
    let transitions = apply_node_alerts(&mut map, node_id, samples);
    if transitions.is_empty() {
        return;
    }
    let json = serde_json::to_string(&*map).unwrap_or_else(|_| "{}".into());
    drop(map);
    if let Err(e) = state.stores.metadata.kv_set(ALERTS_STATE_KV_KEY, &json) {
        warn!("persist alerts_state: {e}");
    }
    let url = state.stored_server_settings().alert_webhook_url;
    if url.is_empty() {
        return;
    }
    let hostname = display_hostname(state, node_id);
    let client = state.http.clone();
    let node_id = node_id.to_string();
    tokio::spawn(async move {
        post_alert_webhooks(client, url, node_id, hostname, transitions).await;
    });
}

async fn post_alert_webhooks(
    client: reqwest::Client,
    url: String,
    node_id: String,
    hostname: String,
    transitions: Vec<AlertTransition>,
) {
    for t in transitions {
        let event = t.event.clone();
        let body = serde_json::json!({
            "source": "keystone",
            "event": t.event,
            "node_id": node_id,
            "hostname": hostname,
            "chip": t.chip,
            "label": t.label,
            "severity": t.severity,
            "display": t.display,
            "hint": t.hint,
            "at": chrono::Utc::now().to_rfc3339(),
        });
        match client.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {}
            Ok(resp) => warn!("alert webhook {event} {node_id} → HTTP {}", resp.status()),
            Err(e) => warn!("alert webhook {node_id}: {e}"),
        }
    }
}
