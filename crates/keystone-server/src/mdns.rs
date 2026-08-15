// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Advertise gRPC ingest on `_keystone._tcp.local.`. Never puts the ingest
//! token in TXT. Failure is non-fatal: agents can still use a fixed URL.

use std::sync::OnceLock;

use mdns_sd::{ServiceDaemon, ServiceInfo};
use tracing::{info, warn};

static DAEMON: OnceLock<ServiceDaemon> = OnceLock::new();

/// Publish ingest on the LAN. Keeps the daemon alive for the process.
pub fn advertise_ingest(grpc_listen: &str, ingest_tls: bool) {
    match try_advertise(grpc_listen, ingest_tls) {
        Ok(port) => info!(
            "mDNS advertised {} on UDP 5353 (ingest :{port})",
            keystone_core::MDNS_SERVICE_TYPE
        ),
        Err(e) => warn!("mDNS advertise skipped: {e}"),
    }
}

fn try_advertise(grpc_listen: &str, ingest_tls: bool) -> anyhow::Result<u16> {
    let port = grpc_port(grpc_listen);
    let service = ingest_service_info(
        keystone_core::MDNS_SERVICE_TYPE,
        &mdns_host_label(),
        port,
        ingest_tls,
    )?;
    let mdns = ServiceDaemon::new()?;
    mdns.register(service)?;
    let _ = DAEMON.set(mdns);
    Ok(port)
}

fn grpc_port(listen: &str) -> u16 {
    listen
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9100)
}

/// TXT is only `scheme`. Never the ingest token.
fn ingest_service_info(
    service_type: &str,
    instance: &str,
    port: u16,
    ingest_tls: bool,
) -> anyhow::Result<ServiceInfo> {
    let label = sanitize_label(instance);
    let host_name = format!("{label}.local.");
    let scheme = if ingest_tls { "https" } else { "http" };
    Ok(ServiceInfo::new(
        service_type,
        &label,
        &host_name,
        "",
        port,
        &[("scheme", scheme)][..],
    )?
    .enable_addr_auto())
}

fn mdns_host_label() -> String {
    let raw = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "keystone".into());
    sanitize_label(&raw)
}

fn sanitize_label(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if (c == '-' || c == '.') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "keystone".into()
    } else {
        out.chars().take(63).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdns_sd::ServiceEvent;
    use std::time::Duration;

    #[test]
    fn sanitizes_hostname() {
        assert_eq!(sanitize_label("Jessie-PC"), "jessie-pc");
        assert_eq!(sanitize_label("foo.local"), "foo-local");
        assert_eq!(sanitize_label("***"), "keystone");
    }

    #[test]
    fn grpc_port_from_listen_addrs() {
        assert_eq!(grpc_port("0.0.0.0:9100"), 9100);
        assert_eq!(grpc_port("127.0.0.1:9100"), 9100);
        assert_eq!(grpc_port("[::]:9100"), 9100);
        assert_eq!(grpc_port("not-a-port"), 9100);
    }

    #[test]
    fn advertised_txt_is_scheme_never_token() {
        let http = ingest_service_info(keystone_core::MDNS_SERVICE_TYPE, "Jessie-PC", 9100, false)
            .unwrap();
        assert_eq!(http.get_type(), keystone_core::MDNS_SERVICE_TYPE);
        assert_eq!(http.get_port(), 9100);
        assert_eq!(http.get_property_val_str("scheme"), Some("http"));
        assert!(http.get_property_val_str("ingest_token").is_none());
        assert!(http.get_property_val_str("token").is_none());
        assert!(http.get_fullname().starts_with("jessie-pc."));

        let tls = ingest_service_info(keystone_core::MDNS_SERVICE_TYPE, "ui", 9100, true).unwrap();
        assert_eq!(tls.get_property_val_str("scheme"), Some("https"));
        assert!(tls.get_property_val_str("ingest_token").is_none());
    }

    /// Same-host advertise + browse. Uses a unique type so a running KeyStone
    /// UI on the developer LAN cannot satisfy (or poison) the test.
    #[tokio::test]
    async fn advertise_is_browsable_on_this_host() {
        let ty = "_kstest._tcp.local.";
        let instance = format!("ks{}", std::process::id());
        let port = 19100;
        let daemon = ServiceDaemon::new().expect("mDNS daemon");
        let info = ingest_service_info(ty, &instance, port, false).unwrap();
        daemon.register(info).expect("register test service");
        let rx = daemon.browse(ty).expect("browse");
        let resolved = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv_async().await {
                    Ok(ServiceEvent::ServiceResolved(info)) if info.get_port() == port => {
                        return info;
                    }
                    Ok(_) => {}
                    Err(e) => panic!("mDNS browse ended: {e}"),
                }
            }
        })
        .await
        .expect("this host should see the record we just advertised (UDP 5353)");
        assert_eq!(resolved.get_port(), port);
        assert_eq!(resolved.get_property_val_str("scheme"), Some("http"));
        assert!(resolved.get_property_val_str("ingest_token").is_none());
        let _ = daemon.shutdown();
    }
}
