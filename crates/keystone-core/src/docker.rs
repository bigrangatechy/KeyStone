// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

use crate::rbac::Permission;

/// Docker operations the agent may perform. The UI, control RPC, and audit
/// log all use this enum. Keep `docs/dev/src/docker.md` in sync.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    EnumIter,
    IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum DockerOp {
    ContainerList,
    ContainerInspect,
    ContainerStart,
    ContainerStop,
    ContainerRestart,
    ContainerKill,
    ContainerRemove,
    ContainerPause,
    ContainerUnpause,
    ContainerPrune,
    ContainerLogs,
    ContainerStats,
    ContainerExec,
    ComposePs,
    ComposeUp,
    ComposeStop,
    ComposeStart,
    ComposeRestart,
    ComposeDown,
    ComposeLogs,
    ComposePull,
    ComposeUpdate,
    ImageList,
    ImageInspect,
    ImagePull,
    ImageLogin,
    ImagePrune,
    ImageRemove,
    VolumeList,
    VolumeInspect,
    VolumeCreate,
    VolumeRemove,
    VolumePrune,
    NetworkList,
    NetworkInspect,
    NetworkCreate,
    NetworkRemove,
    NetworkPrune,
}

impl DockerOp {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// Every op. Prefer this over depending on `strum` in other crates.
    pub fn all() -> impl Iterator<Item = Self> {
        use strum::IntoEnumIterator;
        Self::iter()
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ContainerList => "List containers",
            Self::ContainerInspect => "Inspect a container",
            Self::ContainerStart => "Start a container",
            Self::ContainerStop => "Stop a container",
            Self::ContainerRestart => "Restart a container",
            Self::ContainerKill => "Kill a container",
            Self::ContainerRemove => "Remove a container",
            Self::ContainerPause => "Pause a container",
            Self::ContainerUnpause => "Unpause a container",
            Self::ContainerPrune => "Prune stopped containers",
            Self::ContainerLogs => "Stream container logs (on-demand)",
            Self::ContainerStats => "Stream live container stats (on-demand)",
            Self::ContainerExec => {
                "Exec a command in a container (disabled unless docker.allow_exec)"
            }
            Self::ComposePs => "List Compose project services",
            Self::ComposeUp => "Compose up",
            Self::ComposeStop => "Compose stop",
            Self::ComposeStart => "Compose start",
            Self::ComposeRestart => "Compose restart",
            Self::ComposeDown => "Compose down",
            Self::ComposeLogs => "Compose logs",
            Self::ComposePull => "Compose pull",
            Self::ComposeUpdate => "Compose pull then up",
            Self::ImageList => "List images",
            Self::ImageInspect => "Inspect an image",
            Self::ImagePull => "Pull an image",
            Self::ImageLogin => "Log in to Docker Hub or GHCR on this node",
            Self::ImagePrune => "Prune unused images",
            Self::ImageRemove => "Remove an image",
            Self::VolumeList => "List volumes",
            Self::VolumeInspect => "Inspect a volume",
            Self::VolumeCreate => "Create a volume",
            Self::VolumeRemove => "Remove a volume",
            Self::VolumePrune => "Prune unused volumes",
            Self::NetworkList => "List networks",
            Self::NetworkInspect => "Inspect a network",
            Self::NetworkCreate => "Create a network",
            Self::NetworkRemove => "Remove a network",
            Self::NetworkPrune => "Prune unused networks",
        }
    }

    pub fn mutating(self) -> bool {
        matches!(
            self,
            Self::ContainerStart
                | Self::ContainerStop
                | Self::ContainerRestart
                | Self::ContainerKill
                | Self::ContainerRemove
                | Self::ContainerPause
                | Self::ContainerUnpause
                | Self::ContainerPrune
                | Self::ContainerExec
                | Self::ComposeUp
                | Self::ComposeStop
                | Self::ComposeStart
                | Self::ComposeRestart
                | Self::ComposeDown
                | Self::ComposePull
                | Self::ComposeUpdate
                | Self::ImagePull
                | Self::ImageLogin
                | Self::ImagePrune
                | Self::ImageRemove
                | Self::VolumeCreate
                | Self::VolumeRemove
                | Self::VolumePrune
                | Self::NetworkCreate
                | Self::NetworkRemove
                | Self::NetworkPrune
        )
    }

    pub fn permission(self) -> Permission {
        match self {
            Self::ContainerExec => Permission::DockerExec,
            op if op.mutating() => Permission::DockerManage,
            _ => Permission::DockerView,
        }
    }

    /// Agent sends `StreamChunk`s then a `CommandResult` (logs).
    pub fn streams(self) -> bool {
        matches!(self, Self::ContainerLogs | Self::ComposeLogs)
    }

    /// Fresh authenticator code when TOTP is on. No Docker op uses this
    /// yet; keep-outs we promote later opt in here. IPv4 is a `SysOp`.
    pub fn needs_step_up(self) -> bool {
        let _ = self;
        false
    }
}

