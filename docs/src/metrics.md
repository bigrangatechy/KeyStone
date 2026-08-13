<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Metrics

The agent only pushes names KeyStone already knows. Scraped Prometheus text
is filtered the same way: unknown names never become series. That is
deliberate — a random exporter cannot fill the store with junk, and the
dashboard widgets always bind to a fixed catalog.

You do not configure the catalog. If a card is empty, the collector did not
see that signal (no GPU, no hwmon, Docker observe off, scrape job pointing
at the wrong URL).

**All samples** on a node Overview lists every stored series for that node.

## Host

| What you see | Catalog name | Notes |
|---|---|---|
| CPU donut | `node_cpu_usage_ratio` | 0–1 across all cores. |
| RAM donut | `node_memory_used_bytes` / `node_memory_total_bytes` | Available RAM is `node_memory_available_bytes`. |
| Load | `node_load1`, `node_load5`, `node_load15` | Usual Linux load averages. |
| Uptime | `node_uptime_seconds` | `node_boot_time_seconds` is the boot Unix time. |
| Agent | `keystone_agent_up` | 1 while this agent is pushing. |
| Disks | `node_filesystem_size_bytes`, `node_filesystem_avail_bytes` | Labeled `device`, `mountpoint`, `fstype`. Bars are used space. |
| NIC rates | `node_network_receive_bytes_per_second`, `node_network_transmit_bytes_per_second` | Labeled `device`; unlabeled series is the sum of non-virtual NICs (or the interfaces you listed in Settings). |
| NIC counters | `node_network_*_bytes_total`, `*_packets_total`, `*_errs_total` | Counters, labeled by `device`. |

## Temperatures and GPU

| What you see | Catalog name | Notes |
|---|---|---|
| CPU package | `node_cpu_temperature_celsius` | Tctl / Package / hottest CPU-class sensor. |
| hwmon chips | `node_hwmon_temp_celsius` | Labels `sensor`, `chip`, `kind` (`cpu`, `gpu`, `disk`, `nic`, `acpi`, `other`). Unlabeled series is the hottest reading. |
| Sensor max | `node_hwmon_temp_max_celsius` | Same labels, when the driver exposes a high/crit. Gauges use this as 100%. |
| GPU busy | `node_gpu_usage_ratio` | Labels `gpu`, `vendor`. Unlabeled = average of cards that report usage. |
| GPU memory | `node_gpu_memory_used_bytes`, `node_gpu_memory_total_bytes` | Unlabeled = sum across cards. |
| GPU temp | `node_gpu_temperature_celsius` | Unlabeled = hottest card. |

Per-sensor Overview cards are added from Customize after these samples
exist. See [Dashboards](dashboard.md).

## Containers

Pushed only when **Observe Docker** is on. These are coarse background
gauges (not the live stats stream on the Containers tab).

| Name | Labels |
|---|---|
| `container_cpu_usage_ratio` | `id`, `name`, `compose_project` |
| `container_memory_usage_bytes` | same |
| `container_running` | 1 if running |

## Scrapes

Prometheus jobs keep only allowlisted names (typically you scrape something
that already emits `node_*` if you want it on a KeyStone board). SNMP in
this version stores:

| Name | Meaning |
|---|---|
| `snmp_scrape_ok` | 1 if the last GET succeeded (`target` label). |
| `snmp_sys_uptime_ticks` | `sysUpTime.0` in hundredths of a second. |

Those samples attach to the scrape job’s `node_id` (or the job name), which
shows up as its own row on the nodes list.
