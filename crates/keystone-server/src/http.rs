// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;

use askama::Template;
use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use keystone_core::docker::DockerOp;
use keystone_core::metrics::catalog;
use keystone_core::rbac::Permission;
use keystone_core::widgets::{hydrate, presets_for_samples, Dashboard, WidgetKind};
use keystone_core::{NodeSettings, ServerSettings};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth;
use crate::help;
use crate::state::AppState;

const SESSION_COOKIE: &str = "keystone_session";

pub fn router(state: AppState) -> Router {
    let authed = Router::new()
        .route("/", get(nodes_page))
        .route("/nodes", get(nodes_page).post(add_node_post))
        .route("/nodes/new", get(add_node_page))
        .route("/nodes/{id}", get(node_page))
        .route("/nodes/{id}/setup", get(node_setup_page))
        .route("/nodes/{id}/settings", post(node_settings_post))
        .route("/settings", get(settings_page).post(settings_post))
        .route("/settings/rotate-token", post(settings_rotate_token))
        .route("/nodes/{id}/docker/{op}", post(docker_action))
        .route("/nodes/{id}/containers/{cid}/logs", get(container_logs_sse))
        .route(
            "/nodes/{id}/containers/{cid}/stats",
            get(container_stats_sse),
        )
        .route("/help", get(help_index))
        .route("/help/{slug}", get(help_section))
        .route("/api/v1/catalog", get(catalog_api))
        .route(
            "/api/v1/nodes/{id}/dashboard",
            get(dashboard_get)
                .put(dashboard_put)
                .delete(dashboard_delete),
        )
        .route("/logout", post(logout))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/login", get(login_page).post(login_post))
        .route("/static/app.css", get(css))
        .route("/static/app.js", get(js))
        .merge(authed)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("static/app.css"),
    )
}

async fn js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("static/app.js"),
    )
}

async fn require_session(
    State(state): State<AppState>,
    jar: CookieJar,
    request: Request<Body>,
    next: Next,
) -> Response {
    if request.uri().path().starts_with("/static") || request.uri().path() == "/health" {
        return next.run(request).await;
    }
    let Some(cookie) = jar.get(SESSION_COOKIE) else {
        return Redirect::to("/login").into_response();
    };
    match state.stores.metadata.get_session(cookie.value()) {
        Ok(Some(_)) => next.run(request).await,
        _ => Redirect::to("/login").into_response(),
    }
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: String,
}

