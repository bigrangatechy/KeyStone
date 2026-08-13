// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Linux hwmon + thermal-zone temperature collectors.

use std::path::Path;

use crate::sample::Sample;

#[derive(Debug, Clone, PartialEq)]
pub struct TempReading {
    pub sensor: String,
    pub chip: String,
    pub kind: String,
    pub celsius: f64,
    pub max_celsius: Option<f64>,
}

/// Read `/sys/class/hwmon` and `/sys/class/thermal`.
pub fn read_linux(hwmon_root: &Path, thermal_root: &Path) -> Vec<TempReading> {
    let mut out = read_hwmon(hwmon_root);
    let have_cpu = out.iter().any(|r| r.kind == "cpu");
    out.extend(read_thermal(thermal_root, have_cpu));
    out.sort_by(|a, b| a.sensor.cmp(&b.sensor));
    out
}

pub fn readings_to_samples(readings: &[TempReading], ts: i64) -> Vec<Sample> {
    let mut samples = Vec::new();
    let mut hottest: Option<f64> = None;
    for r in readings {
        let labeled = |metric: &str, value: f64| {
            Sample::new(metric, value, ts)
                .with_label("sensor", &r.sensor)
                .with_label("chip", &r.chip)
                .with_label("kind", &r.kind)
        };
        samples.push(labeled("node_hwmon_temp_celsius", r.celsius));
        if let Some(max) = r.max_celsius {
            samples.push(labeled("node_hwmon_temp_max_celsius", max));
        }
        hottest = Some(hottest.map(|h| h.max(r.celsius)).unwrap_or(r.celsius));
    }
    if let Some(t) = hottest {
        samples.push(Sample::new("node_hwmon_temp_celsius", t, ts));
    }
    if let Some(t) = cpu_package_temp(readings) {
        samples.push(Sample::new("node_cpu_temperature_celsius", t, ts));
    }
    samples
}

pub fn cpu_package_temp(readings: &[TempReading]) -> Option<f64> {
    let cpus: Vec<&TempReading> = readings.iter().filter(|r| r.kind == "cpu").collect();
    let prefer = [
        "package",
        "tctl",
        "tdie",
        "cpu-thermal",
        "cpu_thermal",
        "soc",
    ];
    for needle in prefer {
        if let Some(r) = cpus.iter().find(|r| {
            r.sensor.to_ascii_lowercase().contains(needle)
                || r.chip.to_ascii_lowercase().contains(needle)
        }) {
            return Some(r.celsius);
        }
    }
    cpus.iter()
        .map(|r| r.celsius)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

fn read_hwmon(root: &Path) -> Vec<TempReading> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut chips: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("hwmon"))
        .collect();
    chips.sort();
    let mut out = Vec::new();
    for chip_dir in chips {
        let dir = root.join(&chip_dir);
        let chip = read_trim(&dir.join("name")).unwrap_or_else(|| chip_dir.clone());
        let instance = instance_name(&dir, &chip);
        let indexes = temp_indexes(&dir);
        for idx in indexes {
            let Some(celsius) = read_milli(&dir.join(format!("temp{idx}_input"))) else {
                continue;
            };
            if !plausible(celsius) {
                continue;
            }
            let label = read_trim(&dir.join(format!("temp{idx}_label")));
            let sensor = sensor_name(&chip, &instance, label.as_deref(), idx);
            let max = read_milli(&dir.join(format!("temp{idx}_max")))
                .or_else(|| read_milli(&dir.join(format!("temp{idx}_crit"))))
                .or_else(|| read_milli(&dir.join(format!("temp{idx}_emergency"))))
                .filter(|t| *t > 0.0);
            let kind = classify(&chip, &sensor, &instance);
            out.push(TempReading {
                sensor,
                chip: chip.clone(),
                kind: kind.into(),
                celsius,
                max_celsius: max,
            });
        }
    }
    out
}

