// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Per-node dashboard widgets.
//!
//! To add a widget later:
//! 1. Add a [`WidgetKind`] variant and a line in [`WidgetKind::description`].
//! 2. Fill values in [`hydrate`].
//! 3. Draw it in `crates/keystone-server/src/static/app.js` (`renderWidget`).
//! 4. Register a [`presets`] entry so the overview picker can place it.
//! 5. Optionally include it on the built-in default dashboard.

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
    /// When set, Stat/Gauge/Sparkline (and BarList rows) use only the series
    /// whose [`Sample::labels_key`] equals this value. That is how one
    /// temperature sensor becomes its own card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    /// For [`WidgetKind::BarList`]: treat `metrics[0]` as remaining space (`total - value`).
    #[serde(default)]
    pub invert: bool,
}

impl WidgetInstance {
    pub fn new(
        id: impl Into<String>,
        kind: WidgetKind,
        title: impl Into<String>,
        metrics: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            title: title.into(),
            span: 1,
            metrics: metrics.into_iter().map(Into::into).collect(),
            label: None,
            series: None,
            invert: false,
        }
    }

    pub fn with_span(mut self, span: u8) -> Self {
        self.span = span;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_invert(mut self) -> Self {
        self.invert = true;
        self
    }

    pub fn with_series(mut self, series: impl Into<String>) -> Self {
        self.series = Some(series.into());
        self
    }
}

/// A catalog entry the node overview picker can drop onto a dashboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetPreset {
    pub id: String,
    pub group: String,
    pub description: String,
    pub widget: WidgetInstance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dashboard {
    pub version: u32,
    pub widgets: Vec<WidgetInstance>,
}

impl Dashboard {
    pub const VERSION: u32 = 1;