async fn login_page() -> impl IntoResponse {
    Html(
        LoginTemplate {
            error: String::new(),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

async fn login_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Response {
    let expected_user = &state.config.auth.username;
    let ok_user = form.username == *expected_user;
    let hash = state
        .stores
        .metadata
        .user_hash(&form.username)
        .ok()
        .flatten();
    let ok_pass = hash
        .as_deref()
        .map(|h| auth::verify_password(&form.password, h))
        .unwrap_or(false);
    if !ok_user || !ok_pass {
        return (
            StatusCode::UNAUTHORIZED,
            Html(
                LoginTemplate {
                    error: "Invalid username or password".into(),
                }
                .render()
                .unwrap_or_default(),
            ),
        )
            .into_response();
    }
    let sid = auth::new_session_id();
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 86400 * 7;
    let _ = state
        .stores
        .metadata
        .put_session(&sid, &form.username, expires);
    let mut cookie = Cookie::new(SESSION_COOKIE, sid);
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    (jar.add(cookie), Redirect::to("/")).into_response()
}

async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        let _ = state.stores.metadata.delete_session(c.value());
    }
    let mut cookie = Cookie::from(SESSION_COOKIE);
    cookie.set_path("/");
    (jar.remove(cookie), Redirect::to("/login")).into_response()
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate {
    retention_hours: u32,
    ingest_token: String,
    ingest_token_env_override: bool,
    prometheus_scrape: String,
    snmp_scrape: String,
    username: String,
    saved: bool,
    rotated: bool,
    error: String,
}

#[derive(Deserialize)]
struct SettingsQuery {
    saved: Option<String>,
    rotated: Option<String>,
}

fn settings_view(
    state: &AppState,
    stored: &ServerSettings,
    saved: bool,
    rotated: bool,
    error: String,
    prometheus_scrape: Option<String>,
    snmp_scrape: Option<String>,
) -> SettingsTemplate {
    SettingsTemplate {
        retention_hours: ServerSettings::clamp_retention_hours(stored.retention_hours),
        ingest_token: if state.ingest_token_env_override() {
            state.ingest_token()
        } else {
            stored.ingest_token.clone()
        },
        ingest_token_env_override: state.ingest_token_env_override(),
        prometheus_scrape: prometheus_scrape
            .unwrap_or_else(|| ServerSettings::format_prometheus_lines(&stored.prometheus_scrape)),
        snmp_scrape: snmp_scrape
            .unwrap_or_else(|| ServerSettings::format_snmp_lines(&stored.snmp_scrape)),
        username: state.config.auth.username.clone(),
        saved,
        rotated,
        error,
    }
}

async fn settings_page(
    State(state): State<AppState>,
    Query(q): Query<SettingsQuery>,
) -> impl IntoResponse {
    let stored = state.stored_server_settings();
    Html(
        settings_view(
            &state,
            &stored,
            q.saved.as_deref() == Some("1"),
            q.rotated.as_deref() == Some("1"),
            String::new(),
            None,
            None,
        )
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

#[derive(Deserialize)]
struct ServerSettingsForm {
    #[serde(default)]
    retention_hours: Option<u32>,
    #[serde(default)]
    ingest_token: String,
    #[serde(default)]
    prometheus_scrape: String,
    #[serde(default)]
    snmp_scrape: String,
    #[serde(default)]
    new_password: String,
    #[serde(default)]
    new_password_confirm: String,
}

async fn settings_post(
    State(state): State<AppState>,
    Form(form): Form<ServerSettingsForm>,
) -> Response {
    let fail = |state: &AppState, error: String, form: &ServerSettingsForm| {
        let mut stored = state.stored_server_settings();
        stored.retention_hours = form.retention_hours.unwrap_or(stored.retention_hours);
        stored.ingest_token = form.ingest_token.clone();
        Html(
            settings_view(
                state,
                &stored,
                false,
                false,
                error,
                Some(form.prometheus_scrape.clone()),
                Some(form.snmp_scrape.clone()),
            )
            .render()
            .unwrap_or_else(|e| e.to_string()),
        )
        .into_response()
    };
    let prom = match ServerSettings::parse_prometheus_lines(&form.prometheus_scrape) {
        Ok(j) => j,
        Err(error) => return fail(&state, error, &form),
    };
    let snmp = match ServerSettings::parse_snmp_lines(&form.snmp_scrape) {
        Ok(j) => j,
        Err(error) => return fail(&state, error, &form),
    };
    let new_password = form.new_password.trim();
    let confirm = form.new_password_confirm.trim();
    if new_password != confirm {
        return fail(
            &state,
            "new password and confirmation do not match".into(),
            &form,
        );
    }
    if !new_password.is_empty() {
        match auth::hash_password(new_password) {
            Ok(hash) => {
                if let Err(e) = state
                    .stores
                    .metadata
                    .upsert_user(&state.config.auth.username, &hash)
                {
                    return fail(&state, format!("could not update password: {e}"), &form);
                }
            }
            Err(e) => return fail(&state, format!("could not hash password: {e}"), &form),
        }
    }
    let mut next = state.stored_server_settings();
    next.retention_hours =
        ServerSettings::clamp_retention_hours(form.retention_hours.unwrap_or(next.retention_hours));
    if !state.ingest_token_env_override() {
        next.ingest_token = form.ingest_token.trim().to_string();
    }
    next.prometheus_scrape = prom;
    next.snmp_scrape = snmp;
    if let Err(e) = state.save_server_settings(&next) {
        return fail(&state, format!("could not save: {e}"), &form);
    }
    Redirect::to("/settings?saved=1").into_response()
}

async fn settings_rotate_token(State(state): State<AppState>) -> Response {
    if state.ingest_token_env_override() {
        return Redirect::to("/settings").into_response();
    }
    let mut next = state.stored_server_settings();
    next.ingest_token = auth::generate_ingest_token();
    let _ = state.save_server_settings(&next);
    Redirect::to("/settings?rotated=1").into_response()
}

#[derive(Template)]
#[template(path = "nodes.html")]
struct NodesTemplate {
    nodes: Vec<NodeRow>,
}

struct NodeRow {
    node_id: String,
    hostname: String,
    os: String,
    agent_version: String,
    docker_version: String,
    last_seen: String,
    status: String,
}

async fn nodes_page(State(state): State<AppState>) -> impl IntoResponse {
    let nodes = state.stores.metadata.list_nodes().unwrap_or_default();
    let rows = nodes
        .into_iter()
        .map(|n| {
            let status = if state.agents.is_connected(&n.node_id) {
                "connected".into()
            } else if n.awaiting_agent() {
                "awaiting agent".into()
            } else if n.online {
                "seen".into()
            } else {
                "offline".into()
            };
            let last_seen = if n.awaiting_agent() {
                "never".into()
            } else {
                n.last_seen().to_rfc3339()
            };
            NodeRow {
                node_id: n.node_id.clone(),
                hostname: {
                    let settings = NodeSettings::parse_or_default(
                        state
                            .stores
                            .metadata
                            .node_settings_json(&n.node_id)
                            .ok()
                            .flatten()
                            .as_deref(),
                    );
                    settings.display_host(&n.hostname).to_string()
                },
                os: if n.os == "awaiting-agent" {
                    String::new()
                } else {
                    n.os
                },
                agent_version: n.agent_version,
                docker_version: n.docker_version.unwrap_or_default(),
                last_seen,
                status,
            }
        })
        .collect();
    Html(
        NodesTemplate { nodes: rows }
            .render()
            .unwrap_or_else(|e| e.to_string()),
    )
}

#[derive(Template)]
#[template(path = "node_new.html")]
struct NodeNewTemplate {
    ingest_url: String,
    error: String,
}

fn host_without_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return match rest.find(']') {
            Some(end) => &host[..=end + 1],
            None => host,
        };
    }
    match host.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => host,
    }
}

fn suggested_ingest_url(headers: &HeaderMap, grpc_listen: &str) -> String {
    let grpc_port = grpc_listen.rsplit(':').next().unwrap_or("9100");
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1");
    format!("http://{}:{grpc_port}", host_without_port(host))
}

fn slug_node_id(hostname: &str, node_id: &str) -> Result<String, String> {
    let raw = if node_id.trim().is_empty() {
        hostname
    } else {
        node_id
    };
    let s: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        Err("hostname is required".into())
    } else {
        Ok(s)
    }
}

