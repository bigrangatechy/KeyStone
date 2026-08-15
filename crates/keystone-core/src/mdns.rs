// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! LAN DNS-SD type and ingest URL picking. No sockets — server and agent
//! own the mdns-sd daemon.

use std::net::IpAddr;

/// DNS-SD type the UI advertises and agents browse (`_keystone._tcp.local.`).
pub const MDNS_SERVICE_TYPE: &str = "_keystone._tcp.local.";

/// `ingest_url` values that mean “browse mDNS” instead of a fixed host.
pub fn wants_mdns(ingest_url: &str) -> bool {
    let s = ingest_url.trim();
    s.is_empty() || s.eq_ignore_ascii_case("mdns") || s.eq_ignore_ascii_case("mdns://")
}

/// Build `http(s)://host:port` from a resolved DNS-SD record.
///
/// Prefers reachable LAN IPv4 over docker0, link-local, and loopback.
/// Returns `None` when there is no usable address or the scheme/port is bad.
pub fn ingest_url_from_mdns(
    scheme: &str,
    port: u16,
    addrs: impl IntoIterator<Item = IpAddr>,
) -> Option<String> {
    if port == 0 {
        return None;
    }
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let mut best: Option<(i32, IpAddr)> = None;
    for ip in addrs {
        let score = mdns_addr_score(ip);
        if score < 0 {
            continue;
        }
        match best {
            None => best = Some((score, ip)),
            Some((s, cur)) => {
                if score > s || (score == s && ip_ord(ip) < ip_ord(cur)) {
                    best = Some((score, ip));
                }
            }
        }
    }
    let ip = best?.1;
    let host = match ip {
        IpAddr::V4(v) => v.to_string(),
        IpAddr::V6(v) => format!("[{v}]"),
    };
    Some(format!("{scheme}://{host}:{port}"))
}

fn ip_ord(ip: IpAddr) -> String {
    ip.to_string()
}

fn mdns_addr_score(ip: IpAddr) -> i32 {
    match ip {
        IpAddr::V4(v) => {
            if v.is_loopback()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_multicast()
                || v.is_link_local()
            {
                return -1;
            }
            let o = v.octets();
            if o[0] == 172 && o[1] == 17 {
                return 1; // default docker0
            }
            if o[0] == 192 && o[1] == 168 {
                return 40;
            }
            if o[0] == 10 {
                return 30;
            }
            if o[0] == 172 && (16..=31).contains(&o[1]) {
                return 20;
            }
            10
        }
        IpAddr::V6(v) => {
            if v.is_loopback() || v.is_unspecified() || v.is_multicast() {
                return -1;
            }
            if (v.segments()[0] & 0xffc0) == 0xfe80 {
                return -1;
            }
            5
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn wants_mdns_sentinels() {
        assert!(wants_mdns("mdns"));
        assert!(wants_mdns("MDNS"));
        assert!(wants_mdns(" mdns:// "));
        assert!(wants_mdns(""));
        assert!(!wants_mdns("http://127.0.0.1:9100"));
        assert!(!wants_mdns("https://keystone.home.arpa:9100"));
    }

    #[test]
    fn picks_lan_over_docker0_and_loopback() {
        let url = ingest_url_from_mdns(
            "http",
            9100,
            [
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V4(Ipv4Addr::new(172, 17, 0, 1)),
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
            ],
        )
        .unwrap();
        assert_eq!(url, "http://192.168.1.20:9100");
    }

    #[test]
    fn prefers_rfc1918_10_over_docker0() {
        let url = ingest_url_from_mdns(
            "https",
            9100,
            [
                IpAddr::V4(Ipv4Addr::new(172, 17, 0, 1)),
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            ],
        )
        .unwrap();
        assert_eq!(url, "https://10.0.0.5:9100");
    }

    #[test]
    fn brackets_ipv6() {
        let url = ingest_url_from_mdns(
            "http",
            9100,
            [IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1))],
        )
        .unwrap();
        assert_eq!(url, "http://[fd00::1]:9100");
    }

    #[test]
    fn skips_link_local_and_bad_scheme() {
        assert!(
            ingest_url_from_mdns("http", 9100, [IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))],)
                .is_none()
        );
        assert!(
            ingest_url_from_mdns("ftp", 9100, [IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))])
                .is_none()
        );
        assert!(
            ingest_url_from_mdns("http", 0, [IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))]).is_none()
        );
    }
}
