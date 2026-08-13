<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Metrics catalog

Unknown names are dropped in `sample::allowlist` (agent push and Prometheus
scrape). The allowlist is `inventory` submissions from
`crates/keystone-core/src/metrics/` (`host.rs`, `container.rs`, `snmp.rs`).
`catalog()` returns them sorted by name. `is_known_metric` is what ingest,
widgets, and scrape use.

Do not duplicate this table in the operator book as the extension guide.
Operators get meaning and empty-card hints in `docs/src/metrics.md`. This
page is the list CI checks against `catalog()`.

## Add a metric

1. `define_metric!` in the matching `metrics/*.rs` module (name, type, unit,
   help, labels, stability).
2. Collect it in the agent (`keystone-agent` collectors) or in
   `keystone-server` scrape — catalog entry alone does nothing.
3. Mention `` `the_name` `` in **this** file (coverage test).
4. If operators should see it on Overview, add a widget preset (see
   [Widgets](widgets.md)) and a sentence in the operator metrics chapter.

Names are stable once shipped. Prefer a new name over silently changing
labels.

## Allowlist

| Name | Type | Unit | Labels | Help |
|---|---|---|---|---|
| `container_cpu_usage_ratio` | gauge | ratio | id, name, compose_project | Coarse CPU usage ratio for a container (background push, not live stats) |
| `container_memory_usage_bytes` | gauge | bytes | id, name, compose_project | Container memory usage |
| `container_running` | gauge | boolean | id, name, compose_project | 1 if the container is running |
| `keystone_agent_up` | gauge | boolean | — | 1 while the agent is running and pushing |
| `node_boot_time_seconds` | gauge | seconds | — | Unix timestamp when the node last booted |
| `node_cpu_temperature_celsius` | gauge | celsius | — | CPU package / SoC temperature (Tctl, Package, or hottest CPU sensor) |
| `node_cpu_usage_ratio` | gauge | ratio | — | CPU usage ratio across all cores (0–1) |
| `node_filesystem_avail_bytes` | gauge | bytes | device, mountpoint, fstype | Filesystem space available to non-root users |
| `node_filesystem_size_bytes` | gauge | bytes | device, mountpoint, fstype | Filesystem size |
| `node_gpu_memory_total_bytes` | gauge | bytes | gpu, vendor | GPU memory total. Labeled by gpu; unlabeled series is the sum across cards |
| `node_gpu_memory_used_bytes` | gauge | bytes | gpu, vendor | GPU memory used. Labeled by gpu; unlabeled series is the sum across cards |
| `node_gpu_temperature_celsius` | gauge | celsius | gpu, vendor | GPU temperature. Labeled by gpu; unlabeled series is the hottest card |
| `node_gpu_usage_ratio` | gauge | ratio | gpu, vendor | GPU busy ratio (0–1). Labeled by gpu; unlabeled series is the average of cards that report usage |
| `node_hwmon_temp_celsius` | gauge | celsius | sensor, chip, kind | Hardware monitor temperature. Labeled by sensor/chip/kind (cpu, gpu, disk, nic, acpi, other); unlabeled series is the hottest reading |
| `node_hwmon_temp_max_celsius` | gauge | celsius | sensor, chip, kind | High/critical threshold for the matching hwmon sensor, when the driver exposes one |
| `node_load1` | gauge | load | — | 1 minute load average |
| `node_load15` | gauge | load | — | 15 minute load average |
| `node_load5` | gauge | load | — | 5 minute load average |
| `node_memory_available_bytes` | gauge | bytes | — | Estimate of memory available for starting new applications |
| `node_memory_total_bytes` | gauge | bytes | — | Total physical memory |
| `node_memory_used_bytes` | gauge | bytes | — | Used physical memory |
| `node_network_receive_bytes_per_second` | gauge | bytes_per_second | device | Receive rate. Labeled by device; unlabeled series is the sum of non-virtual interfaces |
| `node_network_receive_bytes_total` | counter | bytes | device | Total bytes received on a network interface |
| `node_network_receive_errs_total` | counter | errors | device | Receive errors on a network interface |
| `node_network_receive_packets_total` | counter | packets | device | Total packets received on a network interface |
| `node_network_transmit_bytes_per_second` | gauge | bytes_per_second | device | Transmit rate. Labeled by device; unlabeled series is the sum of non-virtual interfaces |
| `node_network_transmit_bytes_total` | counter | bytes | device | Total bytes transmitted on a network interface |
| `node_network_transmit_errs_total` | counter | errors | device | Transmit errors on a network interface |
| `node_network_transmit_packets_total` | counter | packets | device | Total packets transmitted on a network interface |
| `node_uptime_seconds` | gauge | seconds | — | Seconds since boot |
| `snmp_scrape_ok` | gauge | boolean | target | 1 if the last SNMP scrape of this target succeeded |
| `snmp_sys_uptime_ticks` | gauge | ticks | target | SNMP sysUpTime.0 (TimeTicks, hundredths of a second) |

`Sample::labels_key` is the canonical label encoding widgets use for
`WidgetInstance.series` (one temperature sensor = one card).