async fn add_node_page(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    Html(
        NodeNewTemplate {
            ingest_url: suggested_ingest_url(&headers, &state.config.grpc_listen),
            error: String::new(),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

#[derive(Deserialize)]
struct AddNodeForm {
    hostname: String,
    node_id: Option<String>,
    ingest_url: Option<String>,
    #[serde(default)]
    docker: String,
}

async fn add_node_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<AddNodeForm>,
) -> Response {
    let hostname = form.hostname.trim().to_string();
    let id_src = form.node_id.unwrap_or_default();
    match slug_node_id(&hostname, &id_src) {
        Ok(node_id) => {
            if let Err(e) = state
                .stores
                .metadata
                .register_node(&node_id, &hostname, "[]")
            {
                return Html(
                    NodeNewTemplate {
                        ingest_url: suggested_ingest_url(&headers, &state.config.grpc_listen),
                        error: format!("could not register node: {e}"),
                    }
                    .render()
                    .unwrap_or_default(),
                )
                .into_response();
            }
            let docker = form.docker == "on" || form.docker == "true" || form.docker == "1";
            if docker {
                let settings = NodeSettings {
                    docker_enabled: true,
                    docker_manage: true,
                    ..Default::default()
                };
                let encoded = serde_json::to_string(&settings).unwrap_or_else(|_| "{}".into());
                let _ = state
                    .stores
                    .metadata
                    .set_node_settings_json(&node_id, Some(&encoded));
            }
            let ingest = form
                .ingest_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| suggested_ingest_url(&headers, &state.config.grpc_listen));
            let qs = format!(
                "/nodes/{node_id}/setup?ingest_url={}&docker={}",
                urlencoding_lite(&ingest),
                docker
            );
            Redirect::to(&qs).into_response()
        }
        Err(error) => Html(
            NodeNewTemplate {
                ingest_url: suggested_ingest_url(&headers, &state.config.grpc_listen),
                error,
            }
            .render()
            .unwrap_or_default(),
        )
        .into_response(),
    }
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Template)]
#[template(path = "node_setup.html")]
struct NodeSetupTemplate {
    node_id: String,
    hostname: String,
    ingest_url: String,
    agent_toml: String,
    docker: bool,
    awaiting: bool,
}

