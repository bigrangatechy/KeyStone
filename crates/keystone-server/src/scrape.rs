// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::time::Duration;

use keystone_core::config::{PrometheusScrape, SnmpScrape};
use keystone_core::node::NodeIdentity;
use keystone_core::sample::{self, Sample};
use tracing::{debug, warn};

use crate::state::AppState;

pub fn spawn(state: AppState) {
    tokio::spawn(supervisor(state));
}

async fn supervisor(state: AppState) {
    let mut epoch = u64::MAX;
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        let current = state.scrape_epoch();
        if current != epoch {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            epoch = current;
            let settings = state.server_settings();
            for job in settings.prometheus_scrape {
                let st = state.clone();
                tasks.spawn(async move { prometheus_loop(st, job).await });
            }
            for job in settings.snmp_scrape {
                let st = state.clone();
                tasks.spawn(async move { snmp_loop(st, job).await });
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn prometheus_loop(state: AppState, job: PrometheusScrape) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("reqwest");
    let mut tick = tokio::time::interval(Duration::from_secs(job.interval_secs.max(5)));
    loop {
        tick.tick().await;
        match scrape_prometheus(&client, &job.url).await {
            Ok(samples) => {
                let node_id = if job.node_id.is_empty() {
                    job.name.clone()
                } else {
                    job.node_id.clone()
                };
                let identity = NodeIdentity {
                    node_id: node_id.clone(),
                    hostname: job.name.clone(),
                    agent_version: "scrape".into(),
                    os: "prometheus".into(),
                    kernel: String::new(),
                    docker_version: None,
                    labels: vec![],
                };
                let _ = state.stores.metadata.upsert_heartbeat(&identity, true);
                let (kept, dropped) = sample::allowlist(samples);
                if dropped > 0 {
                    debug!(
                        "prometheus scrape {}: dropped {dropped} unknown metrics",
                        job.name
                    );
                }
                if let Err(e) = state.stores.series.write_samples(&node_id, &kept) {
                    warn!("store scrape {}: {e}", job.name);
                } else {
                    crate::alerts::note_samples(&state, &node_id, &kept);
                }
            }
            Err(e) => warn!("prometheus scrape {}: {e}", job.name),
        }
    }
}

async fn scrape_prometheus(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<Sample>> {
    let text = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse_prometheus_text(&text, crate_now_ms()))
}

pub fn parse_prometheus_text(text: &str, ts: i64) -> Vec<Sample> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(sample) = parse_prom_line(line, ts) {
            out.push(sample);
        }
    }
    out
}

fn parse_prom_line(line: &str, ts: i64) -> Option<Sample> {
    let (name_labels, rest) = line.rsplit_once(' ')?;
    let value: f64 = rest.trim().parse().ok()?;
    if let Some((name, labels_raw)) = name_labels.split_once('{') {
        let labels_raw = labels_raw.trim_end_matches('}');
        let mut sample = Sample::new(name, value, ts);
        for part in labels_raw.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (k, v) = part.split_once('=')?;
            let v = v.trim_matches('"');
            sample = sample.with_label(k.trim(), v);
        }
        Some(sample)
    } else {
        Some(Sample::new(name_labels, value, ts))
    }
}

async fn snmp_loop(state: AppState, job: SnmpScrape) {
    let mut tick = tokio::time::interval(Duration::from_secs(job.interval_secs.max(5)));
    loop {
        tick.tick().await;
        let node_id = if job.node_id.is_empty() {
            job.name.clone()
        } else {
            job.node_id.clone()
        };
        let ts = crate_now_ms();
        match snmp_sys_uptime(&job.target, &job.community).await {
            Ok(ticks) => {
                let identity = NodeIdentity {
                    node_id: node_id.clone(),
                    hostname: job.name.clone(),
                    agent_version: "scrape".into(),
                    os: "snmp".into(),
                    kernel: String::new(),
                    docker_version: None,
                    labels: vec![],
                };
                let _ = state.stores.metadata.upsert_heartbeat(&identity, true);
                let samples = vec![
                    Sample::new("snmp_sys_uptime_ticks", ticks, ts)
                        .with_label("target", &job.target),
                    Sample::new("snmp_scrape_ok", 1.0, ts).with_label("target", &job.target),
                ];
                let (kept, _) = sample::allowlist(samples);
                if state.stores.series.write_samples(&node_id, &kept).is_ok() {
                    crate::alerts::note_samples(&state, &node_id, &kept);
                }
            }
            Err(e) => {
                warn!("snmp scrape {}: {e}", job.name);
                let samples =
                    vec![Sample::new("snmp_scrape_ok", 0.0, ts).with_label("target", &job.target)];
                let (kept, _) = sample::allowlist(samples);
                if state.stores.series.write_samples(&node_id, &kept).is_ok() {
                    crate::alerts::note_samples(&state, &node_id, &kept);
                }
            }
        }
    }
}

