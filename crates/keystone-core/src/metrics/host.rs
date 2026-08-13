// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::metrics::{MetricType, Stability};

define_metric! {
    name: "node_cpu_usage_ratio",
    ty: MetricType::Gauge,
    unit: "ratio",
    help: "CPU usage ratio across all cores (0–1)",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_memory_total_bytes",
    ty: MetricType::Gauge,
    unit: "bytes",
    help: "Total physical memory",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_memory_available_bytes",
    ty: MetricType::Gauge,
    unit: "bytes",
    help: "Estimate of memory available for starting new applications",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_memory_used_bytes",
    ty: MetricType::Gauge,
    unit: "bytes",
    help: "Used physical memory",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_load1",
    ty: MetricType::Gauge,
    unit: "load",
    help: "1 minute load average",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_load5",
    ty: MetricType::Gauge,
    unit: "load",
    help: "5 minute load average",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_load15",
    ty: MetricType::Gauge,
    unit: "load",
    help: "15 minute load average",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_filesystem_size_bytes",
    ty: MetricType::Gauge,
    unit: "bytes",
    help: "Filesystem size",
    labels: ["device", "mountpoint", "fstype"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_filesystem_avail_bytes",
    ty: MetricType::Gauge,
    unit: "bytes",
    help: "Filesystem space available to non-root users",
    labels: ["device", "mountpoint", "fstype"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_boot_time_seconds",
    ty: MetricType::Gauge,
    unit: "seconds",
    help: "Unix timestamp when the node last booted",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_uptime_seconds",
    ty: MetricType::Gauge,
    unit: "seconds",
    help: "Seconds since boot",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "keystone_agent_up",
    ty: MetricType::Gauge,
    unit: "boolean",
    help: "1 while the agent is running and pushing",
    labels: [],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_network_receive_bytes_total",
    ty: MetricType::Counter,
    unit: "bytes",
    help: "Total bytes received on a network interface",
    labels: ["device"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_network_transmit_bytes_total",
    ty: MetricType::Counter,
    unit: "bytes",
    help: "Total bytes transmitted on a network interface",
    labels: ["device"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_network_receive_packets_total",
    ty: MetricType::Counter,
    unit: "packets",
    help: "Total packets received on a network interface",
    labels: ["device"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_network_transmit_packets_total",
    ty: MetricType::Counter,
    unit: "packets",
    help: "Total packets transmitted on a network interface",
    labels: ["device"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_network_receive_errs_total",
    ty: MetricType::Counter,
    unit: "errors",
    help: "Receive errors on a network interface",
    labels: ["device"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_network_transmit_errs_total",
    ty: MetricType::Counter,
    unit: "errors",
    help: "Transmit errors on a network interface",
    labels: ["device"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_network_receive_bytes_per_second",
    ty: MetricType::Gauge,
    unit: "bytes_per_second",
    help: "Receive rate. Labeled by device; unlabeled series is the sum of non-virtual interfaces",
    labels: ["device"],
    stability: Stability::Stable,
}

define_metric! {
    name: "node_network_transmit_bytes_per_second",
    ty: MetricType::Gauge,
    unit: "bytes_per_second",
    help: "Transmit rate. Labeled by device; unlabeled series is the sum of non-virtual interfaces",
    labels: ["device"],
    stability: Stability::Stable,
}