/// CPU/memory from pushed container series, keyed by short container `id`.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct ContainerUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<f64>,
}

pub fn container_usage_by_id(
    samples: &[crate::Sample],
) -> std::collections::BTreeMap<String, ContainerUsage> {
    let mut out = std::collections::BTreeMap::<String, ContainerUsage>::new();
    for s in samples {
        let Some(id) = s
            .labels
            .iter()
            .find(|l| l.name == "id")
            .map(|l| l.value.as_str())
        else {
            continue;
        };
        let row = out.entry(id.to_string()).or_default();
        match s.metric.as_str() {
            "container_cpu_usage_ratio" => row.cpu_ratio = Some(s.value),
            "container_memory_usage_bytes" => row.memory_bytes = Some(s.value),
            _ => {}
        }
    }
    out.retain(|_, u| u.cpu_ratio.is_some() || u.memory_bytes.is_some());
    out
}

/// Join background container series onto a `container_list` row by short `id`.
pub fn merge_container_usage(list: &mut [serde_json::Value], samples: &[crate::Sample]) {
    let usage = container_usage_by_id(samples);
    for row in list {
        let Some(id) = row.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
            continue;
        };
        if let Some(u) = usage.get(&id) {
            if let Some(v) = u.cpu_ratio {
                row["cpu_ratio"] = serde_json::json!(v);
            }
            if let Some(v) = u.memory_bytes {
                row["memory_bytes"] = serde_json::json!(v);
            }
        }
    }
}

fn json_field<'a>(v: &'a serde_json::Value, names: &[&str]) -> Option<&'a serde_json::Value> {
    let obj = v.as_object()?;
    for name in names {
        if let Some(x) = obj.get(*name) {
            if !x.is_null() {
                return Some(x);
            }
        }
    }
    None
}

fn json_str(v: &serde_json::Value, names: &[&str]) -> Option<String> {
    json_field(v, names).and_then(|x| match x {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

fn json_bool(v: &serde_json::Value, names: &[&str]) -> Option<bool> {
    json_field(v, names).and_then(|x| x.as_bool())
}

/// Log in to Docker Hub or GHCR. Password is argv-stdin only and must not
/// be audited. Credentials stay on the node (`docker login`), never in the
/// server SQLite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageLogin {
    pub registry: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockerLoginError {
    Registry,
    Username,
    Password,
}

impl std::fmt::Display for DockerLoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Registry => write!(f, "registry must be docker.io or ghcr.io"),
            Self::Username => write!(f, "username is invalid"),
            Self::Password => write!(f, "password is invalid"),
        }
    }
}

impl ImageLogin {
    pub fn parse_json(raw: &str) -> Result<Self, DockerLoginError> {
        let v: Self = serde_json::from_str(raw).map_err(|_| DockerLoginError::Registry)?;
        v.validate()
    }

    pub fn validate(mut self) -> Result<Self, DockerLoginError> {
        self.registry = validate_login_registry(&self.registry)?;
        self.username = validate_login_username(&self.username)?;
        self.password = validate_login_password(&self.password)?;
        Ok(self)
    }
}

/// Listed registries only. Not a hostname textbox.
pub fn validate_login_registry(raw: &str) -> Result<String, DockerLoginError> {
    match raw.trim() {
        "docker.io" => Ok("docker.io".into()),
        "ghcr.io" => Ok("ghcr.io".into()),
        _ => Err(DockerLoginError::Registry),
    }
}