/// Minimal SNMPv2c GET for sysUpTime.0 (1.3.6.1.2.1.1.3.0).
async fn snmp_sys_uptime(target: &str, community: &str) -> anyhow::Result<f64> {
    use tokio::net::UdpSocket;

    let dest = if target.contains(':') {
        target.to_string()
    } else {
        format!("{target}:161")
    };
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(&dest).await?;
    let packet = encode_snmp_get(community.as_bytes(), &[1, 3, 6, 1, 2, 1, 1, 3, 0]);
    socket.send(&packet).await?;
    let mut buf = [0u8; 1500];
    let n = tokio::time::timeout(Duration::from_secs(5), socket.recv(&mut buf)).await??;
    decode_timeticks(&buf[..n])
}

fn encode_snmp_get(community: &[u8], oid: &[u8]) -> Vec<u8> {
    // SNMPv2c GetRequest, request-id 1, sysUpTime
    let mut oid_enc = vec![0x06, oid.len() as u8];
    oid_enc.extend_from_slice(oid);
    // varbind: SEQUENCE { OID, NULL }
    let mut varbind = Vec::new();
    varbind.push(0x30);
    varbind.push((oid_enc.len() + 2) as u8);
    varbind.extend_from_slice(&oid_enc);
    varbind.extend_from_slice(&[0x05, 0x00]);
    // varbind list
    let mut vbl = vec![0x30, varbind.len() as u8];
    vbl.extend_from_slice(&varbind);
    // PDU: GetRequest (0xA0) request-id=1, error-status=0, error-index=0, vbl
    let mut pdu_inner = Vec::new();
    pdu_inner.extend_from_slice(&[0x02, 0x01, 0x01]); // request id 1
    pdu_inner.extend_from_slice(&[0x02, 0x01, 0x00]);
    pdu_inner.extend_from_slice(&[0x02, 0x01, 0x00]);
    pdu_inner.extend_from_slice(&vbl);
    let mut pdu = vec![0xA0, pdu_inner.len() as u8];
    pdu.extend_from_slice(&pdu_inner);
    let mut msg = Vec::new();
    msg.extend_from_slice(&[0x02, 0x01, 0x01]); // version v2c = 1
    msg.push(0x04);
    msg.push(community.len() as u8);
    msg.extend_from_slice(community);
    msg.extend_from_slice(&pdu);
    let mut out = vec![0x30, msg.len() as u8];
    out.extend_from_slice(&msg);
    out
}

fn decode_timeticks(bytes: &[u8]) -> anyhow::Result<f64> {
    // Hunt for a TimeTicks tag 0x43 and read integer contents.
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == 0x43 {
            let len = bytes[i + 1] as usize;
            if i + 2 + len <= bytes.len() {
                let mut v: u64 = 0;
                for b in &bytes[i + 2..i + 2 + len] {
                    v = (v << 8) | u64::from(*b);
                }
                return Ok(v as f64);
            }
        }
        i += 1;
    }
    anyhow::bail!("no TimeTicks in SNMP response")
}

fn crate_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_prometheus() {
        let text = r#"
# HELP node_load1
node_load1 0.42
node_cpu_usage_ratio 0.1
unknown_metric 9
"#;
        let samples = parse_prometheus_text(text, 1);
        assert!(samples.iter().any(|s| s.metric == "node_load1"));
        let (kept, dropped) = sample::allowlist(samples);
        assert_eq!(dropped, 1);
        assert!(kept.iter().any(|s| s.metric == "node_load1"));
    }
}