#[derive(Deserialize)]
struct SetupQuery {
    ingest_url: Option<String>,
    docker: Option<String>,
}

async fn node_setup_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<SetupQuery>,
) -> Response {
    let Some(node) = state.stores.metadata.get_node(&id).ok().flatten() else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    let ingest_url = q
        .ingest_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| suggested_ingest_url(&headers, &state.config.grpc_listen));
    let settings = node_settings(&state, &id);
    let docker = q.docker.as_deref() == Some("true") || settings.docker_enabled;
    let token = state.ingest_token();
    let awaiting = node.awaiting_agent();
    let agent_toml = format!(
        "ingest_url = \"{ingest_url}\"\ningest_token = \"{token}\"\nnode_id = \"{}\"\nbuffer_dir = \"/var/lib/keystone/agent-buffer\"\n",
        node.node_id,
    );
    Html(
        NodeSetupTemplate {
            node_id: node.node_id,
            hostname: node.hostname,
            ingest_url,
            agent_toml,
            docker,
            awaiting,
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
}

#[derive(Template)]
#[template(path = "node.html")]
struct NodeTemplate {
    node_id: String,
    hostname: String,
    os: String,
    kernel: String,
    agent_version: String,
    docker_version: String,
    last_seen: String,
    online: bool,
    connected: bool,
    awaiting: bool,
    metrics: Vec<MetricRow>,
    containers_json: String,
    compose_json: String,
    images_json: String,
    volumes_json: String,
    networks_json: String,
    docker_error: String,
    widgets_json: String,
    layout_json: String,
    presets_json: String,
    dash_source: String,
    display_name: String,
    notes: String,
    network_devices_text: String,
    detected_nics: String,
    poll_secs: u32,
    labels_text: String,
    docker_enabled: bool,
    docker_manage: bool,
    docker_allow_exec: bool,
    compose_paths_text: String,
    settings_saved: bool,
}

#[derive(Deserialize)]
struct NodePageQuery {
    saved: Option<String>,
}

struct MetricRow {
    metric: String,
    labels: String,
    value: String,
}

const SPARK_WINDOW_MS: i64 = 15 * 60 * 1000;

fn effective_dashboard(state: &AppState, node_id: &str) -> (Dashboard, &'static str) {
    let saved = state
        .stores
        .metadata
        .node_dashboard_json(node_id)
        .ok()
        .flatten();
    match saved.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => match serde_json::from_str::<Dashboard>(raw) {
            Ok(d) if d.validate().is_ok() => (d, "custom"),
            _ => (Dashboard::default_node(), "default"),
        },
        None => (Dashboard::default_node(), "default"),
    }
}

fn node_settings(state: &AppState, node_id: &str) -> NodeSettings {
    NodeSettings::parse_or_default(
        state
            .stores
            .metadata
            .node_settings_json(node_id)
            .ok()
            .flatten()
            .as_deref(),
    )
}

fn hydrate_node_widgets(
    state: &AppState,
    node_id: &str,
    latest: &[keystone_core::Sample],
) -> Vec<keystone_core::widgets::HydratedWidget> {
    let (dash, _) = effective_dashboard(state, node_id);
    let settings = node_settings(state, node_id);
    let since = chrono::Utc::now().timestamp_millis() - SPARK_WINDOW_MS;
    let mut history = HashMap::new();
    for w in &dash.widgets {
        if w.kind == WidgetKind::Sparkline {
            if let Some(metric) = w.metrics.first() {
                if let Ok(by_labels) = state.stores.series.history_all(node_id, metric, since) {
                    for (labels, points) in by_labels {
                        history.insert((metric.clone(), labels), points);
                    }
                }
            }
        }
    }
    hydrate(&dash, latest, &history, &settings)
}

