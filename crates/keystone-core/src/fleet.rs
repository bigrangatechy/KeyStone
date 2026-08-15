// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Compact CPU / memory / disk / temperature chips for the node list.

use serde::{Deserialize, Serialize};

use crate::sample::Sample;
use crate::widgets::format_value;

const NOISY_FSTYPE: &[&str] = &[
    "overlay", "tmpfs", "devtmpfs", "squashfs", "proc", "sysfs", "cgroup", "cgroup2", "nsfs",
    "autofs",
];

/// One health chip on the fleet home page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetChip {
    pub id: String,
    pub label: String,
    pub display: String,
    /// `ok`, `warn`, `crit`, or empty when unknown.
    pub tone: String,
    /// Extra context (mountpoint for disk).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hint: String,
}

impl FleetChip {
    /// Warn and crit chips are alerts; unknown (`—`) and ok are not.
    pub fn is_firing(&self) -> bool {
        self.tone == "warn" || self.tone == "crit"
    }
}

fn empty_chip(id: &str, label: &str) -> FleetChip {
    FleetChip {
        id: id.into(),
        label: label.into(),
        display: "—".into(),
        tone: String::new(),
        hint: String::new(),
    }
}

fn ratio_tone(ratio: Option<f64>) -> String {
    match ratio {
        Some(r) if r >= 0.9 => "crit".into(),
        Some(r) if r >= 0.75 => "warn".into(),
        Some(_) => "ok".into(),
        None => String::new(),
    }
}

fn temp_tone(celsius: f64) -> String {
    if celsius >= 90.0 {
        "crit".into()
    } else if celsius >= 75.0 {
        "warn".into()
    } else {
        "ok".into()
    }
}

fn unlabeled<'a>(samples: &'a [Sample], name: &str) -> Option<&'a Sample> {
    samples
        .iter()
        .find(|s| s.metric == name && s.labels.is_empty())
}

fn labeled<'a>(s: &'a Sample, name: &str) -> Option<&'a str> {
    s.labels
        .iter()
        .find(|l| l.name == name)
        .map(|l| l.value.as_str())
}

fn cpu_chip(samples: &[Sample]) -> FleetChip {
    match unlabeled(samples, "node_cpu_usage_ratio") {
        Some(s) => {
            let ratio = s.value.clamp(0.0, 1.0);
            FleetChip {
                id: "cpu".into(),
                label: "CPU".into(),
                display: format_value(ratio, "ratio"),
                tone: ratio_tone(Some(ratio)),
                hint: String::new(),
            }
        }
        None => empty_chip("cpu", "CPU"),
    }
}

fn memory_chip(samples: &[Sample]) -> FleetChip {
    let used = unlabeled(samples, "node_memory_used_bytes");
    let total = unlabeled(samples, "node_memory_total_bytes");
    match (used, total) {
        (Some(u), Some(t)) if t.value > 0.0 => {
            let ratio = (u.value / t.value).clamp(0.0, 1.0);
            FleetChip {
                id: "mem".into(),
                label: "RAM".into(),
                display: format_value(ratio, "ratio"),
                tone: ratio_tone(Some(ratio)),
                hint: format!(
                    "{} / {}",
                    format_value(u.value, "bytes"),
                    format_value(t.value, "bytes")
                ),
            }
        }
        _ => empty_chip("mem", "RAM"),
    }
}

fn disk_chip(samples: &[Sample]) -> FleetChip {
    let mut worst: Option<(f64, String)> = None;
    for size in samples
        .iter()
        .filter(|s| s.metric == "node_filesystem_size_bytes")
    {
        let fstype = labeled(size, "fstype").unwrap_or("");
        if NOISY_FSTYPE.contains(&fstype) || size.value <= 0.0 {
            continue;
        }
        let key = size.labels_key();
        let Some(avail) = samples
            .iter()
            .find(|s| s.metric == "node_filesystem_avail_bytes" && s.labels_key() == key)
        else {
            continue;
        };
        let used = (size.value - avail.value).max(0.0);
        let ratio = (used / size.value).clamp(0.0, 1.0);
        let mount = labeled(size, "mountpoint").unwrap_or("disk").to_string();
        if worst.as_ref().is_none_or(|(r, _)| ratio > *r) {
            worst = Some((ratio, mount));
        }
    }
    match worst {
        Some((ratio, mount)) => FleetChip {
            id: "disk".into(),
            label: "Disk".into(),
            display: format_value(ratio, "ratio"),
            tone: ratio_tone(Some(ratio)),
            hint: mount,
        },
        None => empty_chip("disk", "Disk"),
    }
}

