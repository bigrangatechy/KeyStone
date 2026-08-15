// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Root helper: allowlisted apt and IPv4. No `sh -c`. Started by systemd socket.

use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::Path;
use std::process::Stdio;

use anyhow::{anyhow, Context};
use keystone_core::sys::{
    netplan_yaml, nmcli_modify_args, parse_apt_simulate, parse_ip_addr_json, NetSet, SysOp,
    SYS_SOCKET_PATH,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::Command;
use tracing::{info, warn};

pub async fn run() -> anyhow::Result<()> {
    let listener = bind_listener().await?;
    info!("keystone-sys listening");
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_conn(stream).await {
                warn!("sys connection: {e}");
            }
        });
    }
}

async fn bind_listener() -> anyhow::Result<UnixListener> {
    if let Some(fd) = systemd_listen_fd() {
        // SAFETY: LISTEN_FDS=1 and LISTEN_PID is this process; fd 3 is the
        // socket unit's listening Unix socket (Accept=false).
        let std = unsafe { StdUnixListener::from_raw_fd(fd) };
        std.set_nonblocking(true)?;
        return UnixListener::from_std(std).context("LISTEN_FDS");
    }
    let path = std::env::var("KEYSTONE_SYS_SOCKET").unwrap_or_else(|_| SYS_SOCKET_PATH.to_string());
    let _ = std::fs::remove_file(&path);
    if let Some(dir) = Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    UnixListener::bind(&path).with_context(|| format!("bind {path}"))
}

fn systemd_listen_fd() -> Option<i32> {
    if std::env::var("LISTEN_FDS").ok().as_deref() != Some("1") {
        return None;
    }
    let pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    if pid != std::process::id() {
        return None;
    }
    Some(3)
}

async fn handle_conn(stream: UnixStream) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let Some(line) = lines.next_line().await? else {
        return Ok(());
    };
    let req: Value = serde_json::from_str(&line).context("sys request json")?;
    let op = req.get("op").and_then(|v| v.as_str()).unwrap_or("");
    let payload = req.get("payload").cloned().unwrap_or(json!({}));
    match dispatch(op, payload, &mut writer).await {
        Ok(()) => {}
        Err(e) => {
            let msg = json!({"ok": false, "error": e.to_string()});
            writer.write_all(msg.to_string().as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
    }
    writer.flush().await?;
    Ok(())
}

async fn write_json(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    v: &Value,
) -> anyhow::Result<()> {
    writer.write_all(v.to_string().as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn dispatch(
    op: &str,
    payload: Value,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> anyhow::Result<()> {
    let parsed = op
        .parse::<SysOp>()
        .map_err(|_| anyhow!("unknown sys op {op}"))?;
    match parsed {
        SysOp::Status => {
            let body = status().await?;
            write_json(writer, &json!({"ok": true, "payload": body})).await
        }
        SysOp::UpdatesList => {
            let body = updates_list().await?;
            write_json(writer, &json!({"ok": true, "payload": body})).await
        }
        SysOp::UpdatesApply => {
            updates_apply(writer).await?;
            write_json(writer, &json!({"ok": true, "payload": {"applied": true}})).await
        }
        SysOp::NetSet => {
            let raw = payload.to_string();
            let req = NetSet::parse_json(&raw).map_err(|e| anyhow!("{e}"))?;
            net_set(&req).await?;
            write_json(writer, &json!({"ok": true, "payload": {"ok": true}})).await
        }
    }
}

async fn status() -> anyhow::Result<Value> {
    let backend = detect_backend();
    let interfaces = ip_addrs().await;
    Ok(json!({
        "helper": true,
        "backend": backend,
        "reboot_required": reboot_required(),
        "interfaces": interfaces,
        "net": net_snapshot(backend, &interfaces),
    }))
}

fn detect_backend() -> &'static str {
    if Path::new("/etc/netplan").is_dir() {
        "netplan"
    } else if which("nmcli") {
        "networkmanager"
    } else {
        "unknown"
    }
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p).any(|dir| {
                let cand = dir.join(bin);
                cand.is_file()
            })
        })
        .unwrap_or(false)
}

fn reboot_required() -> bool {
    Path::new("/run/reboot-required").is_file() || Path::new("/var/run/reboot-required").is_file()
}

