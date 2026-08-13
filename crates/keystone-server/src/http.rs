// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;

use askama::Template;
use axum::body::Body;
use axum::extract::{Form, Path, State};
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
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth;
use crate::help;
use crate::state::AppState;

const SESSION_COOKIE: &str = "keystone_session";

pub fn router(state: AppState) -> Router {
    let authed = Router::new()
        .route("/", get(nodes_page))
        .route("/nodes", get(nodes_page))
        .route("/nodes/{id}", get(node_page))
        .route("/nodes/{id}/docker/{op}", post(docker_action))
        .route("/nodes/{id}/containers/{cid}/logs", get(container_logs_sse))
        .route(
            "/nodes/{id}/containers/{cid}/stats",
            get(container_stats_sse),
        )
        .route("/help", get(help_index))
        .route("/help/{slug}", get(help_section))
        .route("/api/v1/catalog", get(catalog_api))
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
    online: bool,
    connected: bool,
}

async fn nodes_page(State(state): State<AppState>) -> impl IntoResponse {
    let nodes = state.stores.metadata.list_nodes().unwrap_or_default();
    let rows = nodes
        .into_iter()
        .map(|n| {
            let last_seen = n.last_seen().to_rfc3339();
            NodeRow {
                connected: state.agents.is_connected(&n.node_id),
                node_id: n.node_id,
                hostname: n.hostname,
                os: n.os,
                agent_version: n.agent_version,
                docker_version: n.docker_version.unwrap_or_default(),
                last_seen,
                online: n.online,
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
    metrics: Vec<MetricRow>,
    containers_json: String,
    compose_json: String,
    images_json: String,
    volumes_json: String,
    networks_json: String,
    docker_error: String,
}

struct MetricRow {
    metric: String,
    labels: String,
    value: String,
}

async fn node_page(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(node) = state.stores.metadata.get_node(&id).ok().flatten() else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    let samples = state.stores.series.latest_samples(&id).unwrap_or_default();
    let metrics = samples
        .into_iter()
        .map(|s| {
            let labels = s.labels_key();
            MetricRow {
                metric: s.metric,
                labels,
                value: format!("{:.4}", s.value),
            }
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
    let last_seen = node.last_seen().to_rfc3339();
    Html(
        NodeTemplate {
            node_id: node.node_id,
            hostname: node.hostname,
            os: node.os,
            kernel: node.kernel,
            agent_version: node.agent_version,
            docker_version: node.docker_version.unwrap_or_default(),
            last_seen,
            online: node.online,
            connected,
            metrics,
            containers_json,
            compose_json,
            images_json,
            volumes_json,
            networks_json,
            docker_error,
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
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

#[allow(dead_code)]
fn _headers(_: HeaderMap, _: HashMap<String, String>) {}
