<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Alerts

A chip on the home page is an alert when it is **warn** or **crit**. The
thresholds are the same as the fleet colours — there is no second set of
numbers to configure in this version.

| Chip | Warn | Crit |
|---|---|---|
| CPU, RAM, disk | ≥ 75% | ≥ 90% |
| Temperature | ≥ 75°C | ≥ 90°C |

Disk is the fullest real filesystem (overlay, tmpfs, and similar skipped).
Temperature is the CPU package when the kernel exposes it, otherwise the
hottest unlabeled hwmon reading. Missing series (`—`) are not alerts.

## Where you see them

- Header **Alerts** with a count of what is firing now.
- The **Alerts** page: host, chip, value, and a short hint (mountpoint for
  disk).
- A red count next to the hostname on the node list.

The list is the current samples, not a history of incidents. When the chip
returns to ok, the row disappears. Last samples stay if the agent
disconnects, so a full disk still shows until new points arrive.

## Webhook

Optional. **Settings → Alerts → Webhook URL**. Empty is off.

KeyStone POSTs JSON when a chip **starts** firing, **changes** severity
(warn ↔ crit), or **clears**. A server restart does not re-send whatever
is already firing. Failures are written to the server journal; metric
ingest does not wait on the remote. Only `http://` and `https://` URLs are
accepted.

Example body:

```json
{
  "source": "keystone",
  "event": "firing",
  "node_id": "pi",
  "hostname": "raspberrypi",
  "chip": "disk",
  "label": "Disk",
  "severity": "crit",
  "display": "92%",
  "hint": "/",
  "at": "2026-08-15T03:38:00+00:00"
}
```

`event` is `firing` or `resolved`. `chip` is `cpu`, `mem`, `disk`, or
`temp`. The body includes the live value — treat the URL as something only
you should be able to change.