async fn ip_addrs() -> Value {
    let output = Command::new("ip").args(["-j", "-4", "addr"]).output().await;
    match output {
        Ok(o) if o.status.success() => {
            serde_json::to_value(parse_ip_addr_json(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or(json!([]))
        }
        _ => json!([]),
    }
}

fn net_snapshot(backend: &str, interfaces: &Value) -> Value {
    let first = interfaces
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|i| i.get("up").and_then(|u| u.as_bool()) == Some(true))
        })
        .or_else(|| interfaces.as_array().and_then(|a| a.first()));
    let iface = first
        .and_then(|i| i.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ipv4 = first
        .and_then(|i| i.get("ipv4"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let (address, prefix) = split_cidr(ipv4);
    let method = match backend {
        "netplan" => netplan_method(iface),
        "networkmanager" => "unknown".into(),
        _ => "unknown".into(),
    };
    json!({
        "iface": iface,
        "method": method,
        "address": address,
        "prefix": prefix,
        "gateway": "",
        "dns": []
    })
}

fn split_cidr(s: &str) -> (String, u8) {
    match s.split_once('/') {
        Some((a, p)) => (a.to_string(), p.parse().unwrap_or(0)),
        None if !s.is_empty() => (s.to_string(), 0),
        None => (String::new(), 0),
    }
}

fn netplan_method(iface: &str) -> String {
    let Ok(dir) = std::fs::read_dir("/etc/netplan") else {
        return "unknown".into();
    };
    let mut saw_iface = false;
    let mut dhcp = false;
    let mut static_addr = false;
    for ent in dir.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml")
            && path.extension().and_then(|e| e.to_str()) != Some("yml")
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !text.contains(iface) {
            continue;
        }
        saw_iface = true;
        if text.contains("dhcp4: true") {
            dhcp = true;
        }
        if text.contains("addresses:") {
            static_addr = true;
        }
    }
    if !saw_iface {
        "unknown".into()
    } else if dhcp && !static_addr {
        "dhcp".into()
    } else if static_addr {
        "static".into()
    } else {
        "unknown".into()
    }
}

async fn updates_list() -> anyhow::Result<Value> {
    apt_cmd(&["update"], false).await?;
    let stdout = apt_cmd(&["-s", "upgrade"], true).await?;
    Ok(json!({"packages": parse_apt_simulate(&stdout)}))
}

async fn updates_apply(writer: &mut tokio::net::unix::OwnedWriteHalf) -> anyhow::Result<()> {
    stream_apt(writer, &["update"]).await?;
    stream_apt(
        writer,
        &[
            "-y",
            "-o",
            "Dpkg::Options::=--force-confold",
            "-o",
            "Dpkg::Options::=--force-confdef",
            "upgrade",
        ],
    )
    .await
}

async fn apt_cmd(args: &[&str], capture: bool) -> anyhow::Result<String> {
    let mut cmd = Command::new("apt-get");
    cmd.args(args)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env("APT_LISTCHANGES_FRONTEND", "none")
        .stdin(Stdio::null());
    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let out = cmd.output().await.context("apt-get")?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        if !out.status.success() {
            anyhow::bail!("apt-get failed: {stderr}{stdout}");
        }
        Ok(stdout)
    } else {
        let st = cmd.status().await.context("apt-get")?;
        if !st.success() {
            anyhow::bail!("apt-get {args:?} failed");
        }
        Ok(String::new())
    }
}

async fn stream_apt(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    args: &[&str],
) -> anyhow::Result<()> {
    let mut child = Command::new("apt-get")
        .args(args)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env("APT_LISTCHANGES_FRONTEND", "none")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("apt-get")?;
    let stdout = child.stdout.take().context("apt stdout")?;
    let stderr = child.stderr.take().context("apt stderr")?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
    let tx_out = tx.clone();
    let tx_err = tx.clone();
    drop(tx);
    let h_out = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if tx_out.send(l).await.is_err() {
                break;
            }
        }
    });
    let h_err = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if tx_err.send(l).await.is_err() {
                break;
            }
        }
    });
    while let Some(l) = rx.recv().await {
        write_json(writer, &json!({"t": l})).await?;
    }
    let _ = h_out.await;
    let _ = h_err.await;
    let st = child.wait().await?;
    if !st.success() {
        anyhow::bail!("apt-get {args:?} failed");
    }
    Ok(())
}

