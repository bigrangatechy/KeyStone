<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Widgets

The node Overview is a customisable grid. Layout JSON is stored on the node
row (`dashboard_json`) when the operator saves Customize; otherwise
`Dashboard::default_node()` is used.

## Kinds (`WidgetKind`)

Serialized snake_case. The UI, layout JSON, and hydrate all use this enum.

| Kind | Metrics | Draw |
|---|---|---|
| `stat` | 1 | Large formatted value from `metrics[0]`. Optional `series` filters `labels_key`. |
| `gauge` | 1 or 2 | One metric treated as 0–1, or used/total from two metrics. |
| `bar_list` | 1 or 2 | One row per labeled series. `label` is the row title key (e.g. `mountpoint`). `invert` means `metrics[0]` is remaining space. |
| `sparkline` | 1 | History of `metrics[0]` (about 15 minutes of retained points). |

`WidgetInstance.span` is 1–4 grid columns. Unknown JSON keys are ignored so
older servers can load a layout after a downgrade of optional fields.

## Add a card type or preset

1. New drawing style: add a `WidgetKind` variant, `description()`, hydrate
   in `hydrate_one`, and a branch in `crates/keystone-server/src/static/app.js`
   (`renderWidget`).
2. New picker entry: `presets()` in `widgets.rs` (id, group, description,
   `WidgetInstance`). Default Overview is `Dashboard::DEFAULT_IDS` — a subset
   of those ids.
3. Per-sensor temps: `presets_for_samples` appends cards after `presets()`,
   keyed by `labels_key`. Sensors with `node_hwmon_temp_max_celsius` become
   gauges; others are `stat`. Do not put every sensor on the default board.
4. Validate: `Dashboard::validate` checks version `1`, unique ids, span,
   catalog names, and metric arity per kind.

Saved custom layouts are left alone when you change the default. Operators
**Reset** to pick up a new built-in set.

## Built-in presets

These ids are what Customize lists before sample-driven temperature cards.

| Id | Group | Kind | Description |
|---|---|---|---|
| `cpu` | CPU | `gauge` | Donut of overall CPU usage |
| `cpu_spark` | CPU | `sparkline` | CPU usage over the last 15 minutes |
| `cpu_stat` | CPU | `stat` | CPU usage as a percentage |
| `cpu_temp` | CPU | `stat` | CPU package / SoC temperature |
| `cpu_temp_spark` | CPU | `sparkline` | CPU temperature over the last 15 minutes |
| `memory` | Memory | `gauge` | Donut of used / total RAM |
| `memory_spark` | Memory | `sparkline` | Used RAM over the last 15 minutes |
| `memory_stat` | Memory | `stat` | Used RAM as a number |
| `memory_avail` | Memory | `stat` | Memory available for new work |
| `load` | Load | `sparkline` | 1 minute load average sparkline |
| `load_stat` | Load | `stat` | 1 minute load average |
| `load5` | Load | `sparkline` | 5 minute load average sparkline |
| `load15` | Load | `stat` | 15 minute load average |
| `load15_spark` | Load | `sparkline` | 15 minute load average sparkline |
| `uptime` | System | `stat` | Time since last boot |
| `agent` | System | `stat` | Whether the agent is pushing |
| `temps` | System | `bar_list` | Every hardware sensor on one card |
| `hottest` | System | `stat` | Hottest sensor on the node |
| `disks` | Disk | `bar_list` | Used space per filesystem |
| `net_rx` | Network | `sparkline` | Receive rate sparkline |
| `net_tx` | Network | `sparkline` | Transmit rate sparkline |
| `net_rx_stat` | Network | `stat` | Current receive rate |
| `net_tx_stat` | Network | `stat` | Current transmit rate |
| `gpu` | GPU | `gauge` | Donut of GPU busy (average if several cards) |
| `gpu_mem` | GPU | `gauge` | Donut of GPU memory used / total |
| `gpu_spark` | GPU | `sparkline` | GPU busy over the last 15 minutes |
| `gpu_list` | GPU | `bar_list` | One busy bar per GPU |
| `gpu_temp` | GPU | `stat` | Hottest GPU temperature |
| `gpu_temps` | GPU | `bar_list` | Temperature per GPU |

Default ids: `cpu`, `cpu_temp`, `memory`, `load`, `uptime`, `disks`,
`load15`, `agent`, `net_rx`, `net_tx`, `gpu`, `gpu_mem`, `gpu_temp`.

## HTTP

`GET /api/v1/nodes/{id}/dashboard` returns `source` (`default` or `custom`),
`layout` (the `Dashboard`), and hydrated `widgets` for `app.js`. PUT saves
a validated layout; DELETE clears it.
