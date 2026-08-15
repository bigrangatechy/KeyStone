// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Client for the opt-in `keystone-sys` unix socket.

use std::time::Duration;

use anyhow::Context;
use keystone_core::sys::{parse_ip_addr_json, SysOp, SYS_SOCKET_PATH};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;

pub fn socket_path() -> String {
    std::env::var("KEYSTONE_SYS_SOCKET").unwrap_or_else(|_| SYS_SOCKET_PATH.to_string())
}

pub fn socket_present() -> bool {
    std::path::Path::new(&socket_path()).exists()
}

pub async fn call(op: SysOp, payload: Value) -> anyhow::Result<Value> {
    let mut stream = connect().await?;
    send_req(&mut stream, op, &payload).await?;
    let mut lines = BufReader::new(stream).lines();
    let mut last = json!({});
    while let Some(line) = lines.next_line().await? {
        let v: Value = serde_json::from_str(&line).context("sys reply")?;
        if v.get("t").and_then(|t| t.as_str()).is_some() && v.get("ok").is_none() {
            continue;
        }
        last = v;
    }
    finish(last)
}

pub async fn stream(
    op: SysOp,
    payload: Value,
    chunk_tx: mpsc::Sender<Vec<u8>>,
) -> anyhow::Result<Value> {
    let mut stream = connect().await?;
    send_req(&mut stream, op, &payload).await?;
    let mut lines = BufReader::new(stream).lines();
    let mut last = json!({"ok": true});
    while let Some(line) = lines.next_line().await? {
        let v: Value = serde_json::from_str(&line).context("sys reply")?;
        if let Some(t) = v.get("t").and_then(|t| t.as_str()) {
            let mut row = t.to_string();
            row.push('\n');
            let _ = chunk_tx.send(row.into_bytes()).await;
            continue;
        }
        last = v;
    }
    finish(last)
}

async fn connect() -> anyhow::Result<UnixStream> {
    connect_path(&socket_path()).await
}

async fn connect_path(path: &str) -> anyhow::Result<UnixStream> {
    let hint = "system helper is not running; enable keystone-sys.socket";
    match timeout(Duration::from_secs(2), UnixStream::connect(path)).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(_)) | Err(_) => anyhow::bail!("{hint}"),
    }
}

async fn send_req(stream: &mut UnixStream, op: SysOp, payload: &Value) -> anyhow::Result<()> {
    let req = json!({"op": op.as_str(), "payload": payload});
    stream.write_all(req.to_string().as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;
    Ok(())
}

fn finish(v: Value) -> anyhow::Result<Value> {
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        Ok(v.get("payload").cloned().unwrap_or(json!({})))
    } else {
        let err = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("system helper failed");
        anyhow::bail!("{err}")
    }
}

pub async fn local_status() -> Value {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default();
    let kernel = sysinfo::System::kernel_version().unwrap_or_default();
    let interfaces = local_addrs().await;
    json!({
        "hostname": hostname,
        "kernel": kernel,
        "helper_running": socket_present(),
        "reboot_required": std::path::Path::new("/run/reboot-required").is_file()
            || std::path::Path::new("/var/run/reboot-required").is_file(),
        "interfaces": interfaces,
    })
}

async fn local_addrs() -> Value {
    let output = Command::new("ip").args(["-j", "-4", "addr"]).output().await;
    match output {
        Ok(o) if o.status.success() => {
            serde_json::to_value(parse_ip_addr_json(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or(json!([]))
        }
        _ => json!([]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn missing_socket_fails_fast_with_enable_hint() {
        let path = format!(
            "{}/ks-sys-missing-{}.sock",
            std::env::temp_dir().display(),
            std::process::id()
        );
        let start = Instant::now();
        let err = connect_path(&path).await.unwrap_err().to_string();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(3),
            "missing helper must not hang"
        );
        assert!(
            err.contains("keystone-sys.socket"),
            "operator needs the systemctl unit name, got {err}"
        );
        assert!(!err.contains("docker.sock"), "{err}");
    }
}