fn read_thermal(root: &Path, have_cpu_hwmon: bool) -> Vec<TempReading> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut zones: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("thermal_zone"))
        .collect();
    zones.sort();
    let mut out = Vec::new();
    for zone in zones {
        let dir = root.join(&zone);
        if dir.join("hwmon").exists() {
            continue;
        }
        let ty = read_trim(&dir.join("type")).unwrap_or_else(|| zone.clone());
        let ty_l = ty.to_ascii_lowercase();
        if have_cpu_hwmon && (ty_l == "x86_pkg_temp" || ty_l == "acpitz") {
            continue;
        }
        let Some(celsius) = read_milli(&dir.join("temp")) else {
            continue;
        };
        if !plausible(celsius) {
            continue;
        }
        let kind = classify(&ty, &ty, "");
        out.push(TempReading {
            sensor: ty.clone(),
            chip: ty,
            kind: kind.into(),
            celsius,
            max_celsius: None,
        });
    }
    out
}

fn temp_indexes(dir: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut idxs: Vec<u32> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|name| {
            let rest = name.strip_prefix("temp")?.strip_suffix("_input")?;
            rest.parse().ok()
        })
        .collect();
    idxs.sort();
    idxs.dedup();
    idxs
}

fn instance_name(hwmon_dir: &Path, chip: &str) -> String {
    let Ok(target) = std::fs::read_link(hwmon_dir.join("device")) else {
        return chip.to_string();
    };
    let Some(base) = target.file_name().and_then(|s| s.to_str()) else {
        return chip.to_string();
    };
    let l = base.to_ascii_lowercase();
    if l.starts_with("nvme")
        || l.starts_with("ata")
        || l.starts_with("sd")
        || l.starts_with("mmc")
        || l.starts_with("wl")
        || l.starts_with("en")
        || l.starts_with("eth")
    {
        base.to_string()
    } else {
        chip.to_string()
    }
}

fn sensor_name(chip: &str, instance: &str, label: Option<&str>, idx: u32) -> String {
    let label = label
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("temp{idx}"));
    if instance != chip
        && !label
            .to_ascii_lowercase()
            .contains(&instance.to_ascii_lowercase())
    {
        format!("{instance} {label}")
    } else if label.to_ascii_lowercase().starts_with("temp") {
        format!("{chip} {label}")
    } else {
        label
    }
}

pub fn classify(chip: &str, sensor: &str, instance: &str) -> &'static str {
    let blob = format!("{chip} {sensor} {instance}").to_ascii_lowercase();
    if blob.contains("nvme")
        || blob.contains("drivetemp")
        || blob.contains("hddtemp")
        || blob.contains("sd ")
        || blob.contains("ata")
    {
        return "disk";
    }
    if blob.contains("coretemp")
        || blob.contains("k10temp")
        || blob.contains("zenpower")
        || blob.contains("cpu-thermal")
        || blob.contains("cpu_thermal")
        || blob.contains("soc_thermal")
        || blob.contains("raspberrypi")
        || blob.contains("package")
        || blob.contains("tctl")
        || blob.contains("tdie")
    {
        return "cpu";
    }
    if blob.contains("amdgpu")
        || blob.contains("nvidia")
        || blob.contains("nouveau")
        || blob.contains("i915")
        || blob.contains(" xe")
        || blob.starts_with("xe ")
        || blob.contains("v3d")
        || blob.contains("vc4")
        || blob.contains("gpu")
    {
        return "gpu";
    }
    if blob.contains("iwlwifi")
        || blob.contains("r8169")
        || blob.contains("mlx")
        || blob.contains("igb")
        || blob.contains("e1000")
        || blob.contains("ath")
        || blob.contains("nic")
        || blob.contains("wifi")
        || blob.contains("wlan")
    {
        return "nic";
    }
    if blob.contains("acpi") {
        return "acpi";
    }
    "other"
}

fn plausible(c: f64) -> bool {
    c.is_finite() && (-40.0..=150.0).contains(&c)
}