async fn node_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<NodePageQuery>,
) -> Response {
    let Some(node) = state.stores.metadata.get_node(&id).ok().flatten() else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    let samples = state.stores.series.latest_samples(&id).unwrap_or_default();
    let (dash, dash_source) = effective_dashboard(&state, &id);
    let widgets_json = serde_json::to_string(&hydrate_node_widgets(&state, &id, &samples))
        .unwrap_or_else(|_| "[]".into());
    let layout_json = serde_json::to_string(&dash).unwrap_or_else(|_| "{}".into());
    let presets_json =
        serde_json::to_string(&presets_for_samples(&samples)).unwrap_or_else(|_| "[]".into());
    let metrics = samples
        .iter()
        .map(|s| MetricRow {
            metric: s.metric.clone(),
            labels: s.labels_key(),
            value: format!("{:.4}", s.value),
        })
        .collect();
    let connected = state.agents.is_connected(&id);
    let mut docker_error = String::new();
    let mut containers_json = "[]".into();
    let mut compose_json = "{}".into();
    let mut images_json = "[]".into();
    let mut volumes_json = "{}".into();
    let mut networks_json = "[]".into();
    if connected {
        match fetch_docker_bundle(&state, &id).await {
            Ok(bundle) => {
                containers_json = bundle.0;
                compose_json = bundle.1;
                images_json = bundle.2;
                volumes_json = bundle.3;
                networks_json = bundle.4;
            }
            Err(e) => docker_error = e.to_string(),
        }
    }
    let awaiting = node.awaiting_agent();
    let last_seen = if awaiting {
        "never".into()
    } else {
        node.last_seen().to_rfc3339()
    };
    let os = if node.os == "awaiting-agent" {
        String::new()
    } else {
        node.os
    };
    let settings = node_settings(&state, &id);
    let mut nics: Vec<String> = samples
        .iter()
        .filter_map(|s| {
            if s.metric.starts_with("node_network_") {
                s.labels
                    .iter()
                    .find(|l| l.name == "device")
                    .map(|l| l.value.clone())
            } else {
                None
            }
        })
        .collect();
    nics.sort();
    nics.dedup();
    Html(
        NodeTemplate {
            node_id: node.node_id,
            hostname: settings.display_host(&node.hostname).to_string(),
            os,
            kernel: node.kernel,
            agent_version: node.agent_version,
            docker_version: node.docker_version.unwrap_or_default(),
            last_seen,
            online: node.online,
            connected,
            awaiting,
            metrics,
            containers_json,
            compose_json,
            images_json,
            volumes_json,
            networks_json,
            docker_error,
            widgets_json,
            layout_json,
            presets_json,
            dash_source: dash_source.into(),
            display_name: settings.display_name.clone(),
            notes: settings.notes.clone(),
            network_devices_text: settings.network_devices.join("\n"),
            detected_nics: nics.join(", "),
            poll_secs: settings.poll_interval_secs() as u32,
            labels_text: settings.labels_text(),
            docker_enabled: settings.docker_enabled,
            docker_manage: settings.docker_manage,
            docker_allow_exec: settings.docker_allow_exec,
            compose_paths_text: settings.compose_paths.join("\n"),
            settings_saved: q.saved.as_deref() == Some("1"),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
}

#[derive(Deserialize)]
struct NodeSettingsForm {
    display_name: String,
    notes: String,
    #[serde(default)]
    network_devices: String,
    #[serde(default)]
    poll_secs: Option<u32>,
    #[serde(default)]
    labels: String,
    #[serde(default)]
    docker_enabled: String,
    #[serde(default)]
    docker_manage: String,
    #[serde(default)]
    docker_allow_exec: String,
    #[serde(default)]
    compose_paths: String,
}

fn form_flag(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "on" | "true" | "1" | "yes"
    )
}

async fn node_settings_post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<NodeSettingsForm>,
) -> Response {
    if state.stores.metadata.get_node(&id).ok().flatten().is_none() {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    }
    let docker_enabled = form_flag(&form.docker_enabled);
    let settings = NodeSettings {
        display_name: form.display_name.trim().to_string(),
        notes: form.notes.trim().to_string(),
        network_devices: NodeSettings::parse_lines(&form.network_devices),
        poll_secs: NodeSettings::clamp_poll_secs(form.poll_secs.unwrap_or(1)),
        docker_enabled,
        docker_manage: docker_enabled && form_flag(&form.docker_manage),
        docker_allow_exec: docker_enabled && form_flag(&form.docker_allow_exec),
        compose_paths: NodeSettings::parse_lines(&form.compose_paths),
        labels: NodeSettings::parse_labels(&form.labels),
    };
    let encoded = serde_json::to_string(&settings).unwrap_or_else(|_| "{}".into());
    let _ = state
        .stores
        .metadata
        .set_node_settings_json(&id, Some(&encoded));
    state.agents.nudge_runtime(&id, &settings);
    Redirect::to(&format!("/nodes/{id}?saved=1&panel=settings")).into_response()
}

