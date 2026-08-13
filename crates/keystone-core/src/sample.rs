// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub metric: String,
    pub labels: Vec<Label>,
    pub value: f64,
    pub timestamp_unix_ms: i64,
}

impl Sample {
    pub fn new(metric: impl Into<String>, value: f64, timestamp_unix_ms: i64) -> Self {
        Self {
            metric: metric.into(),
            labels: Vec::new(),
            value,
            timestamp_unix_ms,
        }
    }

    pub fn with_label(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push(Label {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    pub fn labels_key(&self) -> String {
        let mut parts: Vec<String> = self
            .labels
            .iter()
            .map(|l| format!("{}={}", l.name, l.value))
            .collect();
        parts.sort();
        parts.join(",")
    }
}

/// Drop samples whose metric name is not in the catalog.
pub fn allowlist(samples: impl IntoIterator<Item = Sample>) -> (Vec<Sample>, usize) {
    let mut kept = Vec::new();
    let mut dropped = 0;
    for s in samples {
        if crate::metrics::is_known_metric(&s.metric) {
            kept.push(s);
        } else {
            dropped += 1;
        }
    }
    (kept, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_unknown_metric_names() {
        let samples = vec![
            Sample::new("node_load1", 0.5, 1),
            Sample::new("totally_unknown_metric", 1.0, 1),
        ];
        let (kept, dropped) = allowlist(samples);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, 1);
        assert_eq!(kept[0].metric, "node_load1");
    }
}
