// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Per-node dashboard widgets.
//!
//! To add a widget later:
//! 1. Add a [`WidgetKind`] variant and a line in [`WidgetKind::description`].
//! 2. Fill values in [`hydrate`].
//! 3. Draw it in `crates/keystone-server/src/static/app.js` (`renderWidget`).
//! 4. Optionally place it in [`Dashboard::default_node`].
//!
//! Layout is data ([`Dashboard`] JSON). The default is used until a node has a
//! saved layout — that is the hook for a customisable per-node UI.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

use crate::metrics::{is_known_metric, lookup};
use crate::net;
use crate::sample::Sample;
use crate::settings::NodeSettings;

/// How a card is drawn. The UI, layout JSON, and `/help` all use this enum.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    EnumIter,
    IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    /// Large formatted number from `metrics[0]`.
    Stat,
    /// 0–1 fill. One metric (already a ratio) or used/total from two metrics.
    Gauge,
    /// One bar per labeled series. Two metrics are free-or-used and total.
    BarList,
    /// Recent history of `metrics[0]`.
    Sparkline,
}

impl WidgetKind {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Stat => "Large formatted value (uptime, load, counters)",
            Self::Gauge => "Donut 0–100% from a ratio or used/total pair",
            Self::BarList => "One usage bar per labeled series (filesystems)",
            Self::Sparkline => "Short history sparkline of one metric",
        }
    }
}

fn default_span() -> u8 {
    1
}

/// One card on a node dashboard. Add fields here as widgets need them; unknown
/// JSON keys are ignored so older servers can still load a newer saved layout
/// after a downgrade of optional settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetInstance {
    /// Stable id for a future layout editor (not the metric name).
    pub id: String,
    pub kind: WidgetKind,
    pub title: String,
    /// Grid columns 1–4.
    #[serde(default = "default_span")]
    pub span: u8,
    /// Catalog names. Meaning is kind-specific; see [`WidgetKind`].
    pub metrics: Vec<String>,
    /// Label name used as the row title for [`WidgetKind::BarList`] (e.g. `mountpoint`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// For [`WidgetKind::BarList`]: treat `metrics[0]` as remaining space (`total - value`).
    #[serde(default)]
    pub invert: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dashboard {
    pub version: u32,
    pub widgets: Vec<WidgetInstance>,
}

impl Dashboard {
    pub const VERSION: u32 = 1;

    /// Built-in overview until the node has a saved layout.
    pub fn default_node() -> Self {
        Self {
            version: Self::VERSION,
            widgets: vec![
                WidgetInstance {
                    id: "cpu".into(),
                    kind: WidgetKind::Gauge,
                    title: "CPU".into(),
                    span: 1,
                    metrics: vec!["node_cpu_usage_ratio".into()],
                    label: None,
                    invert: false,
                },
                WidgetInstance {
                    id: "memory".into(),
                    kind: WidgetKind::Gauge,
                    title: "Memory".into(),
                    span: 1,
                    metrics: vec![
                        "node_memory_used_bytes".into(),
                        "node_memory_total_bytes".into(),
                    ],
                    label: None,
                    invert: false,
                },
                WidgetInstance {
                    id: "load".into(),
                    kind: WidgetKind::Sparkline,
                    title: "Load".into(),
                    span: 1,
                    metrics: vec!["node_load1".into()],
                    label: None,
                    invert: false,
                },
                WidgetInstance {
                    id: "uptime".into(),
                    kind: WidgetKind::Stat,
                    title: "Uptime".into(),
                    span: 1,
                    metrics: vec!["node_uptime_seconds".into()],
                    label: None,
                    invert: false,
                },
                WidgetInstance {
                    id: "disks".into(),
                    kind: WidgetKind::BarList,
                    title: "Disks".into(),
                    span: 2,
                    metrics: vec![
                        "node_filesystem_avail_bytes".into(),
                        "node_filesystem_size_bytes".into(),
                    ],
                    label: Some("mountpoint".into()),
                    invert: true,
                },
                WidgetInstance {
                    id: "load15".into(),
                    kind: WidgetKind::Stat,
                    title: "Load 15m".into(),
                    span: 1,
                    metrics: vec!["node_load15".into()],
                    label: None,
                    invert: false,
                },
                WidgetInstance {
                    id: "agent".into(),
                    kind: WidgetKind::Stat,
                    title: "Agent".into(),
                    span: 1,
                    metrics: vec!["keystone_agent_up".into()],
                    label: None,
                    invert: false,
                },
                WidgetInstance {
                    id: "net_rx".into(),
                    kind: WidgetKind::Sparkline,
                    title: "Network in".into(),
                    span: 2,
                    metrics: vec!["node_network_receive_bytes_per_second".into()],
                    label: None,
                    invert: false,
                },
                WidgetInstance {
                    id: "net_tx".into(),
                    kind: WidgetKind::Sparkline,
                    title: "Network out".into(),
                    span: 2,
                    metrics: vec!["node_network_transmit_bytes_per_second".into()],
                    label: None,
                    invert: false,
                },
            ],
        }
    }