async fn fetch_docker_bundle(
    state: &AppState,
    id: &str,
) -> anyhow::Result<(String, String, String, String, String)> {
    let c = call_json(state, id, DockerOp::ContainerList, "{}").await?;
    let p = call_json(state, id, DockerOp::ComposePs, "{}")
        .await
        .unwrap_or_else(|_| "{}".into());
    let i = call_json(state, id, DockerOp::ImageList, "{}")
        .await
        .unwrap_or_else(|_| "[]".into());
    let v = call_json(state, id, DockerOp::VolumeList, "{}")
        .await
        .unwrap_or_else(|_| "{}".into());
    let n = call_json(state, id, DockerOp::NetworkList, "{}")
        .await
        .unwrap_or_else(|_| "[]".into());
    Ok((c, p, i, v, n))
}

async fn call_json(
    state: &AppState,
    node_id: &str,
    op: DockerOp,
    payload: &str,
) -> anyhow::Result<String> {
    let result = state
        .agents
        .call(node_id, op.as_str(), payload.to_string())
        .await?;
    if !result.ok {
        anyhow::bail!("{}", result.error);
    }
    Ok(result.payload_json)
}

#[derive(Deserialize)]
struct DockerForm {
    payload: Option<String>,
    #[serde(default)]
    redirect: String,
}

async fn docker_action(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((id, op)): Path<(String, String)>,
    Form(form): Form<DockerForm>,
) -> Response {
    let username = jar
        .get(SESSION_COOKIE)
        .and_then(|c| {
            state
                .stores
                .metadata
                .get_session(c.value())
                .ok()
                .flatten()
                .map(|s| s.username)
        })
        .unwrap_or_else(|| "unknown".into());
    let parsed = match op.parse::<DockerOp>() {
        Ok(o) => o,
        Err(_) => return (StatusCode::BAD_REQUEST, "unknown op").into_response(),
    };
    let _perm: Permission = parsed.permission();
    let payload = form.payload.unwrap_or_else(|| "{}".into());
    let target = payload.clone();
    let result = state.agents.call(&id, parsed.as_str(), payload).await;
    let (ok, detail) = match &result {
        Ok(r) => (
            r.ok,
            if r.ok {
                r.payload_json.clone()
            } else {
                r.error.clone()
            },
        ),
        Err(e) => (false, e.to_string()),
    };
    let _ = state
        .stores
        .metadata
        .audit(&username, &id, parsed.as_str(), &target, ok, &detail);
    let dest = if form.redirect.is_empty() {
        format!("/nodes/{id}")
    } else {
        form.redirect
    };
    Redirect::to(&dest).into_response()
}

