// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::metrics::{MetricType, Stability};

define_metric! {
    name: "snmp_sys_uptime_ticks",
    ty: MetricType::Gauge,
    unit: "ticks",
    help: "SNMP sysUpTime.0 (TimeTicks, hundredths of a second)",
    labels: ["target"],
    stability: Stability::Stable,
}

define_metric! {
    name: "snmp_scrape_ok",
    ty: MetricType::Gauge,
    unit: "boolean",
    help: "1 if the last SNMP scrape of this target succeeded",
    labels: ["target"],
    stability: Stability::Stable,
}
