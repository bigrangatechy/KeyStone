// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! GPU collectors shared by the agent. Parsers are pure so they can be tested
//! without NVIDIA/AMD drivers.

use std::path::Path;

use crate::sample::Sample;

#[derive(Debug, Clone, PartialEq)]
pub struct GpuReading {
    pub name: String,
    pub vendor: String,
    pub usage_ratio: Option<f64>,
    pub mem_used_bytes: Option<f64>,
    pub mem_total_bytes: Option<f64>,
    pub temp_celsius: Option<f64>,
}

/// `nvidia-smi --query-gpu=index,name,utilization.gpu,utilization.memory,memory.used,memory.total,temperature.gpu --format=csv,noheader,nounits`
pub fn parse_nvidia_smi(csv: &str) -> Vec<GpuReading> {
    let mut out = Vec::new();
    for (i, line) in csv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        if cols.len() < 7 {
            continue;
        }
        let index = cols[0];
        let name = if cols[1].is_empty() {
            format!("nvidia-{index}")
        } else {
            cols[1].to_string()
        };
        out.push(GpuReading {
            name: unique_name(&name, i),
            vendor: "nvidia".into(),
            usage_ratio: parse_percent_ratio(cols[2]),
            mem_used_bytes: parse_mib_bytes(cols[4]),
            mem_total_bytes: parse_mib_bytes(cols[5]),
            temp_celsius: parse_number(cols[6]),
        });
    }
    out
}

pub fn parse_vcgencmd_temp(text: &str) -> Option<f64> {
    let text = text.trim();
    let rest = text.strip_prefix("temp=")?;
    let num = rest.split('\'').next()?.trim();
    parse_number(num)
}

/// DRM primary nodes (`card0`, `card1`, …) under `/sys/class/drm`.
pub fn read_drm(drm_root: &Path) -> Vec<GpuReading> {
    let Ok(entries) = std::fs::read_dir(drm_root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_primary_card(n))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for (i, card) in names.iter().enumerate() {
        let device = drm_root.join(card).join("device");
        if !device.is_dir() {
            continue;
        }
        let vendor = vendor_from_id(read_trim(&device.join("vendor")).as_deref());
        let usage =
            read_trim(&device.join("gpu_busy_percent")).and_then(|s| parse_percent_ratio(&s));
        let mem_used = read_u64(&device.join("mem_info_vram_used")).map(|n| n as f64);
        let mem_total = read_u64(&device.join("mem_info_vram_total")).map(|n| n as f64);
        let temp = drm_temp_c(&device);
        if usage.is_none() && mem_used.is_none() && mem_total.is_none() && temp.is_none() {
            continue;
        }
        let pretty = drm_pretty_name(&device, card);
        out.push(GpuReading {
            name: unique_name(&pretty, i),
            vendor,
            usage_ratio: usage,
            mem_used_bytes: mem_used,
            mem_total_bytes: mem_total,
            temp_celsius: temp,
        });
    }
    out
}

pub fn readings_to_samples(readings: &[GpuReading], ts: i64) -> Vec<Sample> {
    let mut samples = Vec::new();
    let mut usage_sum = 0.0;
    let mut usage_n = 0u32;
    let mut mem_used = 0.0;
    let mut mem_total = 0.0;
    let mut have_mem = false;
    let mut hottest: Option<f64> = None;
    for g in readings {
        let labeled = |metric: &str, value: f64| {
            Sample::new(metric, value, ts)
                .with_label("gpu", &g.name)
                .with_label("vendor", &g.vendor)
        };
        if let Some(u) = g.usage_ratio {
            samples.push(labeled("node_gpu_usage_ratio", u.clamp(0.0, 1.0)));
            usage_sum += u.clamp(0.0, 1.0);
            usage_n += 1;
        }
        if let Some(v) = g.mem_used_bytes {
            samples.push(labeled("node_gpu_memory_used_bytes", v));
            mem_used += v;
            have_mem = true;
        }
        if let Some(v) = g.mem_total_bytes {
            samples.push(labeled("node_gpu_memory_total_bytes", v));
            mem_total += v;
            have_mem = true;
        }
        if let Some(t) = g.temp_celsius {
            samples.push(labeled("node_gpu_temperature_celsius", t));
            hottest = Some(hottest.map(|h| h.max(t)).unwrap_or(t));
        }
    }
    if usage_n > 0 {
        samples.push(Sample::new(
            "node_gpu_usage_ratio",
            usage_sum / f64::from(usage_n),
            ts,
        ));
    }
    if have_mem {
        samples.push(Sample::new("node_gpu_memory_used_bytes", mem_used, ts));
        samples.push(Sample::new("node_gpu_memory_total_bytes", mem_total, ts));
    }
    if let Some(t) = hottest {
        samples.push(Sample::new("node_gpu_temperature_celsius", t, ts));
    }
    samples
}

