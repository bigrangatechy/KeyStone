// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Browse `_keystone._tcp.local.` for gRPC ingest. Token is never taken
//! from mDNS — still `ingest_token` in agent.toml.

use std::time::Duration;

use anyhow::Context;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

pub async fn discover_ingest_url() -> anyhow::Result<String> {
    let mdns = ServiceDaemon::new().context("mDNS daemon")?;
    let rx = mdns
        .browse(keystone_core::MDNS_SERVICE_TYPE)
        .context("mDNS browse")?;
    let found = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match rx.recv_async().await {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    if let Some(url) = url_from_service(&info) {
                        return Ok(url);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!("mDNS browse ended: {e}"));
                }
            }
        }
    })
    .await;
    let _ = mdns.shutdown();
    match found {
        Ok(Ok(url)) => Ok(url),
        Ok(Err(e)) => Err(e),
        Err(_) => anyhow::bail!(
            "no KeyStone server found via mDNS on this LAN; set ingest_url to http://<ui-host>:9100"
        ),
    }
}

fn url_from_service(info: &ServiceInfo) -> Option<String> {
    let scheme = info.get_property_val_str("scheme").unwrap_or("http");
    keystone_core::ingest_url_from_mdns(
        scheme,
        info.get_port(),
        info.get_addresses().iter().copied(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_record_picks_lan_over_docker0() {
        let info = ServiceInfo::new(
            keystone_core::MDNS_SERVICE_TYPE,
            "keystone",
            "keystone.local.",
            "127.0.0.1,172.17.0.1,192.168.1.20",
            9100,
            &[("scheme", "http")][..],
        )
        .unwrap();
        assert_eq!(
            url_from_service(&info).as_deref(),
            Some("http://192.168.1.20:9100")
        );
        assert!(info.get_property_val_str("ingest_token").is_none());
    }

    #[test]
    fn https_scheme_from_txt() {
        let info = ServiceInfo::new(
            keystone_core::MDNS_SERVICE_TYPE,
            "keystone",
            "keystone.local.",
            "10.0.0.5",
            9100,
            &[("scheme", "https")][..],
        )
        .unwrap();
        assert_eq!(
            url_from_service(&info).as_deref(),
            Some("https://10.0.0.5:9100")
        );
    }
}