async fn container_logs_sse(
    State(state): State<AppState>,
    Path((id, cid)): Path<(String, String)>,
) -> Response {
    let payload = serde_json::json!({"id": cid, "tail": "200"}).to_string();
    match state
        .agents
        .call(&id, DockerOp::ContainerLogs.as_str(), payload)
        .await
    {
        Ok(r) if r.ok => {
            let body = format!("data: {}\n\n", r.payload_json.replace('\n', "\\n"));
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from(body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Ok(r) => (StatusCode::BAD_GATEWAY, r.error).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

async fn container_stats_sse(
    State(state): State<AppState>,
    Path((id, cid)): Path<(String, String)>,
) -> Response {
    let payload = serde_json::json!({"id": cid}).to_string();
    match state
        .agents
        .call(&id, DockerOp::ContainerStats.as_str(), payload)
        .await
    {
        Ok(r) if r.ok => {
            let body = format!("data: {}\n\n", r.payload_json);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from(body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Ok(r) => (StatusCode::BAD_GATEWAY, r.error).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

#[derive(Template)]
#[template(path = "help.html")]
struct HelpTemplate {
    sections: Vec<(String, String)>,
    title: String,
    body_html: String,
}

async fn help_index() -> impl IntoResponse {
    let sections = help::sections();
    let first = sections.first();
    let title = first
        .map(|s| s.title.clone())
        .unwrap_or_else(|| "Help".into());
    let md = first.map(|s| s.markdown.clone()).unwrap_or_default();
    Html(
        HelpTemplate {
            sections: sections
                .iter()
                .map(|s| (s.slug.clone(), s.title.clone()))
                .collect(),
            title,
            body_html: help::markdown_to_html(&md),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

async fn help_section(Path(slug): Path<String>) -> Response {
    let Some(sec) = help::section(&slug) else {
        return (StatusCode::NOT_FOUND, "unknown help section").into_response();
    };
    let sections = help::sections();
    Html(
        HelpTemplate {
            sections: sections
                .iter()
                .map(|s| (s.slug.clone(), s.title.clone()))
                .collect(),
            title: sec.title,
            body_html: help::markdown_to_html(&sec.markdown),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
}

#[derive(Serialize, ToSchema)]
pub struct CatalogMetric {
    pub name: String,
    pub metric_type: String,
    pub unit: String,
    pub help: String,
    pub labels: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CatalogApi {
    pub metrics: Vec<CatalogMetric>,
}

/// Metric catalog allowlist compiled into this binary.
#[utoipa::path(
    get,
    path = "/api/v1/catalog",
    responses((status = 200, description = "Catalog", body = CatalogApi))
)]
pub async fn catalog_api() -> Json<CatalogApi> {
    let metrics = catalog()
        .iter()
        .map(|d| CatalogMetric {
            name: d.name.to_string(),
            metric_type: format!("{:?}", d.metric_type),
            unit: d.unit.to_string(),
            help: d.help.to_string(),
            labels: d.labels.iter().map(|s| s.to_string()).collect(),
        })
        .collect();
    Json(CatalogApi { metrics })
}

#[derive(Serialize, ToSchema)]
pub struct NodeDashboardApi {
    pub source: String,
    pub layout: serde_json::Value,
    pub widgets: serde_json::Value,
}

/// Layout JSON plus hydrated widget values. PUT saves a custom per-node layout.
#[utoipa::path(
    get,
    path = "/api/v1/nodes/{id}/dashboard",
    params(("id" = String, Path, description = "Node id")),
    responses((status = 200, description = "Dashboard layout and widgets", body = NodeDashboardApi))
)]
pub async fn dashboard_get(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if state.stores.metadata.get_node(&id).ok().flatten().is_none() {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    }
    let latest = state.stores.series.latest_samples(&id).unwrap_or_default();
    let (dash, source) = effective_dashboard(&state, &id);
    let widgets = hydrate_node_widgets(&state, &id, &latest);
    Json(NodeDashboardApi {
        source: source.into(),
        layout: serde_json::to_value(&dash).unwrap_or(serde_json::Value::Null),
        widgets: serde_json::to_value(&widgets).unwrap_or(serde_json::Value::Null),
    })
    .into_response()
}

#[utoipa::path(
    put,
    path = "/api/v1/nodes/{id}/dashboard",
    params(("id" = String, Path, description = "Node id")),
    responses(
        (status = 204, description = "Saved"),
        (status = 400, description = "Invalid layout"),
        (status = 404, description = "Unknown node")
    )
)]
/// Save a custom per-node dashboard layout.
pub async fn dashboard_put(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let dash: Dashboard = match serde_json::from_value(body) {
        Ok(d) => d,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    if let Err(e) = dash.validate() {
        return (StatusCode::BAD_REQUEST, e).into_response();
    }
    let encoded = match serde_json::to_string(&dash) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    match state
        .stores
        .metadata
        .set_node_dashboard_json(&id, Some(&encoded))
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/nodes/{id}/dashboard",
    params(("id" = String, Path, description = "Node id")),
    responses(
        (status = 204, description = "Reset to default"),
        (status = 404, description = "Unknown node")
    )
)]
/// Clear a custom layout so the built-in default is used again.
pub async fn dashboard_delete(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if state.stores.metadata.get_node(&id).ok().flatten().is_none() {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    }
    match state.stores.metadata.set_node_dashboard_json(&id, None) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

#[allow(dead_code)]
fn _headers(_: HeaderMap, _: HashMap<String, String>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_strips_numeric_port_only() {
        assert_eq!(host_without_port("127.0.0.1:8080"), "127.0.0.1");
        assert_eq!(host_without_port("127.0.0.1"), "127.0.0.1");
        assert_eq!(host_without_port("lab.example.com:8080"), "lab.example.com");
        assert_eq!(host_without_port("[::1]:8080"), "[::1]");
        assert_eq!(host_without_port("[::1]"), "[::1]");
    }

    #[test]
    fn slug_from_hostname() {
        assert_eq!(slug_node_id("Pi Hole", "").unwrap(), "pi-hole");
        assert_eq!(slug_node_id("pi-hole", "Custom_ID").unwrap(), "custom_id");
        assert!(slug_node_id("   ", "").is_err());
    }
}
