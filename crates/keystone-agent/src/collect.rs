// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::time::{SystemTime, UNIX_EPOCH};

use keystone_core::config::AgentConfig;
use keystone_core::sample::Sample;
use sysinfo::{Disks, System};

pub fn collect_host(cfg: &AgentConfig) -> Vec<Sample> {
    let ts = now_ms();
    let mut samples = Vec::new();
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu = sys.global_cpu_usage() as f64 / 100.0;
    samples.push(Sample::new("node_cpu_usage_ratio", cpu, ts));
    samples.push(Sample::new(
        "node_memory_total_bytes",
        sys.total_memory() as f64,
        ts,
    ));
    samples.push(Sample::new(
        "node_memory_available_bytes",
        sys.available_memory() as f64,
        ts,
    ));
    samples.push(Sample::new(
        "node_memory_used_bytes",
        sys.used_memory() as f64,
        ts,
    ));

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
    samples
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
