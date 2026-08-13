// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Network interface helpers shared by the agent and the node dashboard.

/// Loopback and container/VM bridges inflate totals if summed with the NIC.
pub fn skip_virtual_iface(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "lo"
        || n.starts_with("lo.")
        || n.starts_with("docker")
        || n.starts_with("br-")
        || n.starts_with("veth")
        || n.starts_with("virbr")
        || n.starts_with("tun")
        || n.starts_with("tap")
        || n.starts_with("cni")
        || n.starts_with("flannel")
        || n.starts_with("calico")
        || n.starts_with("kube-")
        || n.starts_with("dummy")
}

/// Whether this interface is included in the dashboard aggregate.
/// An empty `allow` list means automatic (skip virtual NICs, but never
/// skip *every* interface except `lo`).
pub fn include_iface(name: &str, allow: &[String]) -> bool {
    if !allow.is_empty() {
        return allow.iter().any(|a| a == name);
    }
    !skip_virtual_iface(name)
}

/// Names to sum for RX/TX widgets. Falls back to every non-loopback device
/// when the automatic filter would otherwise be empty (common on a box that
/// only has `lo` + `docker0` during a smoke test).
pub fn aggregate_ifaces<'a>(
    names: impl IntoIterator<Item = &'a str>,
    allow: &[String],
) -> Vec<&'a str> {
    let all: Vec<&str> = names.into_iter().collect();
    let picked: Vec<&str> = all
        .iter()
        .copied()
        .filter(|n| include_iface(n, allow))
        .collect();
    if !picked.is_empty() {
        return picked;
    }
    all.into_iter()
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            l != "lo" && !l.starts_with("lo.")
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfaceCounters {
    pub device: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errs: u64,
    pub tx_errs: u64,
}

/// Parse `/proc/net/dev`. Used on Linux so a smoke test does not depend on
/// sysinfo finding sysfs statistics.
pub fn parse_proc_net_dev(text: &str) -> Vec<IfaceCounters> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name == "face" || name == "Inter-" {
            continue;
        }
        let nums: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() < 10 {
            continue;
        }
        out.push(IfaceCounters {
            device: name.to_string(),
            rx_bytes: nums[0],
            rx_packets: nums[1],
            rx_errs: nums[2],
            tx_bytes: nums[8],
            tx_packets: nums[9],
            tx_errs: nums.get(10).copied().unwrap_or(0),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proc_net_dev_sample() {
        let text = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 100 1 0 0 0 0 0 0 100 1 0 0 0 0 0 0
  eth0: 2000 10 0 0 0 0 0 0 4000 20 1 0 0 0 0 0
";
        let ifaces = parse_proc_net_dev(text);
        assert_eq!(ifaces.len(), 2);
        assert_eq!(ifaces[1].device, "eth0");
        assert_eq!(ifaces[1].rx_bytes, 2000);
        assert_eq!(ifaces[1].tx_bytes, 4000);
        assert_eq!(ifaces[1].tx_errs, 1);
    }

    #[test]
    fn aggregate_falls_back_when_only_virtual() {
        let names = ["lo", "docker0"];
        let picked = aggregate_ifaces(names, &[]);
        assert_eq!(picked, vec!["docker0"]);
    }
}
