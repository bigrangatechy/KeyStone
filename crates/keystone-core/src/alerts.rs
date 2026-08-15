// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Fleet-chip alerts. A chip fires when its tone is `warn` or `crit`.
//! Thresholds live in `fleet` (`ratio_tone`, `temp_tone`).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::fleet::{fleet_chips, FleetChip};
use crate::sample::Sample;

/// SQLite kv key for the last firing map (webhook de-dupe across restarts).
pub const ALERTS_STATE_KV_KEY: &str = "alerts_state";

/// Chips that are currently warn or crit.
pub fn firing_alerts(samples: &[Sample]) -> Vec<FleetChip> {
    fleet_chips(samples)
        .into_iter()
        .filter(FleetChip::is_firing)
        .collect()
}

/// Last known firing chip, keyed `{node_id}::{chip}` in kv JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertSnapshot {
    pub severity: String,
    pub display: String,
    pub hint: String,
    pub label: String,
}

/// Webhook / log event when a chip starts firing, changes severity, or clears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertTransition {
    /// `firing` or `resolved`.
    pub event: String,
    pub node_id: String,
    pub chip: String,
    pub label: String,
    pub severity: String,
    pub display: String,
    pub hint: String,
}

fn alert_key(node_id: &str, chip: &str) -> String {
    format!("{node_id}::{chip}")
}

/// Diff this node's latest samples against `state`. Mutates `state` only on
/// real transitions (new fire, severity change, resolve). Same-severity
/// value wobble is ignored so kv is not rewritten every ingest.
pub fn apply_node_alerts(
    state: &mut BTreeMap<String, AlertSnapshot>,
    node_id: &str,
    samples: &[Sample],
) -> Vec<AlertTransition> {
    let firing = firing_alerts(samples);
    let prefix = format!("{node_id}::");
    let mut keep = BTreeSet::new();
    let mut events = Vec::new();
    for chip in firing {
        let key = alert_key(node_id, &chip.id);
        keep.insert(key.clone());
        let snap = AlertSnapshot {
            severity: chip.tone.clone(),
            display: chip.display.clone(),
            hint: chip.hint.clone(),
            label: chip.label.clone(),
        };
        let changed = match state.get(&key) {
            None => true,
            Some(old) => old.severity != snap.severity,
        };
        if changed {
            events.push(AlertTransition {
                event: "firing".into(),
                node_id: node_id.into(),
                chip: chip.id,
                label: chip.label,
                severity: chip.tone,
                display: chip.display,
                hint: chip.hint,
            });
            state.insert(key, snap);
        }
    }
    let stale: Vec<String> = state
        .keys()
        .filter(|k| k.starts_with(&prefix) && !keep.contains(*k))
        .cloned()
        .collect();
    for key in stale {
        if let Some(old) = state.remove(&key) {
            let chip = key
                .strip_prefix(&prefix)
                .unwrap_or(key.as_str())
                .to_string();
            events.push(AlertTransition {
                event: "resolved".into(),
                node_id: node_id.into(),
                chip,
                label: old.label,
                severity: old.severity,
                display: old.display,
                hint: old.hint,
            });
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_quiet() {
        let mut state = BTreeMap::new();
        assert!(apply_node_alerts(&mut state, "pi", &[]).is_empty());
        assert!(state.is_empty());
        assert!(firing_alerts(&[]).is_empty());
    }

    #[test]
    fn high_cpu_fires_once() {
        let mut state = BTreeMap::new();
        let hot = vec![Sample::new("node_cpu_usage_ratio", 0.95, 1)];
        let first = apply_node_alerts(&mut state, "pi", &hot);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].event, "firing");
        assert_eq!(first[0].chip, "cpu");
        assert_eq!(first[0].severity, "crit");
        let again = apply_node_alerts(&mut state, "pi", &hot);
        assert!(again.is_empty());
        let wobble = vec![Sample::new("node_cpu_usage_ratio", 0.96, 2)];
        assert!(apply_node_alerts(&mut state, "pi", &wobble).is_empty());
    }

    #[test]
    fn warn_to_crit_is_a_new_firing() {
        let mut state = BTreeMap::new();
        let warn = vec![Sample::new("node_cpu_usage_ratio", 0.80, 1)];
        let first = apply_node_alerts(&mut state, "pi", &warn);
        assert_eq!(first[0].severity, "warn");
        let crit = vec![Sample::new("node_cpu_usage_ratio", 0.91, 2)];
        let second = apply_node_alerts(&mut state, "pi", &crit);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].event, "firing");
        assert_eq!(second[0].severity, "crit");
    }

    #[test]
    fn cool_cpu_resolves() {
        let mut state = BTreeMap::new();
        let hot = vec![Sample::new("node_cpu_usage_ratio", 0.95, 1)];
        apply_node_alerts(&mut state, "pi", &hot);
        let cool = vec![Sample::new("node_cpu_usage_ratio", 0.10, 2)];
        let events = apply_node_alerts(&mut state, "pi", &cool);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "resolved");
        assert_eq!(events[0].chip, "cpu");
        assert!(state.is_empty());
    }

    #[test]
    fn other_nodes_are_isolated() {
        let mut state = BTreeMap::new();
        let hot = vec![Sample::new("node_cpu_usage_ratio", 0.95, 1)];
        apply_node_alerts(&mut state, "a", &hot);
        let events = apply_node_alerts(&mut state, "b", &[]);
        assert!(events.is_empty());
        assert_eq!(state.len(), 1);
    }

    #[test]
    fn disk_warn_fires() {
        let samples = vec![
            Sample::new("node_filesystem_size_bytes", 100.0, 1)
                .with_label("mountpoint", "/")
                .with_label("fstype", "ext4"),
            Sample::new("node_filesystem_avail_bytes", 20.0, 1)
                .with_label("mountpoint", "/")
                .with_label("fstype", "ext4"),
        ];
        let firing = firing_alerts(&samples);
        assert_eq!(firing.len(), 1);
        assert_eq!(firing[0].id, "disk");
        assert_eq!(firing[0].tone, "warn");
    }
}
