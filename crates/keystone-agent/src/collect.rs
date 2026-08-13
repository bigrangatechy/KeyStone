// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use keystone_core::config::AgentConfig;
use keystone_core::gpu::{self, GpuReading};
use keystone_core::net::{self, IfaceCounters};
use keystone_core::sample::Sample;
use keystone_core::temp::{self, TempReading};
use sysinfo::{Components, Disks, Networks, System};

fn host_sys() -> std::sync::MutexGuard<'static, System> {
    static SYS: OnceLock<Mutex<System>> = OnceLock::new();
    SYS.get_or_init(|| Mutex::new(System::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn collect_host(cfg: &AgentConfig) -> Vec<Sample> {
    let ts = now_ms();
    let mut samples = Vec::new();
    let (cpu, total_mem, avail_mem, used_mem) = {
        let mut sys = host_sys();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        (
            sys.global_cpu_usage() as f64 / 100.0,
            sys.total_memory() as f64,
            sys.available_memory() as f64,
            sys.used_memory() as f64,
        )
    };

    samples.push(Sample::new("node_cpu_usage_ratio", cpu, ts));
    samples.push(Sample::new("node_memory_total_bytes", total_mem, ts));
    samples.push(Sample::new("node_memory_available_bytes", avail_mem, ts));
    samples.push(Sample::new("node_memory_used_bytes", used_mem, ts));

    let load = System::load_average();
    samples.push(Sample::new("node_load1", load.one, ts));
    samples.push(Sample::new("node_load5", load.five, ts));
    samples.push(Sample::new("node_load15", load.fifteen, ts));

    let boot = System::boot_time();
    samples.push(Sample::new("node_boot_time_seconds", boot as f64, ts));
    let uptime = System::uptime();
    samples.push(Sample::new("node_uptime_seconds", uptime as f64, ts));
    samples.push(Sample::new("keystone_agent_up", 1.0, ts));

    let disks = Disks::new_with_refreshed_list();
    for disk in disks.list() {
        let device = disk.name().to_string_lossy().into_owned();
        let mount = disk.mount_point().to_string_lossy().into_owned();
        let fstype = String::from_utf8_lossy(disk.file_system().as_encoded_bytes()).into_owned();
        samples.push(
            Sample::new("node_filesystem_size_bytes", disk.total_space() as f64, ts)
                .with_label("device", &device)
                .with_label("mountpoint", &mount)
                .with_label("fstype", &fstype),
        );
        samples.push(
            Sample::new(
                "node_filesystem_avail_bytes",
                disk.available_space() as f64,
                ts,
            )
            .with_label("device", &device)
            .with_label("mountpoint", &mount)
            .with_label("fstype", &fstype),
        );
    }

    let _ = cfg;
    collect_network(&mut samples, ts);
    collect_gpu(&mut samples, ts);
    collect_temps(&mut samples, ts);
    samples
}

#[derive(Clone)]
struct NetSnap {
    ts_ms: i64,
    rx: HashMap<String, u64>,
    tx: HashMap<String, u64>,
}

fn read_ifaces() -> Vec<IfaceCounters> {
    if cfg!(target_os = "linux") {
        if let Ok(text) = std::fs::read_to_string("/proc/net/dev") {
            let parsed = net::parse_proc_net_dev(&text);
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }
    sysinfo_ifaces()
}

fn sysinfo_ifaces() -> Vec<IfaceCounters> {
    let nets = Networks::new_with_refreshed_list();
    nets.iter()
        .map(|(name, data)| IfaceCounters {
            device: name.to_string(),
            rx_bytes: data.total_received(),
            tx_bytes: data.total_transmitted(),
            rx_packets: data.total_packets_received(),
            tx_packets: data.total_packets_transmitted(),
            rx_errs: data.total_errors_on_received(),
            tx_errs: data.total_errors_on_transmitted(),
        })
        .collect()
}

fn collect_network(samples: &mut Vec<Sample>, ts: i64) {
    static PREV: Mutex<Option<NetSnap>> = Mutex::new(None);

    let ifaces = read_ifaces();
    let mut now_rx = HashMap::new();
    let mut now_tx = HashMap::new();

    let prev = PREV.lock().ok().and_then(|g| g.clone());
    let dt = prev
        .as_ref()
        .map(|p| (ts - p.ts_ms) as f64 / 1000.0)
        .filter(|d| *d >= 0.5);

    for iface in &ifaces {
        now_rx.insert(iface.device.clone(), iface.rx_bytes);
        now_tx.insert(iface.device.clone(), iface.tx_bytes);

        samples.push(
            Sample::new(
                "node_network_receive_bytes_total",
                iface.rx_bytes as f64,
                ts,
            )
            .with_label("device", &iface.device),
        );
        samples.push(
            Sample::new(
                "node_network_transmit_bytes_total",
                iface.tx_bytes as f64,
                ts,
            )
            .with_label("device", &iface.device),
        );
        samples.push(
            Sample::new(
                "node_network_receive_packets_total",
                iface.rx_packets as f64,
                ts,
            )
            .with_label("device", &iface.device),
        );
        samples.push(
            Sample::new(
                "node_network_transmit_packets_total",
                iface.tx_packets as f64,
                ts,
            )
            .with_label("device", &iface.device),
        );
        samples.push(
            Sample::new("node_network_receive_errs_total", iface.rx_errs as f64, ts)
                .with_label("device", &iface.device),
        );
        samples.push(
            Sample::new("node_network_transmit_errs_total", iface.tx_errs as f64, ts)
                .with_label("device", &iface.device),
        );

        let (rx_rate, tx_rate) = match (prev.as_ref(), dt) {
            (Some(prev), Some(dt)) => {
                let prx = prev
                    .rx
                    .get(&iface.device)
                    .copied()
                    .unwrap_or(iface.rx_bytes);
                let ptx = prev
                    .tx
                    .get(&iface.device)
                    .copied()
                    .unwrap_or(iface.tx_bytes);
                let rx = if iface.rx_bytes >= prx {
                    (iface.rx_bytes - prx) as f64 / dt
                } else {
                    0.0
                };
                let tx = if iface.tx_bytes >= ptx {
                    (iface.tx_bytes - ptx) as f64 / dt
                } else {
                    0.0
                };
                (rx, tx)
            }
            _ => (0.0, 0.0),
        };
        samples.push(
            Sample::new("node_network_receive_bytes_per_second", rx_rate, ts)
                .with_label("device", &iface.device),
        );
        samples.push(
            Sample::new("node_network_transmit_bytes_per_second", tx_rate, ts)
                .with_label("device", &iface.device),
        );
    }

    let names: Vec<&str> = ifaces.iter().map(|i| i.device.as_str()).collect();
    let agg = net::aggregate_ifaces(names, &[]);
    let mut agg_rx = 0.0;
    let mut agg_tx = 0.0;
    for s in samples.iter() {
        let Some(dev) = s
            .labels
            .iter()
            .find(|l| l.name == "device")
            .map(|l| l.value.as_str())
        else {
            continue;
        };
        if !agg.contains(&dev) {
            continue;
        }
        if s.metric == "node_network_receive_bytes_per_second" {
            agg_rx += s.value;
        }
        if s.metric == "node_network_transmit_bytes_per_second" {
            agg_tx += s.value;
        }
    }
    samples.push(Sample::new(
        "node_network_receive_bytes_per_second",
        agg_rx,
        ts,
    ));
    samples.push(Sample::new(
        "node_network_transmit_bytes_per_second",
        agg_tx,
        ts,
    ));

    if let Ok(mut g) = PREV.lock() {
        *g = Some(NetSnap {
            ts_ms: ts,
            rx: now_rx,
            tx: now_tx,
        });
    }
}

fn collect_gpu(samples: &mut Vec<Sample>, ts: i64) {
    let mut readings = Vec::new();
    if let Some(csv) = run_limited(
        "nvidia-smi",
        &[
            "--query-gpu=index,name,utilization.gpu,utilization.memory,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ],
        Duration::from_millis(400),
    ) {
        readings.extend(gpu::parse_nvidia_smi(&csv));
    }
    let have_nvidia = readings.iter().any(|g| g.vendor == "nvidia");
    for g in gpu::read_drm(Path::new("/sys/class/drm")) {
        if g.vendor == "nvidia" && have_nvidia {
            continue;
        }
        readings.push(g);
    }
    if readings.is_empty() {
        if let Some(out) = run_limited("vcgencmd", &["measure_temp"], Duration::from_millis(200)) {
            if let Some(t) = gpu::parse_vcgencmd_temp(&out) {
                readings.push(GpuReading {
                    name: "VideoCore".into(),
                    vendor: "broadcom".into(),
                    usage_ratio: None,
                    mem_used_bytes: None,
                    mem_total_bytes: None,
                    temp_celsius: Some(t),
                });
            }
        }
    }
    samples.extend(gpu::readings_to_samples(&readings, ts));
}

fn collect_temps(samples: &mut Vec<Sample>, ts: i64) {
    let mut readings = temp::read_linux(
        Path::new("/sys/class/hwmon"),
        Path::new("/sys/class/thermal"),
    );
    if readings.is_empty() {
        readings.extend(sysinfo_temps());
    }
    samples.extend(temp::readings_to_samples(&readings, ts));
}

fn sysinfo_temps() -> Vec<TempReading> {
    let components = Components::new_with_refreshed_list();
    let mut out = Vec::new();
    for c in components.list() {
        let Some(temp) = c.temperature() else {
            continue;
        };
        if !temp.is_finite() || !(-40.0..=150.0).contains(&temp) {
            continue;
        }
        let name = c.label();
        if name.trim().is_empty() {
            continue;
        }
        let kind = temp::classify(name, name, "");
        out.push(TempReading {
            sensor: name.to_string(),
            chip: "sysinfo".into(),
            kind: kind.into(),
            celsius: f64::from(temp),
            max_celsius: None,
        });
    }
    out
}

fn run_limited(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut buf = String::new();
                child.stdout.as_mut()?.read_to_string(&mut buf).ok()?;
                return Some(buf);
            }
            Ok(Some(_)) => return None,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(_) => return None,
        }
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_emits_per_device_and_aggregate_rates() {
        let cfg = AgentConfig::default();
        let samples = collect_host(&cfg);
        assert!(samples
            .iter()
            .any(|s| s.metric == "node_network_receive_bytes_total"));
        assert!(samples.iter().any(|s| {
            s.metric == "node_network_receive_bytes_per_second" && s.labels.is_empty()
        }));
        assert!(samples.iter().any(|s| {
            s.metric == "node_network_transmit_bytes_per_second" && s.labels.is_empty()
        }));
    }
}
