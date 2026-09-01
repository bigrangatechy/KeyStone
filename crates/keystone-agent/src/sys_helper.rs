// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Root helper: allowlisted apt (upgrade / autoremove), leftover services,
//! failed units, unit restart from those lists, reboot, journal follow, IPv4,
//! GitLab Omnibus backup, and unattended-upgrades observe. No `sh -c`.
//! Started by systemd socket.

use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::Path;
use std::process::Stdio;

use anyhow::{anyhow, Context};
use keystone_core::sys::{
    gitlab_backup_name_ok, journal_unit, merge_upgradable, netplan_yaml, newest_gitlab_backup,
    nmcli_modify_args, parse_apt_list_upgradable, parse_apt_simulate, parse_ip_addr_json,
    parse_needrestart_batch, parse_ntp_sync, parse_restart_unit, parse_systemctl_failed,
    parse_unattended_periodic, unit_listed_for_restart, NeedrestartBatch, NetSet, SysOp,
    GITLAB_BACKUP_BIN, GITLAB_BACKUP_DIR, SYS_SOCKET_PATH, UNATTENDED_AUTO_UPGRADES,
    UNATTENDED_LOG, UNATTENDED_STAMP, UNATTENDED_UPGRADE_BIN, UPDATES_LIST_CAP,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
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
        SysOp::UpdatesAutoremove => {
            updates_autoremove(writer).await?;
            write_json(writer, &json!({"ok": true, "payload": {"ok": true}})).await
        }
        SysOp::NetSet => {
            let raw = payload.to_string();
            let req = NetSet::parse_json(&raw).map_err(|e| anyhow!("{e}"))?;
            net_set(&req).await?;
            write_json(writer, &json!({"ok": true, "payload": {"ok": true}})).await
        }
        SysOp::GitlabBackup => {
            gitlab_backup(writer).await?;
            write_json(writer, &json!({"ok": true, "payload": {"ok": true}})).await
        }
        SysOp::Reboot => reboot(writer).await,
        SysOp::Journal => journal_follow(&payload, writer).await,
        SysOp::UnitRestart => {
            unit_restart(&payload).await?;
            write_json(writer, &json!({"ok": true, "payload": {"ok": true}})).await
        }
    }
}