fn read_milli(path: &Path) -> Option<f64> {
    let n: f64 = read_trim(path)?.parse().ok()?;
    Some(n / 1000.0)
}

fn read_trim(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "keystone-temp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn hwmon_reads_every_input_and_skips_dup_thermal() {
        let root = tree();
        let hw = root.join("hwmon");
        let th = root.join("thermal");
        let cpu = hw.join("hwmon0");
        let nvme = hw.join("hwmon1");
        std::fs::create_dir_all(&cpu).unwrap();
        std::fs::create_dir_all(&nvme).unwrap();
        std::fs::create_dir_all(th.join("thermal_zone0")).unwrap();
        std::fs::create_dir_all(th.join("thermal_zone1")).unwrap();
        std::fs::write(cpu.join("name"), "coretemp\n").unwrap();
        std::fs::write(cpu.join("temp1_input"), "45000\n").unwrap();
        std::fs::write(cpu.join("temp1_label"), "Package id 0\n").unwrap();
        std::fs::write(cpu.join("temp1_max"), "80000\n").unwrap();
        std::fs::write(cpu.join("temp2_input"), "42000\n").unwrap();
        std::fs::write(cpu.join("temp2_label"), "Core 0\n").unwrap();
        std::fs::write(nvme.join("name"), "nvme\n").unwrap();
        std::fs::write(nvme.join("temp1_input"), "35000\n").unwrap();
        std::fs::write(nvme.join("temp1_label"), "Composite\n").unwrap();
        std::fs::write(nvme.join("temp1_crit"), "80000\n").unwrap();
        std::fs::write(th.join("thermal_zone0/type"), "x86_pkg_temp\n").unwrap();
        std::fs::write(th.join("thermal_zone0/temp"), "44900\n").unwrap();
        std::fs::write(th.join("thermal_zone1/type"), "wifi\n").unwrap();
        std::fs::write(th.join("thermal_zone1/temp"), "41000\n").unwrap();

        let readings = read_linux(&hw, &th);
        let _ = std::fs::remove_dir_all(&root);

        assert!(readings
            .iter()
            .any(|r| r.sensor == "Package id 0" && r.kind == "cpu"));
        assert!(readings.iter().any(|r| r.sensor == "Core 0"));
        assert!(readings
            .iter()
            .any(|r| r.sensor == "Composite" && r.kind == "disk"));
        assert!(readings.iter().any(|r| r.sensor == "wifi"));
        assert!(!readings.iter().any(|r| r.sensor == "x86_pkg_temp"));
        assert_eq!(cpu_package_temp(&readings), Some(45.0));

        let samples = readings_to_samples(&readings, 1);
        assert!(samples
            .iter()
            .any(|s| s.metric == "node_cpu_temperature_celsius" && s.labels.is_empty()));
        assert!(samples.iter().any(|s| s.metric == "node_hwmon_temp_celsius"
            && s.labels.is_empty()
            && s.value >= 45.0));
        assert!(samples.iter().any(|s| {
            s.metric == "node_hwmon_temp_max_celsius"
                && label_of(s, "sensor") == Some("Package id 0")
                && (s.value - 80.0).abs() < 1e-9
        }));
    }

    #[test]
    fn pi_thermal_zone_when_no_hwmon() {
        let root = tree();
        let hw = root.join("hwmon");
        let th = root.join("thermal/thermal_zone0");
        std::fs::create_dir_all(&hw).unwrap();
        std::fs::create_dir_all(&th).unwrap();
        std::fs::write(th.join("type"), "cpu-thermal\n").unwrap();
        std::fs::write(th.join("temp"), "51234\n").unwrap();
        let readings = read_linux(&hw, &root.join("thermal"));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].kind, "cpu");
        assert!((readings[0].celsius - 51.234).abs() < 0.001);
        assert_eq!(cpu_package_temp(&readings), Some(readings[0].celsius));
    }

    fn label_of<'a>(s: &'a crate::sample::Sample, name: &str) -> Option<&'a str> {
        s.labels
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.value.as_str())
    }
}
