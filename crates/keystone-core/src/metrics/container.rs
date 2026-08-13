// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::metrics::{MetricType, Stability};

define_metric! {
    name: "container_cpu_usage_ratio",
    ty: MetricType::Gauge,
    unit: "ratio",
    help: "Coarse CPU usage ratio for a container (background push, not live stats)",
    labels: ["id", "name", "compose_project"],
    stability: Stability::Stable,
}

define_metric! {
    name: "container_memory_usage_bytes",
    ty: MetricType::Gauge,
    unit: "bytes",
    help: "Container memory usage",
    labels: ["id", "name", "compose_project"],
    stability: Stability::Stable,
}

define_metric! {
    name: "container_running",
    ty: MetricType::Gauge,
    unit: "boolean",
    help: "1 if the container is running",
    labels: ["id", "name", "compose_project"],
    stability: Stability::Stable,
}