async fn status() -> anyhow::Result<Value> {
    let backend = detect_backend();
    let (interfaces, leftovers, failed, ntp, unattended) = tokio::join!(
        ip_addrs(),
        leftover_services(),
        failed_units(),
        ntp_sync(),
        unattended_status()
    );
    Ok(json!({
        "helper": true,
        "backend": backend,
        "reboot_required": reboot_required() || leftovers.kernel_pending,
        "kernel_pending": leftovers.kernel_pending,
        "interfaces": interfaces,
        "net": net_snapshot(backend, &interfaces),
        "ntp": ntp,
        "unattended": unattended,
        "gitlab": gitlab_status(),
        "restart_services": leftovers.services,
        "failed_units": failed,
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

fn gitlab_kind() -> &'static str {
    if Path::new(GITLAB_BACKUP_BIN).is_file() {
        "omnibus"
    } else {
        "none"
    }
}

fn gitlab_status() -> Value {
    if gitlab_kind() != "omnibus" {
        return json!({ "kind": "none" });
    }
    match newest_gitlab_backup(&read_gitlab_backup_entries(Path::new(GITLAB_BACKUP_DIR))) {
        Some((backup_name, backup_unix)) => json!({
            "kind": "omnibus",
            "backup_name": backup_name,
            "backup_unix": backup_unix,
        }),
        None => json!({ "kind": "omnibus" }),
    }
}

fn file_mtime_unix(path: &str) -> Option<i64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let mtime = meta.modified().ok()?;
    mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

async fn unattended_status() -> Value {
    let available = Path::new(UNATTENDED_UPGRADE_BIN).is_file();
    if !available {
        return json!({ "available": false, "enabled": false });
    }
    let conf = std::fs::read_to_string(UNATTENDED_AUTO_UPGRADES).unwrap_or_default();
    let enabled = match parse_unattended_periodic(&conf) {
        Some(v) => v,
        None => unattended_unit_enabled().await,
    };
    match file_mtime_unix(UNATTENDED_STAMP).or_else(|| file_mtime_unix(UNATTENDED_LOG)) {
        Some(last_unix) => json!({
            "available": true,
            "enabled": enabled,
            "last_unix": last_unix,
        }),
        None => json!({ "available": true, "enabled": enabled }),
    }
}

async fn unattended_unit_enabled() -> bool {
    let output = timeout(
        Duration::from_secs(2),
        Command::new("systemctl")
            .args(["is-enabled", "unattended-upgrades"])
            .stdin(Stdio::null())
            .output(),
    )
    .await;
    match output {
        Ok(Ok(o)) => String::from_utf8_lossy(&o.stdout).trim() == "enabled",
        _ => false,
    }
}

fn read_gitlab_backup_entries(dir: &Path) -> Vec<(String, i64)> {
    let mut entries = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return entries;
    };
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if !gitlab_backup_name_ok(&name) {
            continue;
        }
        let Ok(ft) = ent.file_type() else {
            continue;
        };
        if ft.is_symlink() || !ft.is_file() {
            continue;
        }
        let Ok(meta) = ent.metadata() else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let unix = mtime
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        entries.push((name, unix));
    }
    entries
}

fn reboot_required() -> bool {
    Path::new("/run/reboot-required").is_file() || Path::new("/var/run/reboot-required").is_file()
}

async fn ip_addrs() -> Value {
    let output = timeout(
        Duration::from_secs(2),
        Command::new("ip").args(["-j", "-4", "addr"]).output(),
    )
    .await;
    match output {
        Ok(Ok(o)) if o.status.success() => {
            serde_json::to_value(parse_ip_addr_json(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or(json!([]))
        }
        _ => json!([]),
    }
}

async fn leftover_services() -> NeedrestartBatch {
    if !which("needrestart") {
        return NeedrestartBatch::default();
    }
    let output = timeout(
        Duration::from_secs(2),
        Command::new("needrestart")
            .args(["-b", "--restart=l"])
            .stdin(Stdio::null())
            .output(),
    )
    .await;
    match output {
        Ok(Ok(o)) => parse_needrestart_batch(&String::from_utf8_lossy(&o.stdout)),
        _ => NeedrestartBatch::default(),
    }
}

async fn failed_units() -> Vec<String> {
    let output = timeout(
        Duration::from_secs(2),
        Command::new("systemctl")
            .args(["--failed", "--plain", "--no-legend", "--no-pager"])
            .stdin(Stdio::null())
            .output(),
    )
    .await;
    match output {
        Ok(Ok(o)) => parse_systemctl_failed(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

async fn ntp_sync() -> Value {
    let output = timeout(
        Duration::from_secs(2),
        Command::new("timedatectl")
            .args(["show", "-p", "NTPSynchronized", "--value"])
            .stdin(Stdio::null())
            .output(),
    )
    .await;
    match output {
        Ok(Ok(o)) if o.status.success() => {
            if let Some(sync) = parse_ntp_sync(&String::from_utf8_lossy(&o.stdout)) {
                return json!({ "available": true, "synchronized": sync });
            }
        }
        _ => {}
    }
    json!({ "available": false, "synchronized": false })
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
    let mut pkgs = parse_apt_list_upgradable(&apt_list_upgradable().await.unwrap_or_default());
    let sim = apt_cmd(
        &[
            "-s",
            "-o",
            "APT::Get::Always-Include-Phased-Updates=true",
            "dist-upgrade",
        ],
        true,
    )
    .await
    .unwrap_or_default();
    merge_upgradable(&mut pkgs, parse_apt_simulate(&sim));
    let capped = pkgs.len() > UPDATES_LIST_CAP;
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    pkgs.truncate(UPDATES_LIST_CAP);
    Ok(json!({"packages": pkgs, "capped": capped}))
}

fn apply_apt_env(cmd: &mut Command) {
    cmd.env("DEBIAN_FRONTEND", "noninteractive")
        .env("APT_LISTCHANGES_FRONTEND", "none")
        .env("NEEDRESTART_MODE", "list");
}

async fn apt_list_upgradable() -> anyhow::Result<String> {
    let mut cmd = Command::new("apt");
    apply_apt_env(&mut cmd);
    let out = cmd
        .args(["-qq", "list", "--upgradable"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("apt list")?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(s)
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

async fn updates_autoremove(writer: &mut tokio::net::unix::OwnedWriteHalf) -> anyhow::Result<()> {
    if cfg!(test) {
        anyhow::bail!("autoremove is not invoked in tests");
    }
    stream_apt(writer, &["-y", "autoremove"]).await
}

async fn apt_cmd(args: &[&str], capture: bool) -> anyhow::Result<String> {
    let mut cmd = Command::new("apt-get");
    apply_apt_env(&mut cmd);
    cmd.args(args).stdin(Stdio::null());
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
    let mut cmd = Command::new("apt-get");
    apply_apt_env(&mut cmd);
    let mut child = cmd
        .args(args)
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

async fn gitlab_backup(writer: &mut tokio::net::unix::OwnedWriteHalf) -> anyhow::Result<()> {
    if gitlab_kind() != "omnibus" {
        anyhow::bail!(
            "GitLab Omnibus is not installed on this node ({GITLAB_BACKUP_BIN} missing). Docker GitLab is not in this version."
        );
    }
    let mut child = Command::new(GITLAB_BACKUP_BIN)
        .arg("create")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("gitlab-backup")?;
    let stdout = child.stdout.take().context("gitlab-backup stdout")?;
    let stderr = child.stderr.take().context("gitlab-backup stderr")?;
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
        anyhow::bail!("gitlab-backup create failed");
    }
    write_json(
        writer,
        &json!({
            "t": "Copy /etc/gitlab (gitlab.rb and gitlab-secrets.json) next to the archive. Restore is not in this UI."
        }),
    )
    .await?;
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

async fn reboot(writer: &mut tokio::net::unix::OwnedWriteHalf) -> anyhow::Result<()> {
    if cfg!(test) {
        anyhow::bail!("reboot is not invoked in tests");
    }
    write_json(writer, &json!({"ok": true, "payload": {"rebooting": true}})).await?;
    tokio::spawn(async {
        let _ = Command::new("systemctl").arg("reboot").status().await;
    });
    tokio::time::sleep(Duration::from_millis(250)).await;
    Ok(())
}

async fn unit_restart(payload: &Value) -> anyhow::Result<()> {
    let unit = parse_restart_unit(&payload.to_string()).map_err(|e| anyhow!("{e}"))?;
    if cfg!(test) {
        anyhow::bail!("unit restart is not invoked in tests");
    }
    let (leftovers, failed) = tokio::join!(leftover_services(), failed_units());
    if !unit_listed_for_restart(&unit, &leftovers.services, &failed) {
        anyhow::bail!("unit is not leftover or failed");
    }
    let st = Command::new("systemctl")
        .args(["restart", "--", &unit])
        .status()
        .await
        .context("systemctl restart")?;
    if !st.success() {
        anyhow::bail!("systemctl restart failed");
    }
    Ok(())
}

async fn journal_follow(
    payload: &Value,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> anyhow::Result<()> {
    let raw = payload.get("unit").and_then(|v| v.as_str()).unwrap_or("");
    let unit = journal_unit(raw).map_err(|_| anyhow!("unknown journal unit"))?;
    if cfg!(test) {
        anyhow::bail!("journal follow is not invoked in tests");
    }
    let mut child = Command::new("journalctl")
        .args([
            "-u",
            unit,
            "-n",
            "200",
            "-f",
            "--no-pager",
            "--output=short-iso",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("journalctl")?;
    let stdout = child.stdout.take().context("journalctl stdout")?;
    let stderr = child.stderr.take().context("journalctl stderr")?;
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
    let _ = child.wait().await;
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
        let kind = v
            .get("payload")
            .and_then(|p| p.get("gitlab"))
            .and_then(|g| g.get("kind"))
            .and_then(|k| k.as_str());
        assert!(
            kind == Some("omnibus") || kind == Some("none"),
            "status must report gitlab.kind, got {line}"
        );
        let payload = v.get("payload").expect("payload");
        assert!(
            payload
                .get("restart_services")
                .and_then(|x| x.as_array())
                .is_some(),
            "status must include leftover services, got {line}"
        );
        assert!(
            payload
                .get("failed_units")
                .and_then(|x| x.as_array())
                .is_some(),
            "status must include failed units, got {line}"
        );
        assert!(
            payload
                .get("ntp")
                .and_then(|n| n.get("available"))
                .and_then(|v| v.as_bool())
                .is_some(),
            "status ntp.available must be a bool, got {line}"
        );
        assert!(
            payload
                .get("ntp")
                .and_then(|n| n.get("synchronized"))
                .and_then(|v| v.as_bool())
                .is_some(),
            "status ntp.synchronized must be a bool, got {line}"
        );
        assert!(
            payload
                .get("unattended")
                .and_then(|n| n.get("available"))
                .and_then(|v| v.as_bool())
                .is_some(),
            "status unattended.available must be a bool, got {line}"
        );
        assert!(
            payload
                .get("unattended")
                .and_then(|n| n.get("enabled"))
                .and_then(|v| v.as_bool())
                .is_some(),
            "status unattended.enabled must be a bool, got {line}"
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
    async fn helper_gitlab_backup_without_omnibus_does_not_spawn() {
        if Path::new(GITLAB_BACKUP_BIN).is_file() {
            return;
        }
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"gitlab_backup","payload":{}}"#)
            .await
            .unwrap();
        client.write_all(b"\n").await.unwrap();
        client.flush().await.unwrap();
        let mut lines = BufReader::new(client).lines();
        let line = lines.next_line().await.unwrap().expect("reply");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v.get("ok").and_then(|o| o.as_bool()), Some(false), "{line}");
        let err = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
        assert!(err.contains("Omnibus"), "{err}");
        assert!(
            !err.contains("apt-get"),
            "missing GitLab must not run apt: {err}"
        );
        server.await.unwrap();
    }

    #[test]
    fn gitlab_backup_is_argv_not_shell() {
        let src = include_str!("sys_helper.rs");
        let body = src
            .split("async fn gitlab_backup")
            .nth(1)
            .expect("gitlab_backup")
            .split("async fn net_set")
            .next()
            .expect("gitlab_backup body");
        assert!(body.contains("GITLAB_BACKUP_BIN"));
        assert!(body.contains(".arg(\"create\")"));
        assert!(!body.contains("sh -c") && !body.contains("bash -c"));
    }

    #[test]
    fn updates_list_uses_upgradable_and_dist_upgrade_simulate() {
        let src = include_str!("sys_helper.rs");
        let list = src
            .split("async fn updates_list")
            .nth(1)
            .expect("updates_list")
            .split("async fn updates_apply")
            .next()
            .expect("updates_list body");
        assert!(
            list.contains("list") && list.contains("upgradable"),
            "Check for updates must use apt list --upgradable, not only apt-get -s upgrade"
        );
        assert!(
            list.contains("dist-upgrade"),
            "simulate dist-upgrade so held-back packages are listed"
        );
        assert!(
            !list.contains("\"upgrade\""),
            "listing must not use apt-get -s upgrade (misses kept-back and new apt output)"
        );
        let apply = src
            .split("async fn updates_apply")
            .nth(1)
            .expect("updates_apply")
            .split("async fn updates_autoremove")
            .next()
            .expect("updates_apply body");
        assert!(apply.contains("upgrade"));
        assert!(
            !apply.contains("dist-upgrade"),
            "Apply stays apt-get upgrade, not dist-upgrade"
        );
        let autoremove = src
            .split("async fn updates_autoremove")
            .nth(1)
            .expect("updates_autoremove")
            .split("async fn apt_cmd")
            .next()
            .expect("updates_autoremove body");
        assert!(autoremove.contains("autoremove"));
        assert!(autoremove.contains("stream_apt"));
        assert!(autoremove.contains("cfg!(test)"));
        assert!(!autoremove.contains("dist-upgrade"));
        assert!(!autoremove.contains("sh -c") && !autoremove.contains("bash -c"));
    }

    #[test]
    fn apt_invocations_set_needrestart_mode_list() {
        let src = include_str!("sys_helper.rs");
        let env = src
            .split("fn apply_apt_env")
            .nth(1)
            .expect("apply_apt_env")
            .split("async fn")
            .next()
            .expect("apply_apt_env body");
        assert!(
            env.contains("NEEDRESTART_MODE") && env.contains("\"list\""),
            "Ubuntu 24.04 needrestart must list, not auto-restart docker/ssh mid-upgrade"
        );
        let apt_cmd = src
            .split("async fn apt_cmd")
            .nth(1)
            .expect("apt_cmd")
            .split("async fn stream_apt")
            .next()
            .expect("apt_cmd body");
        assert!(apt_cmd.contains("apply_apt_env"), "{apt_cmd}");
        let stream = src
            .split("async fn stream_apt")
            .nth(1)
            .expect("stream_apt")
            .split("async fn gitlab_backup")
            .next()
            .expect("stream_apt body");
        assert!(stream.contains("apply_apt_env"), "{stream}");
        let list = src
            .split("async fn apt_list_upgradable")
            .nth(1)
            .expect("apt_list_upgradable")
            .split("async fn updates_apply")
            .next()
            .expect("apt_list_upgradable body");
        assert!(list.contains("apply_apt_env"), "{list}");
    }

    #[test]
    fn leftover_observe_does_not_restart_units() {
        let src = include_str!("sys_helper.rs");
        let leftover = src
            .split("async fn leftover_services")
            .nth(1)
            .expect("leftover_services")
            .split("async fn failed_units")
            .next()
            .expect("leftover_services body");
        assert!(leftover.contains("needrestart"));
        assert!(leftover.contains("\"-b\""));
        assert!(
            leftover.contains("--restart=l"),
            "needrestart observe must list only, not auto-restart"
        );
        assert!(!leftover.contains("--restart=a"));
        assert!(!leftover.contains("--restart=i"));
        let failed = src
            .split("async fn failed_units")
            .nth(1)
            .expect("failed_units")
            .split("async fn ntp_sync")
            .next()
            .expect("failed_units body");
        assert!(failed.contains("systemctl"));
        assert!(failed.contains("--failed"));
        assert!(!failed.contains("restart"));
        assert!(!failed.contains("apt-get"));
    }

    #[test]
    fn unattended_observe_does_not_enable_or_edit() {
        let src = include_str!("sys_helper.rs");
        let body = src
            .split("async fn unattended_status")
            .nth(1)
            .expect("unattended_status")
            .split("fn reboot_required")
            .next()
            .expect("unattended observe body");
        assert!(body.contains("UNATTENDED_AUTO_UPGRADES"));
        assert!(body.contains("is-enabled"));
        assert!(body.contains("unattended-upgrades"));
        assert!(!body.contains("\"enable\""));
        assert!(!body.contains("\"start\""));
        assert!(!body.contains("\"stop\""));
        assert!(!body.contains("fs::write"));
        assert!(!body.contains("unattended-upgrade\""));
    }

    #[test]
    fn reboot_is_systemctl_argv_not_shell() {
        let src = include_str!("sys_helper.rs");
        let body = src
            .split("async fn reboot")
            .nth(1)
            .expect("reboot")
            .split("async fn unit_restart")
            .next()
            .expect("reboot body");
        assert!(body.contains("systemctl"));
        assert!(body.contains("\"reboot\""));
        assert!(body.contains("cfg!(test)"));
        assert!(!body.contains("poweroff"));
        assert!(!body.contains("halt"));
        assert!(!body.contains("sh -c") && !body.contains("bash -c"));
        assert!(
            !body.contains(".arg(") || body.contains(".arg(\"reboot\")"),
            "reboot argv must be hardcoded systemctl reboot"
        );
    }

    #[test]
    fn unit_restart_is_systemctl_argv_not_shell() {
        let src = include_str!("sys_helper.rs");
        let body = src
            .split("async fn unit_restart")
            .nth(1)
            .expect("unit_restart")
            .split("async fn journal_follow")
            .next()
            .expect("unit_restart body");
        assert!(body.contains("systemctl"));
        assert!(body.contains("\"restart\""));
        assert!(body.contains("unit_listed_for_restart"));
        assert!(body.contains("parse_restart_unit"));
        assert!(body.contains("cfg!(test)"));
        assert!(body.contains("leftover_services") && body.contains("failed_units"));
        assert!(!body.contains("poweroff"));
        assert!(!body.contains("sh -c") && !body.contains("bash -c"));
    }

    #[tokio::test]
    async fn helper_rejects_shell_unit_over_socket() {
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"unit_restart","payload":{"unit":"docker.service;rm"}}"#)
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
            err.contains("unit") || err.contains("invalid"),
            "shell unit must fail validation, got {err}"
        );
        assert!(
            !err.contains("not invoked"),
            "must reject before spawn: {err}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn helper_unit_restart_bails_in_tests() {
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"unit_restart","payload":{"unit":"docker.service"}}"#)
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
            err.contains("not invoked in tests"),
            "unit restart must not spawn systemctl in CI, got {err}"
        );
        server.await.unwrap();
    }

    #[test]
    fn journal_follow_is_allowlisted_argv_not_shell() {
        let src = include_str!("sys_helper.rs");
        let body = src
            .split("async fn journal_follow")
            .nth(1)
            .expect("journal_follow")
            .split("async fn nm_connection_for_iface")
            .next()
            .expect("journal_follow body");
        assert!(body.contains("journalctl"));
        assert!(body.contains("journal_unit"));
        assert!(body.contains("kill_on_drop"));
        assert!(body.contains("\"-f\""));
        assert!(body.contains("\"200\""));
        assert!(body.contains("short-iso"));
        assert!(body.contains("cfg!(test)"));
        assert!(!body.contains("sh -c") && !body.contains("bash -c"));
        assert!(!body.contains("vacuum"));
    }

    #[tokio::test]
    async fn helper_rejects_unknown_journal_unit_over_socket() {
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"journal","payload":{"unit":"cron.service"}}"#)
            .await
            .unwrap();
        client.write_all(b"\n").await.unwrap();
        client.flush().await.unwrap();
        let mut lines = BufReader::new(client).lines();
        let line = lines.next_line().await.unwrap().expect("reply");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v.get("ok").and_then(|o| o.as_bool()), Some(false), "{line}");
        let err = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
        assert!(err.contains("unknown journal unit"), "{err}");
        assert!(
            !err.contains("journalctl"),
            "must reject before spawn: {err}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn helper_does_not_follow_journal_in_tests() {
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"journal","payload":{"unit":"ssh.service"}}"#)
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
            err.contains("not invoked in tests"),
            "allowlisted journal must not spawn journalctl -f in CI, got {err}"
        );
        server.await.unwrap();
    }

    #[test]
    fn gitlab_backup_dir_skips_noise_and_symlinks() {
        let dir = std::env::temp_dir().join(format!("ks-gl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keep_gitlab_backup.tar"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join("foo_gitlab_backup.tar;rm"), b"x").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", dir.join("link_gitlab_backup.tar")).unwrap();
        let names: Vec<String> = read_gitlab_backup_entries(&dir)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(names, vec!["keep_gitlab_backup.tar".to_string()]);
    }

    #[tokio::test]
    async fn helper_does_not_autoremove_in_tests() {
        let (path, _guard) = scratch_sock();
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            handle_conn(s).await.expect("handle");
        });
        let mut client = UnixStream::connect(&path).await.expect("connect");
        client
            .write_all(br#"{"op":"updates_autoremove","payload":{}}"#)
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
            err.contains("not invoked in tests"),
            "autoremove must not spawn apt-get in CI, got {err}"
        );
        assert!(!err.contains("apt-get"), "must bail before apt: {err}");
        server.await.unwrap();
    }

    #[test]
    fn file_mtime_unix_skips_symlinks() {
        let dir = std::env::temp_dir().join(format!("ks-ua-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let stamp = dir.join("stamp");
        std::fs::write(&stamp, b"x").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink("/etc/passwd", &link).unwrap();
        assert!(file_mtime_unix(stamp.to_str().unwrap()).is_some());
        assert!(file_mtime_unix(link.to_str().unwrap()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
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
