<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Dashboards

This is the Netdata-shaped half of KeyStone: live host metrics on one page
per machine, customisable, without a metrics UI on every box. The **Nodes**
home page is the fleet scan: CPU, RAM, disk, and temperature chips for
every enrolled host, refreshed about once a second.

Each node Overview is a grid of cards. The page refreshes from the server at
the node’s **poll interval** (Settings, default **1 second**, range 1–60).
A connected agent is told to push at the same interval, so the donuts and
sparklines stay in step with collection.

## Default layout

Until you customise a node, Overview shows a built-in set:

- CPU usage donut and CPU package / SoC temperature
- Memory donut
- 1-minute load sparkline and 15-minute load
- Uptime and whether the agent is pushing
- Disk used space (one bar per filesystem)
- Network receive and transmit rate sparklines
- GPU busy, GPU memory, and hottest GPU temperature when those series exist

The default does **not** dump every hardware temperature sensor onto one
card. After the agent has pushed samples, **Customize** lists each hwmon and
GPU sensor so you can add only the chips you care about.

## Customize

Use **Customize** on the Overview toolbar.

- Drag a card onto another card to place it there. ↑ / ↓ still move one
  step if you prefer buttons. **+/−** change width (1–4 columns).
- The picker is grouped (CPU, Memory, Load, Disk, Network, GPU, System,
  Temperature).
- Built-in cards cover overall CPU, RAM, load, disks, NICs, GPU, uptime, and
  an “agent up” indicator.
- Per-sensor temperature cards appear after data exists. If the driver
  exposes a high/critical threshold, that card is a gauge against the max;
  otherwise it is a °C reading.
- **All temperatures** (every sensor on one bar card) is still in the picker
  if you want it; prefer per-sensor cards for a busy board.
- **Reset** drops a saved layout and returns to the built-in default. Cards
  you already saved stay until you reset or remove them.

While Customize is open, the toolbar also has **density** (compact /
comfortable / spacious), **cards** (bordered / flush / raised), and
**accent** (blue / green / amber / rose). Those apply to this node’s
Overview grid only — the site header stays the usual blue.

Each card has a **style** menu:

- Gauge: donut (default) or horizontal bar
- Sparkline: line (default) or filled area
- Stat: large (default) or compact
- Bar list: bars (default) or compact

Layouts are stored **per node**. Customising the NAS does not change the Pi.

## Card types

You do not pick these by name in the UI; they describe how a card draws.
**Customize** can change the drawing style without changing the metric:

- **Stat** — one value (uptime, load, a temperature, a rate). Large or compact.
- **Gauge** — 0–100% from a ratio, or used/total (memory, GPU memory, a
  sensor with a max). Donut or horizontal bar.
- **Bar list** — one bar per labeled series (filesystems, GPUs, all temps).
  Normal or compact.
- **Sparkline** — short history of one metric (about the last 15 minutes of
  retained points). Line or filled area.

## Temperatures

The agent reads Linux hwmon and GPU sensors.

- **CPU package** is Tctl / Package / the hottest CPU-class sensor, shown as
  one number on the default board.
- **GPU** temperature is labeled per card; the unlabeled series is the
  hottest GPU.
- Other chips (NVMe, NIC, ACPI, motherboard) show up as extra picker entries
  named after the sensor.

Empty temperature cards usually mean the kernel did not expose hwmon, or
you are on a VM without sensors. The **All samples** table at the bottom of
Overview shows whether `node_hwmon_temp_celsius` or
`node_cpu_temperature_celsius` arrived.

## Network widgets

Receive/transmit rates default to non-virtual interfaces (skip loopback,
docker, veth) summed for the sparkline. On the node Settings tab you can
list **Network interfaces** (one name per line) to pin which NICs count.
Leave the box empty for automatic selection.

## History

Sparklines use stored series. Retention is global **Settings** (default 24
hours, 1–8760). Shortening retention does not change the Overview layout; it
only shortens how far the lines go back.