    pub fn parse_or_default(json: Option<&str>) -> Self {
        let Some(raw) = json.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::default_node();
        };
        match serde_json::from_str::<Self>(raw) {
            Ok(d) if d.validate().is_ok() => d,
            _ => Self::default_node(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != Self::VERSION {
            return Err(format!("unsupported dashboard version {}", self.version));
        }
        let mut ids = HashSet::new();
        for w in &self.widgets {
            if w.id.trim().is_empty() {
                return Err("widget id is required".into());
            }
            if !ids.insert(w.id.as_str()) {
                return Err(format!("duplicate widget id {}", w.id));
            }
            if !(1..=4).contains(&w.span) {
                return Err(format!("widget {} span must be 1–4", w.id));
            }
            if w.metrics.is_empty() {
                return Err(format!("widget {} needs at least one metric", w.id));
            }
            for m in &w.metrics {
                if !is_known_metric(m) {
                    return Err(format!("widget {} unknown metric {m}", w.id));
                }
            }
            let n = w.metrics.len();
            let ok = match w.kind {
                WidgetKind::Stat | WidgetKind::Sparkline => n == 1,
                WidgetKind::Gauge => n == 1 || n == 2,
                WidgetKind::BarList => n == 1 || n == 2,
            };
            if !ok {
                return Err(format!(
                    "widget {} kind {} does not take {n} metrics",
                    w.id,
                    w.kind.as_str()
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SparkPoint {
    pub t: i64,
    pub v: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HydratedRow {
    pub label: String,
    pub display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
}

/// Values the UI paints. Keep this JSON stable when adding optional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HydratedWidget {
    pub id: String,
    pub kind: WidgetKind,
    pub title: String,
    pub span: u8,
    pub unit: String,
    pub display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
    /// `ok`, `warn`, or `crit` when a ratio is known.
    pub tone: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<HydratedRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spark: Vec<SparkPoint>,
}

const NOISY_FSTYPE: &[&str] = &[
    "overlay", "tmpfs", "devtmpfs", "squashfs", "proc", "sysfs", "cgroup", "cgroup2", "nsfs",
    "autofs",
];

/// `history` keys are `(metric, labels_key)` → `(timestamp_ms, value)` oldest first.
pub fn hydrate(
    dashboard: &Dashboard,
    latest: &[Sample],
    history: &HashMap<(String, String), Vec<(i64, f64)>>,
    settings: &NodeSettings,
) -> Vec<HydratedWidget> {
    dashboard
        .widgets
        .iter()
        .map(|w| hydrate_one(w, latest, history, settings))
        .collect()
}

fn hydrate_one(
    w: &WidgetInstance,
    latest: &[Sample],
    history: &HashMap<(String, String), Vec<(i64, f64)>>,
    settings: &NodeSettings,
) -> HydratedWidget {
    let unit = lookup(w.metrics.first().map(String::as_str).unwrap_or(""))
        .map(|d| d.unit)
        .unwrap_or("");
    let mut out = HydratedWidget {
        id: w.id.clone(),
        kind: w.kind,
        title: w.title.clone(),
        span: w.span,
        unit: unit.into(),
        display: "—".into(),
        value: None,
        ratio: None,
        tone: String::new(),
        rows: Vec::new(),
        spark: Vec::new(),
    };
    match w.kind {
        WidgetKind::Stat => {
            if let Some(s) = find_unlabeled(latest, &w.metrics[0]) {
                out.value = Some(s.value);
                out.display = format_value(s.value, unit);
            }
        }
        WidgetKind::Gauge => {
            if w.metrics.len() == 1 {
                if let Some(s) = find_unlabeled(latest, &w.metrics[0]) {
                    let ratio = s.value.clamp(0.0, 1.0);
                    out.value = Some(s.value);
                    out.ratio = Some(ratio);
                    out.display = format_value(ratio, "ratio");
                }
            } else if let (Some(used), Some(total)) = (
                find_unlabeled(latest, &w.metrics[0]),
                find_unlabeled(latest, &w.metrics[1]),
            ) {
                if total.value > 0.0 {
                    let ratio = (used.value / total.value).clamp(0.0, 1.0);
                    out.value = Some(used.value);
                    out.ratio = Some(ratio);
                    out.display = format!(
                        "{} / {}",
                        format_value(used.value, unit),
                        format_value(total.value, unit)
                    );
                }
            }
        }
        WidgetKind::BarList => {
            out.rows = bar_rows(w, latest, unit);
        }
        WidgetKind::Sparkline => {
            fill_sparkline(&mut out, w, latest, history, settings, unit);
        }
    }
    out.tone = tone(out.ratio);
    out
}

fn fill_sparkline(
    out: &mut HydratedWidget,
    w: &WidgetInstance,
    latest: &[Sample],
    history: &HashMap<(String, String), Vec<(i64, f64)>>,
    settings: &NodeSettings,
    unit: &str,
) {
    let metric = &w.metrics[0];
    let labeled: Vec<&Sample> = latest
        .iter()
        .filter(|s| s.metric == *metric && label(s, "device").is_some())
        .collect();
    if !labeled.is_empty() {
        let names: Vec<&str> = labeled.iter().filter_map(|s| label(s, "device")).collect();
        let agg = net::aggregate_ifaces(names, &settings.network_devices);
        let mut sum = 0.0;
        let mut keys = Vec::new();
        for s in labeled {
            let Some(dev) = label(s, "device") else {
                continue;
            };
            if !agg.contains(&dev) {
                continue;
            }
            sum += s.value;
            keys.push(s.labels_key());
            out.rows.push(HydratedRow {
                label: dev.into(),
                display: format_value(s.value, unit),
                ratio: None,
            });
        }
        out.rows.sort_by(|a, b| a.label.cmp(&b.label));
        out.value = Some(sum);
        out.display = format_value(sum, unit);
        out.spark = merge_spark(history, metric, &keys);
        if out.spark.is_empty() {
            out.spark = merge_spark(history, metric, &[String::new()]);
        }
        return;
    }
    let key = (metric.clone(), String::new());
    if let Some(points) = history.get(&key) {
        out.spark = points
            .iter()
            .map(|(t, v)| SparkPoint { t: *t, v: *v })
            .collect();
    }
    if let Some(s) = find_unlabeled(latest, metric) {
        out.value = Some(s.value);
        out.display = format_value(s.value, unit);
    } else if let Some(last) = out.spark.last() {
        out.value = Some(last.v);
        out.display = format_value(last.v, unit);
    }
}

fn merge_spark(
    history: &HashMap<(String, String), Vec<(i64, f64)>>,
    metric: &str,
    label_keys: &[String],
) -> Vec<SparkPoint> {
    let mut by_t: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
    for lk in label_keys {
        if let Some(points) = history.get(&(metric.to_string(), lk.clone())) {
            for (t, v) in points {
                *by_t.entry(*t).or_insert(0.0) += *v;
            }
        }
    }
    by_t.into_iter().map(|(t, v)| SparkPoint { t, v }).collect()
}

fn bar_rows(w: &WidgetInstance, latest: &[Sample], unit: &str) -> Vec<HydratedRow> {
    let primary = &w.metrics[0];
    let totals: HashMap<String, f64> = if w.metrics.len() == 2 {
        latest
            .iter()
            .filter(|s| s.metric == w.metrics[1])
            .map(|s| (s.labels_key(), s.value))
            .collect()
    } else {
        HashMap::new()
    };
    let label_name = w.label.as_deref();
    let mut rows: Vec<HydratedRow> = latest
        .iter()
        .filter(|s| s.metric == *primary)
        .filter(|s| label(s, "fstype").is_none_or(|ft| !NOISY_FSTYPE.contains(&ft)))
        .filter_map(|s| {
            let mut value = s.value;
            let total = totals.get(&s.labels_key()).copied();
            if w.invert {
                let t = total?;
                if t <= 0.0 {
                    return None;
                }
                value = (t - s.value).max(0.0);
            }
            let ratio = total
                .filter(|t| *t > 0.0)
                .map(|t| (value / t).clamp(0.0, 1.0));
            let title = label_name
                .and_then(|n| label(s, n).map(str::to_string))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| s.labels_key());
            let display = match total {
                Some(t) => format!("{} / {}", format_value(value, unit), format_value(t, unit)),
                None => format_value(value, unit),
            };
            Some(HydratedRow {
                label: title,
                display,
                ratio,
            })
        })
        .collect();
    rows.sort_by(|a, b| a.label.cmp(&b.label));
    rows
}

fn find_unlabeled<'a>(latest: &'a [Sample], name: &str) -> Option<&'a Sample> {
    latest
        .iter()
        .find(|s| s.metric == name && s.labels.is_empty())
        .or_else(|| latest.iter().find(|s| s.metric == name))
}

fn label<'a>(s: &'a Sample, name: &str) -> Option<&'a str> {
    s.labels
        .iter()
        .find(|l| l.name == name)
        .map(|l| l.value.as_str())
}

fn tone(ratio: Option<f64>) -> String {
    match ratio {
        Some(r) if r >= 0.9 => "crit".into(),
        Some(r) if r >= 0.75 => "warn".into(),
        Some(_) => "ok".into(),
        None => String::new(),
    }
}

pub fn format_value(value: f64, unit: &str) -> String {
    match unit {
        "bytes" => format_bytes(value),
        "seconds" => format_duration(value),
        "ratio" => format!("{:.0}%", (value * 100.0).clamp(0.0, 100.0)),
        "boolean" => {
            if value >= 0.5 {
                "up".into()
            } else {
                "down".into()
            }
        }
        "load" => format!("{value:.2}"),
        "bytes_per_second" => format!("{}/s", format_bytes(value)),
        "packets" | "errors" => format!("{value:.0}"),
        _ => format!("{value:.2}"),
    }
}

fn format_bytes(v: f64) -> String {
    const K: f64 = 1024.0;
    let a = v.abs();
    if a >= K * K * K {
        format!("{:.1} GiB", v / (K * K * K))
    } else if a >= K * K {
        format!("{:.1} MiB", v / (K * K))
    } else if a >= K {
        format!("{:.1} KiB", v / K)
    } else {
        format!("{v:.0} B")
    }
}

fn format_duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let d = s / 86_400;
    let h = (s % 86_400) / 3_600;
    let m = (s % 3_600) / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dashboard_is_valid() {
        Dashboard::default_node().validate().unwrap();
    }

    #[test]
    fn hydrate_cpu_and_memory_gauges() {
        let samples = vec![
            Sample::new("node_cpu_usage_ratio", 0.42, 1),
            Sample::new("node_memory_used_bytes", 8.0 * 1024.0 * 1024.0 * 1024.0, 1),
            Sample::new(
                "node_memory_total_bytes",
                16.0 * 1024.0 * 1024.0 * 1024.0,
                1,
            ),
        ];
        let dash = Dashboard::default_node();
        let cards = hydrate(&dash, &samples, &HashMap::new(), &NodeSettings::default());
        let cpu = cards.iter().find(|c| c.id == "cpu").unwrap();
        assert_eq!(cpu.display, "42%");
        assert!((cpu.ratio.unwrap() - 0.42).abs() < 1e-9);
        assert_eq!(cpu.tone, "ok");
        let mem = cards.iter().find(|c| c.id == "memory").unwrap();
        assert!(mem.display.contains("GiB"));
        assert!((mem.ratio.unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn bar_list_inverts_avail_and_skips_overlay() {
        let samples = vec![
            Sample::new("node_filesystem_avail_bytes", 40.0, 1)
                .with_label("mountpoint", "/")
                .with_label("device", "/dev/sda1")
                .with_label("fstype", "ext4"),
            Sample::new("node_filesystem_size_bytes", 100.0, 1)
                .with_label("mountpoint", "/")
                .with_label("device", "/dev/sda1")
                .with_label("fstype", "ext4"),
            Sample::new("node_filesystem_avail_bytes", 1.0, 1)
                .with_label("mountpoint", "/overlay")
                .with_label("fstype", "overlay"),
            Sample::new("node_filesystem_size_bytes", 2.0, 1)
                .with_label("mountpoint", "/overlay")
                .with_label("fstype", "overlay"),
        ];
        let w = WidgetInstance {
            id: "disks".into(),
            kind: WidgetKind::BarList,
            title: "Disks".into(),
            span: 2,
            metrics: vec![
                "node_filesystem_avail_bytes".into(),
                "node_filesystem_size_bytes".into(),
            ],
            label: Some("mountpoint".into()),
            invert: true,
        };
        let rows = bar_rows(&w, &samples, "bytes");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "/");
        assert!((rows[0].ratio.unwrap() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn formats_network_rate() {
        assert_eq!(format_value(1536.0, "bytes_per_second"), "1.5 KiB/s");
    }

    #[test]
    fn sparkline_sums_labeled_nics() {
        let samples = vec![
            Sample::new("node_network_receive_bytes_per_second", 100.0, 1)
                .with_label("device", "eth0"),
            Sample::new("node_network_receive_bytes_per_second", 50.0, 1)
                .with_label("device", "docker0"),
            Sample::new("node_network_receive_bytes_per_second", 10.0, 1)
                .with_label("device", "lo"),
        ];
        let dash = Dashboard {
            version: 1,
            widgets: vec![WidgetInstance {
                id: "net_rx".into(),
                kind: WidgetKind::Sparkline,
                title: "Network in".into(),
                span: 2,
                metrics: vec!["node_network_receive_bytes_per_second".into()],
                label: None,
                invert: false,
            }],
        };
        let cards = hydrate(&dash, &samples, &HashMap::new(), &NodeSettings::default());
        assert_eq!(cards[0].value, Some(100.0));
        assert_eq!(cards[0].rows.len(), 1);
        assert_eq!(cards[0].rows[0].label, "eth0");
    }

    #[test]
    fn bad_layout_falls_back_to_default() {
        let d = Dashboard::parse_or_default(Some(r#"{"version":9,"widgets":[]}"#));
        assert_eq!(d, Dashboard::default_node());
    }
}