fn temp_chip(samples: &[Sample]) -> FleetChip {
    let sample = unlabeled(samples, "node_cpu_temperature_celsius")
        .or_else(|| unlabeled(samples, "node_hwmon_temp_celsius"));
    match sample {
        Some(s) => FleetChip {
            id: "temp".into(),
            label: "Temp".into(),
            display: format_value(s.value, "celsius"),
            tone: temp_tone(s.value),
            hint: if s.metric == "node_cpu_temperature_celsius" {
                "CPU package".into()
            } else {
                "hottest sensor".into()
            },
        },
        None => empty_chip("temp", "Temp"),
    }
}

/// Four chips (CPU, RAM, disk, temp) for the fleet table. Missing series
/// render as `—`.
pub fn fleet_chips(samples: &[Sample]) -> Vec<FleetChip> {
    vec![
        cpu_chip(samples),
        memory_chip(samples),
        disk_chip(samples),
        temp_chip(samples),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_samples_are_dashes() {
        let chips = fleet_chips(&[]);
        assert_eq!(chips.len(), 4);
        assert!(chips.iter().all(|c| c.display == "—"));
        assert_eq!(chips[0].id, "cpu");
        assert_eq!(chips[1].id, "mem");
        assert_eq!(chips[2].id, "disk");
        assert_eq!(chips[3].id, "temp");
    }

    #[test]
    fn cpu_and_memory_percent() {
        let samples = vec![
            Sample::new("node_cpu_usage_ratio", 0.42, 1),
            Sample::new("node_memory_used_bytes", 4.0 * 1024.0 * 1024.0 * 1024.0, 1),
            Sample::new("node_memory_total_bytes", 8.0 * 1024.0 * 1024.0 * 1024.0, 1),
        ];
        let chips = fleet_chips(&samples);
        assert_eq!(chips[0].display, "42%");
        assert_eq!(chips[0].tone, "ok");
        assert_eq!(chips[1].display, "50%");
        assert_eq!(chips[1].tone, "ok");
        assert!(chips[1].hint.contains("GiB"));
    }

    #[test]
    fn disk_uses_worst_and_skips_overlay() {
        let samples = vec![
            Sample::new("node_filesystem_size_bytes", 100.0, 1)
                .with_label("mountpoint", "/")
                .with_label("fstype", "ext4"),
            Sample::new("node_filesystem_avail_bytes", 20.0, 1)
                .with_label("mountpoint", "/")
                .with_label("fstype", "ext4"),
            Sample::new("node_filesystem_size_bytes", 100.0, 1)
                .with_label("mountpoint", "/overlay")
                .with_label("fstype", "overlay"),
            Sample::new("node_filesystem_avail_bytes", 1.0, 1)
                .with_label("mountpoint", "/overlay")
                .with_label("fstype", "overlay"),
            Sample::new("node_filesystem_size_bytes", 50.0, 1)
                .with_label("mountpoint", "/boot")
                .with_label("fstype", "vfat"),
            Sample::new("node_filesystem_avail_bytes", 40.0, 1)
                .with_label("mountpoint", "/boot")
                .with_label("fstype", "vfat"),
        ];
        let disk = &fleet_chips(&samples)[2];
        assert_eq!(disk.display, "80%");
        assert_eq!(disk.tone, "warn");
        assert_eq!(disk.hint, "/");
    }

    #[test]
    fn temp_prefers_cpu_package() {
        let samples = vec![
            Sample::new("node_cpu_temperature_celsius", 48.0, 1),
            Sample::new("node_hwmon_temp_celsius", 91.0, 1),
        ];
        let temp = &fleet_chips(&samples)[3];
        assert_eq!(temp.display, "48°C");
        assert_eq!(temp.tone, "ok");
        let hot = fleet_chips(&[Sample::new("node_hwmon_temp_celsius", 91.0, 1)]);
        assert_eq!(hot[3].display, "91°C");
        assert_eq!(hot[3].tone, "crit");
    }

    #[test]
    fn high_cpu_is_crit() {
        let chips = fleet_chips(&[Sample::new("node_cpu_usage_ratio", 0.95, 1)]);
        assert_eq!(chips[0].tone, "crit");
        assert!(chips[0].is_firing());
        assert!(!fleet_chips(&[Sample::new("node_cpu_usage_ratio", 0.10, 1)])[0].is_firing());
    }
}