fn is_primary_card(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("card") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

fn unique_name(name: &str, index: usize) -> String {
    if name.is_empty() {
        format!("gpu-{index}")
    } else {
        name.to_string()
    }
}

fn parse_percent_ratio(s: &str) -> Option<f64> {
    parse_number(s).map(|v| (v / 100.0).clamp(0.0, 1.0))
}

fn parse_mib_bytes(s: &str) -> Option<f64> {
    parse_number(s).map(|mib| mib * 1024.0 * 1024.0)
}

fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("[n/a]") || s.eq_ignore_ascii_case("n/a") {
        return None;
    }
    s.split_whitespace().next()?.parse().ok()
}

fn vendor_from_id(raw: Option<&str>) -> String {
    let id = raw
        .unwrap_or("")
        .trim()
        .trim_start_matches("0x")
        .to_ascii_lowercase();
    match id.as_str() {
        "10de" => "nvidia".into(),
        "1002" => "amd".into(),
        "8086" => "intel".into(),
        "14e4" => "broadcom".into(),
        _ => "unknown".into(),
    }
}

fn drm_pretty_name(device: &Path, card: &str) -> String {
    if let Some(uevent) = read_trim(&device.join("uevent")) {
        for line in uevent.lines() {
            if let Some(driver) = line.strip_prefix("DRIVER=") {
                let driver = driver.trim();
                if !driver.is_empty() {
                    return format!("{driver} ({card})");
                }
            }
        }
    }
    card.to_string()
}

fn drm_temp_c(device: &Path) -> Option<f64> {
    let hwmon = device.join("hwmon");
    let entries = std::fs::read_dir(hwmon).ok()?;
    let mut temps = Vec::new();
    for ent in entries.flatten() {
        let p = ent.path().join("temp1_input");
        if let Some(milli) = read_u64(&p) {
            temps.push(milli as f64 / 1000.0);
        }
    }
    temps.into_iter().reduce(f64::max)
}

fn read_trim(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_u64(path: &Path) -> Option<u64> {
    read_trim(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_smi_csv() {
        let csv = "0, NVIDIA GeForce RTX 3080, 23, 8, 2048, 10240, 52\n\
             1, NVIDIA GeForce RTX 3080, [N/A], 0, 512, 10240, 41\n";
        let gpus = parse_nvidia_smi(csv);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].vendor, "nvidia");
        assert!((gpus[0].usage_ratio.unwrap() - 0.23).abs() < 1e-9);
        assert_eq!(gpus[0].mem_used_bytes, Some(2048.0 * 1024.0 * 1024.0));
        assert!(gpus[1].usage_ratio.is_none());
        let samples = readings_to_samples(&gpus, 1);
        assert!(samples
            .iter()
            .any(|s| s.metric == "node_gpu_usage_ratio" && s.labels.is_empty()));
        assert!(samples.iter().any(|s| {
            s.metric == "node_gpu_memory_used_bytes"
                && s.labels.is_empty()
                && (s.value - 2560.0 * 1024.0 * 1024.0).abs() < 1.0
        }));
    }

    #[test]
    fn vcgencmd_temp() {
        assert_eq!(parse_vcgencmd_temp("temp=45.2'C"), Some(45.2));
        assert_eq!(parse_vcgencmd_temp("temp=51.0'C\n"), Some(51.0));
    }

    #[test]
    fn drm_skips_connectors_and_reads_amd() {
        let root = std::env::temp_dir().join(format!(
            "keystone-drm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let card = root.join("card0").join("device");
        std::fs::create_dir_all(card.join("hwmon/hwmon0")).unwrap();
        std::fs::create_dir_all(root.join("card0-HDMI-A-1")).unwrap();
        std::fs::write(card.join("vendor"), "0x1002\n").unwrap();
        std::fs::write(card.join("gpu_busy_percent"), "41\n").unwrap();
        std::fs::write(card.join("mem_info_vram_used"), "1073741824\n").unwrap();
        std::fs::write(card.join("mem_info_vram_total"), "8589934592\n").unwrap();
        std::fs::write(card.join("hwmon/hwmon0/temp1_input"), "62000\n").unwrap();
        std::fs::write(card.join("uevent"), "DRIVER=amdgpu\n").unwrap();
        let gpus = read_drm(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].vendor, "amd");
        assert_eq!(gpus[0].name, "amdgpu (card0)");
        assert!((gpus[0].usage_ratio.unwrap() - 0.41).abs() < 1e-9);
        assert_eq!(gpus[0].temp_celsius, Some(62.0));
    }
}