async fn net_set(req: &NetSet) -> anyhow::Result<()> {
    let backend = detect_backend();
    match backend {
        "netplan" => {
            let yaml = netplan_yaml(req).map_err(|e| anyhow!("{e}"))?;
            let path = "/etc/netplan/99-keystone.yaml";
            tokio::fs::write(path, yaml)
                .await
                .with_context(|| format!("write {path}"))?;
            let st = Command::new("netplan")
                .arg("apply")
                .status()
                .await
                .context("netplan apply")?;
            if !st.success() {
                anyhow::bail!("netplan apply failed");
            }
        }
        "networkmanager" => {
            let con = nm_connection_for_iface(&req.iface).await?;
            let mut args = nmcli_modify_args(req).map_err(|e| anyhow!("{e}"))?;
            // nmcli_modify_args uses iface as the connection id; replace with NAME.
            if args.len() >= 3 {
                args[2] = con.clone();
            }
            let st = Command::new("nmcli")
                .args(&args)
                .status()
                .await
                .context("nmcli modify")?;
            if !st.success() {
                anyhow::bail!("nmcli modify failed");
            }
            let st = Command::new("nmcli")
                .args(["connection", "up", &con])
                .status()
                .await
                .context("nmcli up")?;
            if !st.success() {
                anyhow::bail!("nmcli connection up failed");
            }
        }
        _ => anyhow::bail!("no netplan or NetworkManager on this host"),
    }
    Ok(())
}

async fn nm_connection_for_iface(iface: &str) -> anyhow::Result<String> {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])
        .output()
        .await
        .context("nmcli connection show")?;
    let text = String::from_utf8_lossy(&out.stdout);
    connection_for_device(&text, iface)
        .ok_or_else(|| anyhow!("no NetworkManager connection for {iface}"))
}

pub fn connection_for_device(table: &str, iface: &str) -> Option<String> {
    for line in table.lines() {
        let Some((name, dev)) = line.rsplit_once(':') else {
            continue;
        };
        if dev.trim() == iface {
            let name = name.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nmcli_table_maps_iface_to_connection() {
        let table = "Wired connection 1:eth0\ndocker0:docker0\n";
        assert_eq!(
            connection_for_device(table, "eth0").as_deref(),
            Some("Wired connection 1")
        );
        assert!(connection_for_device(table, "wlan0").is_none());
    }

    #[test]
    fn unknown_op_is_rejected_before_apt() {
        assert!("not_an_op".parse::<SysOp>().is_err());
    }

    #[tokio::test]
    async fn helper_status_over_socket_without_apt() {
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind sys test sock");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"status","payload":{}}"#)
            .await
            .unwrap();
        client.write_all(b"\n").await.unwrap();
        client.flush().await.unwrap();
        let mut lines = BufReader::new(client).lines();
        let line = lines.next_line().await.unwrap().expect("reply");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v.get("ok").and_then(|o| o.as_bool()), Some(true), "{line}");
        assert!(
            v.get("payload").and_then(|p| p.get("backend")).is_some(),
            "status payload must include backend, got {line}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn helper_rejects_unknown_op_over_socket() {
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"not_an_op","payload":{}}"#)
            .await
            .unwrap();
        client.write_all(b"\n").await.unwrap();
        client.flush().await.unwrap();
        let mut lines = BufReader::new(client).lines();
        let line = lines.next_line().await.unwrap().expect("reply");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v.get("ok").and_then(|o| o.as_bool()), Some(false), "{line}");
        let err = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
        assert!(err.contains("unknown sys op"), "{err}");
        assert!(
            !err.contains("apt-get"),
            "unknown op must not run apt: {err}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn helper_rejects_shell_iface_over_socket() {
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"net_set","payload":{"iface":"eth0;rm","method":"dhcp"}}"#)
            .await
            .unwrap();
        client.write_all(b"\n").await.unwrap();
        client.flush().await.unwrap();
        let mut lines = BufReader::new(client).lines();
        let line = lines.next_line().await.unwrap().expect("reply");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v.get("ok").and_then(|o| o.as_bool()), Some(false), "{line}");
        let err = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
        assert!(
            err.contains("interface") || err.contains("invalid"),
            "shell iface must fail validation, got {err}"
        );
        server.await.unwrap();
    }

    struct SockGuard(std::path::PathBuf);
    impl Drop for SockGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            if let Some(dir) = self.0.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
    }

    fn scratch_sock() -> (std::path::PathBuf, SockGuard) {
        let dir = std::env::temp_dir().join(format!(
            "ks-sys-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sys.sock");
        (path.clone(), SockGuard(path))
    }
}