pub fn validate_login_username(raw: &str) -> Result<String, DockerLoginError> {
    let t = raw.trim();
    if t.is_empty() || t.len() > 64 {
        return Err(DockerLoginError::Username);
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@'))
    {
        return Err(DockerLoginError::Username);
    }
    Ok(t.to_string())
}

pub fn validate_login_password(raw: &str) -> Result<String, DockerLoginError> {
    let t = raw.trim();
    if t.is_empty() || t.len() > 512 || t.contains('\0') || t.contains('\n') || t.contains('\r') {
        return Err(DockerLoginError::Password);
    }
    Ok(t.to_string())
}

/// `docker login --username … --password-stdin <registry>`. Password is stdin.
pub fn docker_login_args(req: &ImageLogin) -> Result<Vec<String>, DockerLoginError> {
    let req = req.clone().validate()?;
    Ok(vec![
        "login".into(),
        "--username".into(),
        req.username,
        "--password-stdin".into(),
        req.registry,
    ])
}

/// Drop `password` from a Docker mutation payload before Audit.
pub fn audit_docker_target(op: DockerOp, payload: &str) -> String {
    if op != DockerOp::ImageLogin {
        return payload.to_string();
    }
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(payload) else {
        return "{\"password\":\"\"}".into();
    };
    if let Some(obj) = v.as_object_mut() {
        if obj.contains_key("password") {
            obj.insert("password".into(), serde_json::json!(""));
        }
    }
    v.to_string()
}

/// Hub vs GHCR (and other hosts) for a pull name. `nginx:1.27` is Hub.
pub fn registry_host_for_image(name: &str) -> String {
    let name = name.trim().split('@').next().unwrap_or(name).trim();
    let first = name.split('/').next().unwrap_or("");
    if first_component_is_registry(first) {
        let host = first
            .rsplit_once(':')
            .and_then(|(h, p)| p.chars().all(|c| c.is_ascii_digit()).then_some(h))
            .unwrap_or(first)
            .to_ascii_lowercase();
        if host == "docker.io" || host == "index.docker.io" || host == "registry-1.docker.io" {
            return "docker.io".into();
        }
        if host == "ghcr.io" {
            return "ghcr.io".into();
        }
        return host;
    }
    "docker.io".into()
}

fn first_component_is_registry(first: &str) -> bool {
    if first == "localhost" {
        return true;
    }
    if let Some((host, port)) = first.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            return host == "localhost" || host.contains('.');
        }
        return false;
    }
    first.contains('.')
}

/// Decode `auths.<registry>.auth` (`base64(user:pass)`) from a Docker config.json.
pub fn docker_config_auth(config_json: &str, registry: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(config_json).ok()?;
    let auths = v.get("auths")?.as_object()?;
    let keys: &[&str] = match registry {
        "docker.io" => &[
            "https://index.docker.io/v1/",
            "docker.io",
            "https://registry-1.docker.io/v2/",
            "https://index.docker.io/v1",
        ],
        "ghcr.io" => &["ghcr.io", "https://ghcr.io", "https://ghcr.io/v2/"],
        other => return lookup_auth(auths, other),
    };
    for k in keys {
        if let Some(pair) = lookup_auth(auths, k) {
            return Some(pair);
        }
    }
    None
}