    const DEFAULT_IDS: &'static [&'static str] = &[
        "cpu", "cpu_temp", "memory", "load", "uptime", "disks", "load15", "agent", "net_rx",
        "net_tx", "gpu", "gpu_mem", "gpu_temp",
    ];

    /// Built-in overview until the node has a saved layout.
    pub fn default_node() -> Self {
        let catalog = presets();
        Self {
            version: Self::VERSION,
            widgets: Self::DEFAULT_IDS
                .iter()
                .map(|id| {
                    catalog
                        .iter()
                        .find(|p| p.id == *id)
                        .unwrap_or_else(|| panic!("missing widget preset {id}"))
                        .widget
                        .clone()
                })
                .collect(),
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

/// Cards the overview picker can add. Default dashboards are a subset of these ids.
pub fn presets() -> Vec<WidgetPreset> {
    vec![
        preset(
            "cpu",
            "CPU",
            "Donut of overall CPU usage",
            WidgetInstance::new("cpu", WidgetKind::Gauge, "CPU", ["node_cpu_usage_ratio"]),
        ),
        preset(
            "cpu_spark",
            "CPU",
            "CPU usage over the last 15 minutes",
            WidgetInstance::new(
                "cpu_spark",
                WidgetKind::Sparkline,
                "CPU",
                ["node_cpu_usage_ratio"],
            ),
        ),
        preset(
            "cpu_stat",
            "CPU",
            "CPU usage as a percentage",
            WidgetInstance::new(
                "cpu_stat",
                WidgetKind::Stat,
                "CPU",
                ["node_cpu_usage_ratio"],
            ),
        ),
        preset(
            "cpu_temp",
            "CPU",
            "CPU package / SoC temperature",
            WidgetInstance::new(
                "cpu_temp",
                WidgetKind::Stat,
                "CPU temp",
                ["node_cpu_temperature_celsius"],
            ),
        ),
        preset(
            "cpu_temp_spark",
            "CPU",
            "CPU temperature over the last 15 minutes",
            WidgetInstance::new(
                "cpu_temp_spark",
                WidgetKind::Sparkline,
                "CPU temp",
                ["node_cpu_temperature_celsius"],
            ),
        ),
        preset(
            "memory",
            "Memory",
            "Donut of used / total RAM",
            WidgetInstance::new(
                "memory",
                WidgetKind::Gauge,
                "Memory",
                ["node_memory_used_bytes", "node_memory_total_bytes"],
            ),
        ),
        preset(
            "memory_spark",
            "Memory",
            "Used RAM over the last 15 minutes",
            WidgetInstance::new(
                "memory_spark",
                WidgetKind::Sparkline,
                "Memory",
                ["node_memory_used_bytes"],
            ),
        ),
        preset(
            "memory_stat",
            "Memory",
            "Used RAM as a number",
            WidgetInstance::new(
                "memory_stat",
                WidgetKind::Stat,
                "Memory",
                ["node_memory_used_bytes"],
            ),
        ),
        preset(
            "memory_avail",
            "Memory",
            "Memory available for new work",
            WidgetInstance::new(
                "memory_avail",
                WidgetKind::Stat,
                "Mem available",
                ["node_memory_available_bytes"],
            ),
        ),
        preset(
            "load",
            "Load",
            "1 minute load average sparkline",
            WidgetInstance::new("load", WidgetKind::Sparkline, "Load", ["node_load1"]),
        ),
        preset(
            "load_stat",
            "Load",
            "1 minute load average",
            WidgetInstance::new("load_stat", WidgetKind::Stat, "Load 1m", ["node_load1"]),
        ),
        preset(
            "load5",
            "Load",
            "5 minute load average sparkline",
            WidgetInstance::new("load5", WidgetKind::Sparkline, "Load 5m", ["node_load5"]),
        ),
        preset(
            "load15",
            "Load",
            "15 minute load average",
            WidgetInstance::new("load15", WidgetKind::Stat, "Load 15m", ["node_load15"]),
        ),
        preset(
            "load15_spark",
            "Load",
            "15 minute load average sparkline",
            WidgetInstance::new(
                "load15_spark",
                WidgetKind::Sparkline,
                "Load 15m",
                ["node_load15"],
            ),
        ),
        preset(
            "uptime",
            "System",
            "Time since last boot",
            WidgetInstance::new(
                "uptime",
                WidgetKind::Stat,
                "Uptime",
                ["node_uptime_seconds"],
            ),
        ),
        preset(
            "agent",
            "System",
            "Whether the agent is pushing",
            WidgetInstance::new("agent", WidgetKind::Stat, "Agent", ["keystone_agent_up"]),
        ),
        preset(
            "temps",
            "System",
            "Every hardware sensor on one card. Prefer the per-sensor cards under Temperature.",
            WidgetInstance::new(
                "temps",
                WidgetKind::BarList,
                "All temperatures",
                ["node_hwmon_temp_celsius", "node_hwmon_temp_max_celsius"],
            )
            .with_span(2)
            .with_label("sensor"),
        ),
        preset(
            "hottest",
            "System",
            "Hottest sensor on the node",
            WidgetInstance::new(
                "hottest",
                WidgetKind::Stat,
                "Hottest",
                ["node_hwmon_temp_celsius"],
            ),
        ),
        preset(
            "disks",
            "Disk",
            "Used space per filesystem",
            WidgetInstance::new(
                "disks",
                WidgetKind::BarList,
                "Disks",
                ["node_filesystem_avail_bytes", "node_filesystem_size_bytes"],
            )
            .with_span(2)
            .with_label("mountpoint")
            .with_invert(),
        ),
        preset(
            "net_rx",
            "Network",
            "Receive rate sparkline",
            WidgetInstance::new(
                "net_rx",
                WidgetKind::Sparkline,
                "Network in",
                ["node_network_receive_bytes_per_second"],
            )
            .with_span(2),
        ),
        preset(
            "net_tx",
            "Network",
            "Transmit rate sparkline",
            WidgetInstance::new(
                "net_tx",
                WidgetKind::Sparkline,
                "Network out",
                ["node_network_transmit_bytes_per_second"],
            )
            .with_span(2),
        ),
        preset(
            "net_rx_stat",
            "Network",
            "Current receive rate",
            WidgetInstance::new(
                "net_rx_stat",
                WidgetKind::Stat,
                "Net in",
                ["node_network_receive_bytes_per_second"],
            ),
        ),
        preset(
            "net_tx_stat",
            "Network",
            "Current transmit rate",
            WidgetInstance::new(
                "net_tx_stat",
                WidgetKind::Stat,
                "Net out",
                ["node_network_transmit_bytes_per_second"],
            ),
        ),
        preset(
            "gpu",
            "GPU",
            "Donut of GPU busy (average if several cards)",
            WidgetInstance::new("gpu", WidgetKind::Gauge, "GPU", ["node_gpu_usage_ratio"]),
        ),
        preset(
            "gpu_mem",
            "GPU",
            "Donut of GPU memory used / total",
            WidgetInstance::new(
                "gpu_mem",
                WidgetKind::Gauge,
                "GPU memory",
                ["node_gpu_memory_used_bytes", "node_gpu_memory_total_bytes"],
            ),
        ),
        preset(
            "gpu_spark",
            "GPU",
            "GPU busy over the last 15 minutes",
            WidgetInstance::new(
                "gpu_spark",
                WidgetKind::Sparkline,
                "GPU",
                ["node_gpu_usage_ratio"],
            )
            .with_label("gpu"),
        ),
        preset(
            "gpu_list",
            "GPU",
            "One busy bar per GPU",
            WidgetInstance::new(
                "gpu_list",
                WidgetKind::BarList,
                "GPUs",
                ["node_gpu_usage_ratio"],
            )
            .with_span(2)
            .with_label("gpu"),
        ),
        preset(
            "gpu_temp",
            "GPU",
            "Hottest GPU temperature",
            WidgetInstance::new(
                "gpu_temp",
                WidgetKind::Stat,
                "GPU temp",
                ["node_gpu_temperature_celsius"],
            ),
        ),
        preset(
            "gpu_temps",
            "GPU",
            "Temperature per GPU",
            WidgetInstance::new(
                "gpu_temps",
                WidgetKind::BarList,
                "GPU temps",
                ["node_gpu_temperature_celsius"],
            )
            .with_span(2)
            .with_label("gpu"),
        ),
    ]
}

fn preset(id: &str, group: &str, description: &str, widget: WidgetInstance) -> WidgetPreset {
    WidgetPreset {
        id: id.into(),
        group: group.into(),
        description: description.into(),
        widget,
    }
}

/// Built-in picker cards plus one card per temperature sensor in `latest`.
/// The living `/help` table is [`presets`] only; node-specific sensors are
/// offered in Customize after the agent has pushed samples.
pub fn presets_for_samples(latest: &[Sample]) -> Vec<WidgetPreset> {
    let mut out = presets();
    let mut used: HashSet<String> = out.iter().map(|p| p.id.clone()).collect();
    out.extend(temperature_sensor_presets(latest, &mut used));
    out
}

fn temperature_sensor_presets(latest: &[Sample], used: &mut HashSet<String>) -> Vec<WidgetPreset> {
    struct Hit {
        title: String,
        group: String,
        description: String,
        base_id: String,
        labels_key: String,
        has_max: bool,
        metric: &'static str,
        max_metric: Option<&'static str>,
    }

    let mut max_keys: HashSet<String> = HashSet::new();
    for s in latest {
        if s.metric == "node_hwmon_temp_max_celsius" && !s.labels.is_empty() {
            max_keys.insert(s.labels_key());
        }
    }

    let mut hits: Vec<Hit> = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();
    for s in latest {
        if s.labels.is_empty() {
            continue;
        }
        if s.metric != "node_hwmon_temp_celsius" && s.metric != "node_gpu_temperature_celsius" {
            continue;
        }
        if !seen_keys.insert(s.labels_key()) {
            continue;
        }
        if s.metric == "node_hwmon_temp_celsius" {
            let sensor = label(s, "sensor").unwrap_or("sensor");
            let chip = label(s, "chip").unwrap_or("hwmon");
            let kind = label(s, "kind").unwrap_or("other");
            let key = s.labels_key();
            hits.push(Hit {
                title: sensor.to_string(),
                group: temp_group(kind),
                description: format!("{kind} · {chip}"),
                base_id: format!("temp-{}", slug(&format!("{chip}-{sensor}"))),
                has_max: max_keys.contains(&key),
                labels_key: key,
                metric: "node_hwmon_temp_celsius",
                max_metric: Some("node_hwmon_temp_max_celsius"),
            });
        } else if s.metric == "node_gpu_temperature_celsius" {
            let gpu = label(s, "gpu").unwrap_or("GPU");
            let vendor = label(s, "vendor").unwrap_or("");
            let key = s.labels_key();
            let desc = if vendor.is_empty() {
                format!("GPU {gpu}")
            } else {
                format!("{vendor} · {gpu}")
            };
            hits.push(Hit {
                title: format!("{gpu} temp"),
                group: "GPU".into(),
                description: desc,
                base_id: format!("gpu-temp-{}", slug(gpu)),
                has_max: false,
                labels_key: key,
                metric: "node_gpu_temperature_celsius",
                max_metric: None,
            });
        }
    }
    hits.sort_by(|a, b| {
        a.group.cmp(&b.group).then_with(|| {
            a.title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase())
        })
    });

    hits.into_iter()
        .map(|h| {
            let id = unique_preset_id(used, &h.base_id);
            let widget = if h.has_max {
                if let Some(max_metric) = h.max_metric {
                    WidgetInstance::new(
                        id.clone(),
                        WidgetKind::Gauge,
                        h.title.clone(),
                        [h.metric, max_metric],
                    )
                    .with_label("sensor")
                    .with_series(h.labels_key)
                } else {
                    WidgetInstance::new(id.clone(), WidgetKind::Stat, h.title.clone(), [h.metric])
                        .with_series(h.labels_key)
                }
            } else {
                WidgetInstance::new(id.clone(), WidgetKind::Stat, h.title.clone(), [h.metric])
                    .with_series(h.labels_key)
            };
            WidgetPreset {
                id,
                group: h.group,
                description: h.description,
                widget,
            }
        })
        .collect()
}

fn temp_group(kind: &str) -> String {
    let nice = match kind {
        "cpu" => "CPU",
        "gpu" => "GPU",
        "disk" => "Disk",
        "nic" => "Network",
        "acpi" => "ACPI",
        _ => "Other",
    };
    format!("Temperature ({nice})")
}

fn slug(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() {
        "sensor".into()
    } else {
        s
    }
}

fn unique_preset_id(used: &mut HashSet<String>, base: &str) -> String {
    let base = if base.is_empty() { "temp-sensor" } else { base };
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
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
            if let Some(s) = find_sample(latest, &w.metrics[0], w.series.as_deref()) {
                out.value = Some(s.value);
                out.display = format_value(s.value, unit);
            }
        }
        WidgetKind::Gauge => {
            if w.metrics.len() == 1 {
                if let Some(s) = find_sample(latest, &w.metrics[0], w.series.as_deref()) {
                    let ratio = s.value.clamp(0.0, 1.0);
                    out.value = Some(s.value);
                    out.ratio = Some(ratio);
                    out.display = format_value(ratio, "ratio");
                }
            } else if let (Some(used), Some(total)) = (
                find_sample(latest, &w.metrics[0], w.series.as_deref()),
                find_sample(latest, &w.metrics[1], w.series.as_deref()),
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
    if let Some(key) = w.series.as_deref().filter(|s| !s.is_empty()) {
        if let Some(points) = history.get(&(metric.clone(), key.to_string())) {
            out.spark = points
                .iter()
                .map(|(t, v)| SparkPoint { t: *t, v: *v })
                .collect();
        }
        if let Some(s) = find_sample(latest, metric, Some(key)) {
            out.value = Some(s.value);
            out.display = format_value(s.value, unit);
        } else if let Some(last) = out.spark.last() {
            out.value = Some(last.v);
            out.display = format_value(last.v, unit);
        }
        return;
    }
    if let Some(lname) = spark_label(w, latest, metric) {
        let labeled: Vec<&Sample> = latest
            .iter()
            .filter(|s| s.metric == *metric && label(s, lname).is_some())
            .collect();
        if !labeled.is_empty() {
            let names: Vec<&str> = labeled.iter().filter_map(|s| label(s, lname)).collect();
            let included: Vec<&str> = if lname == "device" {
                net::aggregate_ifaces(names, &settings.network_devices)
            } else {
                names
            };
            let mode = combine_mode(unit);
            let mut acc = 0.0;
            let mut n = 0u32;
            let mut keys = Vec::new();
            for s in labeled {
                let Some(dev) = label(s, lname) else {
                    continue;
                };
                if !included.contains(&dev) {
                    continue;
                }
                acc = combine_add(mode, acc, n, s.value);
                n += 1;
                keys.push(s.labels_key());
                out.rows.push(HydratedRow {
                    label: dev.into(),
                    display: format_value(s.value, unit),
                    ratio: None,
                });
            }
            out.rows.sort_by(|a, b| a.label.cmp(&b.label));
            if n > 0 {
                let value = combine_finish(mode, acc, n);
                out.value = Some(value);
                out.display = format_value(value, unit);
            }
            out.spark = merge_spark(history, metric, &keys, mode);
            if out.spark.is_empty() {
                out.spark = merge_spark(history, metric, &[String::new()], Combine::Sum);
            }
            return;
        }
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
    mode: Combine,
) -> Vec<SparkPoint> {
    let mut by_t: std::collections::BTreeMap<i64, (f64, u32)> = std::collections::BTreeMap::new();
    for lk in label_keys {
        if let Some(points) = history.get(&(metric.to_string(), lk.clone())) {
            for (t, v) in points {
                let e = by_t.entry(*t).or_insert((0.0, 0));
                e.0 = combine_add(mode, e.0, e.1, *v);
                e.1 += 1;
            }
        }
    }
    by_t.into_iter()
        .map(|(t, (v, n))| SparkPoint {
            t,
            v: combine_finish(mode, v, n),
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Combine {
    Sum,
    Mean,
    Max,
}

fn combine_mode(unit: &str) -> Combine {
    match unit {
        "ratio" => Combine::Mean,
        "celsius" => Combine::Max,
        _ => Combine::Sum,
    }
}

fn combine_add(mode: Combine, acc: f64, n: u32, value: f64) -> f64 {
    match mode {
        Combine::Sum | Combine::Mean => acc + value,
        Combine::Max => {
            if n == 0 {
                value
            } else {
                acc.max(value)
            }
        }
    }
}

fn combine_finish(mode: Combine, acc: f64, n: u32) -> f64 {
    match mode {
        Combine::Mean if n > 0 => acc / f64::from(n),
        _ => acc,
    }
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
        .filter(|s| !(w.label.is_some() && s.labels.is_empty()))
        .filter(|s| {
            w.series
                .as_deref()
                .filter(|k| !k.is_empty())
                .is_none_or(|k| s.labels_key() == k)
        })
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
                .map(|t| (value / t).clamp(0.0, 1.0))
                .or_else(|| {
                    if unit == "ratio" {
                        Some(value.clamp(0.0, 1.0))
                    } else if unit == "celsius" {
                        Some((value / 100.0).clamp(0.0, 1.0))
                    } else {
                        None
                    }
                });
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

fn spark_label<'a>(w: &'a WidgetInstance, latest: &[Sample], metric: &str) -> Option<&'a str> {
    if let Some(name) = w.label.as_deref() {
        return Some(name);
    }
    ["device", "gpu", "sensor"].into_iter().find(|name| {
        latest
            .iter()
            .any(|s| s.metric == metric && label(s, name).is_some())
    })
}

fn find_sample<'a>(latest: &'a [Sample], name: &str, series: Option<&str>) -> Option<&'a Sample> {
    if let Some(key) = series.filter(|s| !s.is_empty()) {
        return latest
            .iter()
            .find(|s| s.metric == name && s.labels_key() == key);
    }
    find_unlabeled(latest, name)
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
        "celsius" => format!("{value:.0}°C"),
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
            series: None,
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
                series: None,
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

    #[test]
    fn presets_are_valid_and_cover_default() {
        let catalog = presets();
        let mut ids = std::collections::HashSet::new();
        for p in &catalog {
            assert!(ids.insert(p.id.as_str()), "duplicate preset {}", p.id);
            assert_eq!(p.id, p.widget.id);
            Dashboard {
                version: 1,
                widgets: vec![p.widget.clone()],
            }
            .validate()
            .unwrap_or_else(|e| panic!("{}: {e}", p.id));
        }
        for id in Dashboard::DEFAULT_IDS {
            assert!(catalog.iter().any(|p| p.id == *id), "default id {id}");
        }
    }

    #[test]
    fn gpu_bar_list_uses_ratio() {
        let samples = vec![Sample::new("node_gpu_usage_ratio", 0.4, 1)
            .with_label("gpu", "card0")
            .with_label("vendor", "amd")];
        let w = WidgetInstance::new(
            "gpu_list",
            WidgetKind::BarList,
            "GPUs",
            ["node_gpu_usage_ratio"],
        )
        .with_label("gpu");
        let rows = bar_rows(&w, &samples, "ratio");
        assert_eq!(rows.len(), 1);
        assert!((rows[0].ratio.unwrap() - 0.4).abs() < 1e-9);
        assert_eq!(rows[0].display, "40%");
    }

    #[test]
    fn formats_celsius() {
        assert_eq!(format_value(52.4, "celsius"), "52°C");
    }

    #[test]
    fn default_dashboard_skips_all_temps_bar() {
        assert!(!Dashboard::DEFAULT_IDS.contains(&"temps"));
        assert!(Dashboard::default_node()
            .widgets
            .iter()
            .any(|w| w.id == "cpu_temp"));
        assert!(presets().iter().any(|p| p.id == "temps"));
    }

    #[test]
    fn per_sensor_temp_presets_skip_hottest_aggregate() {
        let samples = vec![
            Sample::new("node_hwmon_temp_celsius", 45.0, 1)
                .with_label("sensor", "Package id 0")
                .with_label("chip", "coretemp")
                .with_label("kind", "cpu"),
            Sample::new("node_hwmon_temp_max_celsius", 90.0, 1)
                .with_label("sensor", "Package id 0")
                .with_label("chip", "coretemp")
                .with_label("kind", "cpu"),
            Sample::new("node_hwmon_temp_celsius", 38.0, 1)
                .with_label("sensor", "Composite")
                .with_label("chip", "nvme")
                .with_label("kind", "disk"),
            Sample::new("node_hwmon_temp_celsius", 45.0, 1),
            Sample::new("node_gpu_temperature_celsius", 62.0, 1)
                .with_label("gpu", "card0")
                .with_label("vendor", "amd"),
        ];
        let catalog = presets_for_samples(&samples);
        let pkg = catalog
            .iter()
            .find(|p| p.widget.title == "Package id 0")
            .unwrap();
        assert_eq!(pkg.group, "Temperature (CPU)");
        assert_eq!(pkg.widget.kind, WidgetKind::Gauge);
        assert_eq!(pkg.widget.metrics.len(), 2);
        let nvme = catalog
            .iter()
            .find(|p| p.widget.title == "Composite")
            .unwrap();
        assert_eq!(nvme.group, "Temperature (Disk)");
        assert_eq!(nvme.widget.kind, WidgetKind::Stat);
        assert!(catalog.iter().any(|p| p.widget.title == "card0 temp"));
        assert!(!catalog
            .iter()
            .any(|p| p.widget.series.as_deref() == Some("")));
        let dash = Dashboard {
            version: 1,
            widgets: vec![pkg.widget.clone(), nvme.widget.clone()],
        };
        let cards = hydrate(&dash, &samples, &HashMap::new(), &NodeSettings::default());
        assert_eq!(cards[0].display, "45°C / 90°C");
        assert!((cards[0].ratio.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(cards[1].display, "38°C");
    }

    #[test]
    fn temps_bar_list_skips_hottest_aggregate_and_uses_max() {
        let samples = vec![
            Sample::new("node_hwmon_temp_celsius", 45.0, 1)
                .with_label("sensor", "Package id 0")
                .with_label("chip", "coretemp")
                .with_label("kind", "cpu"),
            Sample::new("node_hwmon_temp_max_celsius", 90.0, 1)
                .with_label("sensor", "Package id 0")
                .with_label("chip", "coretemp")
                .with_label("kind", "cpu"),
            Sample::new("node_hwmon_temp_celsius", 45.0, 1),
        ];
        let w = WidgetInstance::new(
            "temps",
            WidgetKind::BarList,
            "Temperatures",
            ["node_hwmon_temp_celsius", "node_hwmon_temp_max_celsius"],
        )
        .with_label("sensor");
        let rows = bar_rows(&w, &samples, "celsius");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Package id 0");
        assert!((rows[0].ratio.unwrap() - 0.5).abs() < 1e-9);
        assert!(rows[0].display.contains("45°C"));
    }
}
