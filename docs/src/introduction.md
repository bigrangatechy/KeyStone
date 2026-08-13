<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# KeyStone

KeyStone is unlimited-node server monitoring with a per-node Docker control
plane. It is licensed under GPL-2.0-or-later. The software does not impose a
node cap.

Agents **push** catalog metrics (and optional container aggregates) over a
gRPC session. The same session carries Docker commands so NAT works. The
server can also **scrape** Prometheus exporters and SNMP devices. Each node
overview is a customisable widget dashboard (gauges, sparklines, disk bars, GPU);
new cards are added as a `WidgetKind` plus a picker preset, not a new page.

Reference chapters under *Generated reference* are produced by
`cargo xtask docs` from the Rust types this version compiles. The running
server serves the same text at `/help`. Do not edit `docs/src/generated/`.