fn lookup_auth(
    auths: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<(String, String)> {
    let entry = auths.get(key)?;
    let b64 = entry.get("auth")?.as_str()?;
    let raw = decode_std_base64(b64)?;
    let text = String::from_utf8(raw).ok()?;
    let (user, pass) = text.split_once(':')?;
    if user.is_empty() || pass.is_empty() {
        return None;
    }
    Some((user.to_string(), pass.to_string()))
}

fn decode_std_base64(input: &str) -> Option<Vec<u8>> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [255u8; 256];
    for (i, &c) in T.iter().enumerate() {
        table[c as usize] = i as u8;
    }
    let bytes: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        let mut pads = 0;
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'=' {
                pads += 1;
                continue;
            }
            if pads > 0 {
                return None;
            }
            let v = table[c as usize];
            if v == 255 {
                return None;
            }
            n |= u32::from(v) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if pads < 2 {
            out.push((n >> 8) as u8);
        }
        if pads < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// Hex / name token the UI may put in a container inspect URL.
pub fn docker_ref_ok(id: &str) -> bool {
    let t = id.trim();
    !t.is_empty()
        && t.len() <= 128
        && !t.contains("..")
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// Map Engine inspect JSON to what the Containers detail pane may show.
/// Drops `Env` and other secret-shaped fields.
pub fn summarize_container_inspect(raw: &serde_json::Value) -> serde_json::Value {
    let config = json_field(raw, &["Config", "config"])
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let host = json_field(raw, &["HostConfig", "host_config"])
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let state = json_field(raw, &["State", "state"])
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let nets = json_field(raw, &["NetworkSettings", "network_settings"])
        .and_then(|n| json_field(n, &["Networks", "networks"]))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let labels = json_field(&config, &["Labels", "labels"])
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let name = json_str(raw, &["Name", "name"]).map(|n| n.trim_start_matches('/').to_string());
    let image =
        json_str(&config, &["Image", "image"]).or_else(|| json_str(raw, &["Image", "image"]));
    let mut command: Vec<String> = Vec::new();
    if let Some(cmd) = json_field(&config, &["Cmd", "cmd"]).and_then(|c| c.as_array()) {
        command = cmd
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
    }
    if command.is_empty() {
        if let Some(path) = json_str(raw, &["Path", "path"]) {
            command.push(path);
        }
        if let Some(args) = json_field(raw, &["Args", "args"]).and_then(|a| a.as_array()) {
            command.extend(args.iter().filter_map(|x| x.as_str().map(str::to_string)));
        }
    }

    let restart = json_field(&host, &["RestartPolicy", "restart_policy"])
        .and_then(|p| json_str(p, &["Name", "name"]))
        .filter(|s| !s.is_empty() && s != "no");

    let mut mounts = Vec::new();
    if let Some(arr) = json_field(raw, &["Mounts", "mounts"]).and_then(|m| m.as_array()) {
        for m in arr {
            let dest = json_str(m, &["Destination", "destination"]).unwrap_or_default();
            if dest.is_empty() {
                continue;
            }
            mounts.push(serde_json::json!({
                "type": json_str(m, &["Type", "type"]).unwrap_or_else(|| "bind".into()),
                "source": json_str(m, &["Source", "source"]).unwrap_or_default(),
                "destination": dest,
                "rw": json_bool(m, &["RW", "rw"]).unwrap_or(true),
            }));
        }
    }

    let mut networks = Vec::new();
    if let Some(obj) = nets.as_object() {
        for (net_name, n) in obj {
            let ip = json_str(n, &["IPAddress", "ip_address"]).unwrap_or_default();
            networks.push(serde_json::json!({
                "name": net_name,
                "ip": ip,
            }));
        }
    }

    let mut out = serde_json::Map::new();
    if let Some(id) = json_str(raw, &["Id", "id"]) {
        out.insert("id".into(), serde_json::json!(id));
    }
    if let Some(n) = name {
        out.insert("name".into(), serde_json::json!(n));
    }
    if let Some(img) = image {
        out.insert("image".into(), serde_json::json!(img));
    }
    if let Some(created) = json_str(raw, &["Created", "created"]) {
        out.insert("created".into(), serde_json::json!(created));
    }
    if let Some(st) = json_str(&state, &["Status", "status"]) {
        out.insert("status".into(), serde_json::json!(st));
    }
    if let Some(pid) = json_field(&state, &["Pid", "pid"]).and_then(|p| p.as_i64()) {
        out.insert("pid".into(), serde_json::json!(pid));
    }
    if let Some(started) = json_str(&state, &["StartedAt", "started_at"]) {
        out.insert("started_at".into(), serde_json::json!(started));
    }
    if let Some(err) = json_str(&state, &["Error", "error"]) {
        if !err.is_empty() {
            out.insert("error".into(), serde_json::json!(err));
        }
    }
    if !command.is_empty() {
        out.insert("command".into(), serde_json::json!(command));
    }
    if let Some(r) = restart {
        out.insert("restart".into(), serde_json::json!(r));
    }
    if json_bool(&host, &["Privileged", "privileged"]) == Some(true) {
        out.insert("privileged".into(), serde_json::json!(true));
    }
    if let Some(mode) = json_str(&host, &["NetworkMode", "network_mode"]) {
        out.insert("network_mode".into(), serde_json::json!(mode));
    }
    if let Some(project) = json_str(&labels, &["com.docker.compose.project"]) {
        out.insert("compose_project".into(), serde_json::json!(project));
    }
    if let Some(svc) = json_str(&labels, &["com.docker.compose.service"]) {
        out.insert("compose_service".into(), serde_json::json!(svc));
    }
    if !mounts.is_empty() {
        out.insert("mounts".into(), serde_json::json!(mounts));
    }
    if !networks.is_empty() {
        out.insert("networks".into(), serde_json::json!(networks));
    }
    serde_json::Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_update_is_mutating_pull_then_up() {
        assert_eq!(DockerOp::ComposeUpdate.as_str(), "compose_update");
        assert!(DockerOp::ComposeUpdate.mutating());
        assert_eq!(
            DockerOp::ComposeUpdate.permission(),
            Permission::DockerManage
        );
        assert!(!DockerOp::ComposeUpdate.streams());
        assert_eq!(
            DockerOp::ComposeUpdate.description(),
            "Compose pull then up"
        );
    }

    #[test]
    fn merge_container_usage_matches_short_id_only() {
        let samples = vec![
            crate::Sample::new("container_cpu_usage_ratio", 0.25, 1)
                .with_label("id", "abc123def456")
                .with_label("name", "web"),
            crate::Sample::new("container_memory_usage_bytes", 64.0 * 1024.0 * 1024.0, 1)
                .with_label("id", "abc123def456")
                .with_label("name", "web"),
            crate::Sample::new("node_cpu_usage_ratio", 0.9, 1),
        ];
        let mut list = vec![
            serde_json::json!({"id": "abc123def456", "names": ["/web"]}),
            serde_json::json!({"id": "other0000000", "names": ["/db"]}),
        ];
        merge_container_usage(&mut list, &samples);
        assert_eq!(list[0]["cpu_ratio"], 0.25);
        assert_eq!(list[0]["memory_bytes"], 64.0 * 1024.0 * 1024.0);
        assert!(list[1].get("cpu_ratio").is_none());
        assert!(list[1].get("memory_bytes").is_none());
        let map = container_usage_by_id(&samples);
        assert_eq!(
            map.get("abc123def456").and_then(|u| u.cpu_ratio),
            Some(0.25)
        );
        assert!(!map.contains_key("other0000000"));
    }

    #[test]
    fn image_login_is_listed_registry_and_redacts_password() {
        assert_eq!(validate_login_registry("docker.io"), Ok("docker.io".into()));
        assert_eq!(validate_login_registry("ghcr.io"), Ok("ghcr.io".into()));
        assert_eq!(
            validate_login_registry("ghcr.io;rm"),
            Err(DockerLoginError::Registry)
        );
        assert_eq!(
            validate_login_registry("harbor.example"),
            Err(DockerLoginError::Registry)
        );
        assert_eq!(validate_login_username("alice"), Ok("alice".into()));
        assert_eq!(
            validate_login_username("alice;rm"),
            Err(DockerLoginError::Username)
        );
        assert_eq!(
            validate_login_password("ghp_notarealtoken"),
            Ok("ghp_notarealtoken".into())
        );
        assert_eq!(
            validate_login_password("has\nnewline"),
            Err(DockerLoginError::Password)
        );
        let req = ImageLogin {
            registry: " ghcr.io ".into(),
            username: " alice ".into(),
            password: "ghp_notarealtoken".into(),
        }
        .validate()
        .unwrap();
        let args = docker_login_args(&req).unwrap();
        assert_eq!(
            args,
            vec![
                "login",
                "--username",
                "alice",
                "--password-stdin",
                "ghcr.io"
            ]
        );
        assert!(!args.iter().any(|a| a.contains("ghp_")));
        assert!(!args.iter().any(|a| a.contains("sh -c")));
        let raw = r#"{"registry":"ghcr.io","username":"alice","password":"ghp_notarealtoken"}"#;
        let redacted = audit_docker_target(DockerOp::ImageLogin, raw);
        assert!(redacted.contains("alice"));
        assert!(redacted.contains("ghcr.io"));
        assert!(!redacted.contains("ghp_notarealtoken"));
        assert_eq!(
            audit_docker_target(DockerOp::ImagePull, r#"{"name":"nginx"}"#),
            r#"{"name":"nginx"}"#
        );
        assert_eq!(registry_host_for_image("nginx:1.27"), "docker.io");
        assert_eq!(registry_host_for_image("library/nginx"), "docker.io");
        assert_eq!(registry_host_for_image("ghcr.io/org/app:main"), "ghcr.io");
        assert_eq!(registry_host_for_image("localhost:5000/app:1"), "localhost");
        assert_eq!(
            docker_config_auth(
                r#"{"auths":{"ghcr.io":{"auth":"YWxpY2U6Z2hwX25vdGFyZWFsdG9rZW4="}}}"#,
                "ghcr.io"
            ),
            Some(("alice".into(), "ghp_notarealtoken".into()))
        );
        assert_eq!(
            docker_config_auth(
                r#"{"auths":{"https://index.docker.io/v1/":{"auth":"YWxpY2U6c2VjcmV0"}}}"#,
                "docker.io"
            ),
            Some(("alice".into(), "secret".into()))
        );
        assert!(docker_config_auth("{}", "ghcr.io").is_none());
    }

    #[test]
    fn summarize_container_inspect_drops_env_and_keeps_mounts() {
        assert!(docker_ref_ok("abc123def456"));
        assert!(docker_ref_ok("gitlab"));
        assert!(!docker_ref_ok(""));
        assert!(!docker_ref_ok("id;rm"));
        assert!(!docker_ref_ok("../etc"));
        let raw = serde_json::json!({
            "Id": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "Name": "/gitlab",
            "Created": "2026-01-01T00:00:00Z",
            "Config": {
                "Image": "gitlab/gitlab-ce:latest",
                "Cmd": ["gitlab"],
                "Env": ["SECRET=hunter2", "PATH=/usr/bin"],
                "Labels": {
                    "com.docker.compose.project": "gitlab",
                    "com.docker.compose.service": "web"
                }
            },
            "HostConfig": {
                "RestartPolicy": { "Name": "unless-stopped" },
                "Privileged": false,
                "NetworkMode": "bridge"
            },
            "State": { "Status": "running", "Pid": 42, "Error": "" },
            "Mounts": [{
                "Type": "bind",
                "Source": "/opt/gitlab",
                "Destination": "/var/opt/gitlab",
                "RW": true
            }],
            "NetworkSettings": {
                "Networks": { "bridge": { "IPAddress": "172.17.0.2" } }
            }
        });
        let out = summarize_container_inspect(&raw);
        let dumped = out.to_string();
        assert!(
            !dumped.contains("hunter2"),
            "Env must never reach the UI JSON"
        );
        assert!(!dumped.contains("Env"), "{dumped}");
        assert_eq!(out["name"], "gitlab");
        assert_eq!(out["image"], "gitlab/gitlab-ce:latest");
        assert_eq!(out["compose_project"], "gitlab");
        assert_eq!(out["compose_service"], "web");
        assert_eq!(out["restart"], "unless-stopped");
        assert_eq!(out["networks"][0]["ip"], "172.17.0.2");
        assert_eq!(out["mounts"][0]["destination"], "/var/opt/gitlab");
        assert!(out.get("privileged").is_none());
    }

    #[test]
    fn mutating_ops_are_in_the_ui_except_reserved_exec() {
        use strum::IntoEnumIterator;
        let js = include_str!("../../keystone-server/src/static/app.js");
        let html = include_str!("../../keystone-server/templates/node.html");
        for op in DockerOp::iter() {
            if !op.mutating() {
                continue;
            }
            let name = op.as_str();
            if op == DockerOp::ContainerExec {
                assert!(
                    !js.contains(name) && !html.contains(name),
                    "container_exec must stay out of the UI"
                );
                continue;
            }
            assert!(
                js.contains(name) || html.contains(&format!("docker/{name}")),
                "mutating {name} must be reachable from the Docker UI"
            );
        }
    }

    #[test]
    fn new_manage_ops_are_mutations_not_streams() {
        for op in [
            DockerOp::ContainerPause,
            DockerOp::ContainerUnpause,
            DockerOp::ContainerPrune,
            DockerOp::ComposeStop,
            DockerOp::ComposeStart,
            DockerOp::ComposeRestart,
            DockerOp::VolumePrune,
            DockerOp::NetworkPrune,
        ] {
            assert!(op.mutating(), "{} must audit", op.as_str());
            assert!(!op.streams(), "{} is not a log stream", op.as_str());
            assert_eq!(op.permission(), Permission::DockerManage);
        }
        assert!(!DockerOp::ComposePs.mutating());
        assert_eq!(DockerOp::ComposeStop.as_str(), "compose_stop");
        assert_eq!(DockerOp::ContainerPause.as_str(), "container_pause");
        assert_eq!(DockerOp::ImageLogin.as_str(), "image_login");
        assert!(DockerOp::ImageLogin.mutating());
        assert!(!DockerOp::ImageLogin.streams());
        assert_eq!(DockerOp::ImageLogin.permission(), Permission::DockerManage);
        assert!(!DockerOp::ImageLogin.needs_step_up());
    }

    #[test]
    fn no_docker_op_needs_step_up_yet() {
        for op in DockerOp::all() {
            assert!(
                !op.needs_step_up(),
                "{} is confirm-only until a keep-out is promoted",
                op.as_str()
            );
        }
    }
}
