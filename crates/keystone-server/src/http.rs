// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use askama::Template;
use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use futures_util::Stream;
use keystone_core::docker::DockerOp;
use keystone_core::fleet::{fleet_chips, FleetChip};
use keystone_core::metrics::catalog;
use keystone_core::sys::SysOp;
use keystone_core::widgets::{hydrate, presets_for_samples, Dashboard, WidgetKind};
use keystone_core::{NodeSettings, ServerSettings};
use keystone_proto::StreamChunk;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::auth;
use crate::help;
use crate::state::AppState;
use crate::totp;

const SESSION_COOKIE: &str = "keystone_session";
const SESSION_SECS: i64 = 86400 * 7;
const PENDING_2FA_SECS: i64 = 5 * 60;

fn forwarded_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.eq_ignore_ascii_case("https"))
}

fn session_cookie(id: String, headers: &HeaderMap, ui_https: bool) -> Cookie<'static> {
    let mut cookie = Cookie::new(SESSION_COOKIE, id);
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    if ui_https || forwarded_https(headers) {
        cookie.set_secure(true);
    }
    cookie
}

fn expiry_unix(secs: i64) -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + secs
}

pub fn router(state: AppState) -> Router {
    let authed = Router::new()
        .route("/", get(nodes_page))
        .route("/nodes", get(nodes_page).post(add_node_post))
        .route("/nodes/new", get(add_node_page))
        .route("/nodes/{id}", get(node_page))
        .route("/nodes/{id}/setup", get(node_setup_page))
        .route("/nodes/{id}/settings", post(node_settings_post))
        .route("/alerts", get(alerts_page))
        .route("/audit", get(audit_page))
        .route("/password", get(password_page).post(password_post))
        .route("/login/totp", get(totp_login_page).post(totp_login_post))
        .route("/settings", get(settings_page).post(settings_post))
        .route("/settings/rotate-token", post(settings_rotate_token))
        .route("/settings/totp", get(totp_setup_page))
        .route("/settings/totp/start", post(totp_start_post))
        .route("/settings/totp/confirm", post(totp_confirm_post))
        .route("/settings/totp/disable", post(totp_disable_post))
        .route("/nodes/{id}/docker/{op}", post(docker_action))
        .route("/nodes/{id}/sys/{op}", post(sys_action))
        .route("/nodes/{id}/sys/updates", get(sys_updates_page))
        .route("/nodes/{id}/sys/updates/stream", get(sys_updates_sse))
        .route("/nodes/{id}/sys/gitlab-backup", get(sys_gitlab_backup_page))
        .route(
            "/nodes/{id}/sys/gitlab-backup/stream",
            get(sys_gitlab_backup_sse),
        )
        .route(
            "/nodes/{id}/containers/{cid}/logs",
            get(container_logs_page),
        )
        .route(
            "/nodes/{id}/containers/{cid}/logs/stream",
            get(container_logs_sse),
        )
        .route(
            "/nodes/{id}/containers/{cid}/stats",
            get(container_stats_json),
        )
        .route("/nodes/{id}/compose/{project}/logs", get(compose_logs_page))
        .route(
            "/nodes/{id}/compose/{project}/logs/stream",
            get(compose_logs_sse),
        )
        .route("/help", get(help_index))
        .route("/help/{slug}", get(help_section))
        .route("/api/v1/catalog", get(catalog_api))
        .route("/api/v1/alerts", get(alerts_api))
        .route("/api/v1/nodes", get(nodes_api))
        .route("/api/v1/dockerhub/search", get(crate::dockerhub::search))
        .route("/api/v1/dockerhub/tags", get(crate::dockerhub::tags))
        .route("/api/v1/nodes/{id}/sys/updates", get(sys_updates_api))
        .route(
            "/api/v1/nodes/{id}/container-usage",
            get(container_usage_api),
        )
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
        Ok(Some(sess)) => {
            let path = request.uri().path();
            if sess.pending_2fa {
                if path != "/login/totp" && path != "/logout" {
                    return Redirect::to("/login/totp").into_response();
                }
            } else if state
                .stores
                .metadata
                .user_must_change_password(&sess.username)
                .unwrap_or(false)
                && path != "/password"
                && path != "/logout"
            {
                return Redirect::to("/password").into_response();
            }
            next.run(request).await
        }
        _ => Redirect::to("/login").into_response(),
    }
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: String,
    username: String,
}

fn login_view(state: &AppState, error: String) -> LoginTemplate {
    LoginTemplate {
        error,
        username: state.config.auth.username.clone(),
    }
}

async fn login_page(State(state): State<AppState>) -> impl IntoResponse {
    Html(
        login_view(&state, String::new())
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
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let login_fail = |error: String| {
        (
            StatusCode::UNAUTHORIZED,
            Html(login_view(&state, error).render().unwrap_or_default()),
        )
            .into_response()
    };
    if state.login_gate.lock().locked(&form.username) {
        return login_fail("too many attempts; try again in a few minutes".into());
    }
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
        state.login_gate.lock().record_fail(&form.username);
        return login_fail("Invalid username or password".into());
    }
    let totp_on = state
        .stores
        .metadata
        .user_totp_enabled(&form.username)
        .unwrap_or(false);
    let sid = auth::new_session_id();
    let (expires, pending, next) = if totp_on {
        (expiry_unix(PENDING_2FA_SECS), true, "/login/totp")
    } else if state
        .stores
        .metadata
        .user_must_change_password(&form.username)
        .unwrap_or(false)
    {
        (expiry_unix(SESSION_SECS), false, "/password")
    } else {
        (expiry_unix(SESSION_SECS), false, "/")
    };
    let _ = state
        .stores
        .metadata
        .put_session(&sid, &form.username, expires, pending);
    if !pending {
        state.login_gate.lock().clear(&form.username);
    }
    (
        jar.add(session_cookie(sid, &headers, state.config.tls.ui_https())),
        Redirect::to(next),
    )
        .into_response()
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
#[template(path = "totp.html")]
struct TotpLoginTemplate {
    error: String,
}

async fn totp_login_page(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Some(sess) = session_record(&state, &jar) else {
        return Redirect::to("/login").into_response();
    };
    if !sess.pending_2fa {
        return Redirect::to("/").into_response();
    }
    Html(
        TotpLoginTemplate {
            error: String::new(),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
}

#[derive(Deserialize)]
struct TotpLoginForm {
    #[serde(default)]
    code: String,
}

async fn totp_login_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<TotpLoginForm>,
) -> Response {
    let fail = |error: String| {
        Html(
            TotpLoginTemplate { error }
                .render()
                .unwrap_or_else(|e| e.to_string()),
        )
        .into_response()
    };
    let Some(sess) = session_record(&state, &jar) else {
        return Redirect::to("/login").into_response();
    };
    if !sess.pending_2fa {
        return Redirect::to("/").into_response();
    }
    if state.login_gate.lock().locked(&sess.username) {
        return fail("too many attempts; try again in a few minutes".into());
    }
    let Some(mut rec) = state
        .stores
        .metadata
        .user_totp(&sess.username)
        .ok()
        .flatten()
    else {
        return Redirect::to("/login").into_response();
    };
    let mut ok = false;
    if rec.enabled {
        if let Some(step) =
            totp::verify_code_step(&rec.secret, &sess.username, &form.code, Some(rec.last_step))
        {
            rec.last_step = step;
            let _ = state.stores.metadata.set_user_totp(&sess.username, &rec);
            ok = true;
        }
    }
    if !ok {
        let hashes = totp::parse_backup_hashes(&rec.backup_json);
        if let Some(rest) = totp::take_backup_code(&hashes, &form.code) {
            rec.backup_json = totp::backup_hashes_json(&rest);
            let _ = state.stores.metadata.set_user_totp(&sess.username, &rec);
            ok = true;
        }
    }
    if !ok {
        state.login_gate.lock().record_fail(&sess.username);
        return fail("invalid authenticator or backup code".into());
    }
    state.login_gate.lock().clear(&sess.username);
    let _ = state.stores.metadata.delete_session(&sess.id);
    let sid = auth::new_session_id();
    let next = if state
        .stores
        .metadata
        .user_must_change_password(&sess.username)
        .unwrap_or(false)
    {
        "/password"
    } else {
        "/"
    };
    let _ =
        state
            .stores
            .metadata
            .put_session(&sid, &sess.username, expiry_unix(SESSION_SECS), false);
    let mut old = Cookie::from(SESSION_COOKIE);
    old.set_path("/");
    (
        jar.remove(old)
            .add(session_cookie(sid, &headers, state.config.tls.ui_https())),
        Redirect::to(next),
    )
        .into_response()
}

fn session_record(state: &AppState, jar: &CookieJar) -> Option<keystone_store::SessionRecord> {
    let cookie = jar.get(SESSION_COOKIE)?;
    state
        .stores
        .metadata
        .get_session(cookie.value())
        .ok()
        .flatten()
}

#[derive(Template)]
#[template(path = "password.html")]
struct PasswordTemplate {
    error: String,
}

async fn password_page(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Some(username) = session_username(&state, &jar) else {
        return Redirect::to("/login").into_response();
    };
    if !state
        .stores
        .metadata
        .user_must_change_password(&username)
        .unwrap_or(false)
    {
        return Redirect::to("/").into_response();
    }
    Html(
        PasswordTemplate {
            error: String::new(),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
}

#[derive(Deserialize)]
struct PasswordForm {
    #[serde(default)]
    new_password: String,
    #[serde(default)]
    new_password_confirm: String,
}

async fn password_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<PasswordForm>,
) -> Response {
    let fail = |error: String| {
        Html(
            PasswordTemplate { error }
                .render()
                .unwrap_or_else(|e| e.to_string()),
        )
        .into_response()
    };
    let Some(username) = session_username(&state, &jar) else {
        return Redirect::to("/login").into_response();
    };
    if !state
        .stores
        .metadata
        .user_must_change_password(&username)
        .unwrap_or(false)
    {
        return Redirect::to("/").into_response();
    }
    if let Err(error) = auth::validate_new_password(&form.new_password, &form.new_password_confirm)
    {
        return fail(error);
    }
    let current = state.stores.metadata.user_hash(&username).ok().flatten();
    if current
        .as_deref()
        .map(|h| auth::verify_password(&form.new_password, h))
        .unwrap_or(false)
    {
        return fail("choose a different password from the bootstrap one".into());
    }
    match auth::hash_password(&form.new_password) {
        Ok(hash) => {
            if let Err(e) = state
                .stores
                .metadata
                .set_user_password(&username, &hash, false)
            {
                return fail(format!("could not update password: {e}"));
            }
        }
        Err(e) => return fail(format!("could not hash password: {e}")),
    }
    Redirect::to("/?welcome=1").into_response()
}

fn session_username(state: &AppState, jar: &CookieJar) -> Option<String> {
    let cookie = jar.get(SESSION_COOKIE)?;
    state
        .stores
        .metadata
        .get_session(cookie.value())
        .ok()
        .flatten()
        .map(|s| s.username)
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate {
    retention_hours: u32,
    ingest_token: String,
    ingest_token_env_override: bool,
    prometheus_scrape: String,
    snmp_scrape: String,
    alert_webhook_url: String,
    username: String,
    totp_enabled: bool,
    saved: bool,
    rotated: bool,
    error: String,
}

#[derive(Deserialize)]
struct SettingsQuery {
    saved: Option<String>,
    rotated: Option<String>,
    err: Option<String>,
}

fn settings_err_message(err: Option<&str>) -> String {
    match err {
        Some("totp") => "password or code did not match; authenticator was not changed".into(),
        Some("totp-on") => "authenticator is already enabled".into(),
        _ => String::new(),
    }
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
        alert_webhook_url: stored.alert_webhook_url.clone(),
        username: state.config.auth.username.clone(),
        totp_enabled: state
            .stores
            .metadata
            .user_totp_enabled(&state.config.auth.username)
            .unwrap_or(false),
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
            settings_err_message(q.err.as_deref()),
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
    alert_webhook_url: String,
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
        stored.alert_webhook_url = form.alert_webhook_url.clone();
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
    let webhook = match ServerSettings::parse_webhook_url(&form.alert_webhook_url) {
        Ok(u) => u,
        Err(error) => return fail(&state, error, &form),
    };
    if !form.new_password.is_empty() || !form.new_password_confirm.is_empty() {
        if let Err(error) =
            auth::validate_new_password(&form.new_password, &form.new_password_confirm)
        {
            return fail(&state, error, &form);
        }
        let current = state
            .stores
            .metadata
            .user_hash(&state.config.auth.username)
            .ok()
            .flatten();
        if current
            .as_deref()
            .map(|h| auth::verify_password(&form.new_password, h))
            .unwrap_or(false)
        {
            return fail(&state, "choose a different password".into(), &form);
        }
        match auth::hash_password(&form.new_password) {
            Ok(hash) => {
                if let Err(e) = state.stores.metadata.set_user_password(
                    &state.config.auth.username,
                    &hash,
                    false,
                ) {
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
    next.alert_webhook_url = webhook;
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
#[template(path = "totp_setup.html")]
struct TotpSetupTemplate {
    error: String,
    secret: String,
    qr_svg: String,
}

#[derive(Template)]
#[template(path = "totp_backup.html")]
struct TotpBackupTemplate {
    codes: Vec<String>,
}

#[derive(Deserialize)]
struct TotpPasswordForm {
    #[serde(default)]
    password: String,
}

#[derive(Deserialize)]
struct TotpConfirmForm {
    #[serde(default)]
    code: String,
}

#[derive(Deserialize)]
struct TotpDisableForm {
    #[serde(default)]
    password: String,
    #[serde(default)]
    code: String,
}

async fn totp_setup_page(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Some(username) = session_username(&state, &jar) else {
        return Redirect::to("/login").into_response();
    };
    let Some(rec) = state.stores.metadata.user_totp(&username).ok().flatten() else {
        return Redirect::to("/settings").into_response();
    };
    if rec.pending.is_empty() {
        return Redirect::to("/settings").into_response();
    }
    let url = match totp::otpauth_url(&rec.pending, &username) {
        Ok(u) => u,
        Err(e) => {
            return Html(
                TotpSetupTemplate {
                    error: e,
                    secret: rec.pending,
                    qr_svg: String::new(),
                }
                .render()
                .unwrap_or_default(),
            )
            .into_response();
        }
    };
    let qr_svg = totp::qr_svg(&url).unwrap_or_default();
    Html(
        TotpSetupTemplate {
            error: String::new(),
            secret: rec.pending,
            qr_svg,
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
}

async fn totp_start_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<TotpPasswordForm>,
) -> Response {
    let Some(username) = session_username(&state, &jar) else {
        return Redirect::to("/login").into_response();
    };
    let hash = state.stores.metadata.user_hash(&username).ok().flatten();
    let ok = hash
        .as_deref()
        .map(|h| auth::verify_password(&form.password, h))
        .unwrap_or(false);
    if !ok {
        return Redirect::to("/settings?err=totp").into_response();
    }
    let mut rec = state
        .stores
        .metadata
        .user_totp(&username)
        .ok()
        .flatten()
        .unwrap_or_default();
    if rec.enabled {
        return Redirect::to("/settings?err=totp-on").into_response();
    }
    rec.pending = totp::new_secret();
    let _ = state.stores.metadata.set_user_totp(&username, &rec);
    Redirect::to("/settings/totp").into_response()
}

async fn totp_confirm_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<TotpConfirmForm>,
) -> Response {
    let fail = |state: &AppState, username: &str, error: String| {
        let rec = state
            .stores
            .metadata
            .user_totp(username)
            .ok()
            .flatten()
            .unwrap_or_default();
        let url = totp::otpauth_url(&rec.pending, username).unwrap_or_default();
        let qr_svg = if rec.pending.is_empty() {
            String::new()
        } else {
            totp::qr_svg(&url).unwrap_or_default()
        };
        Html(
            TotpSetupTemplate {
                error,
                secret: rec.pending,
                qr_svg,
            }
            .render()
            .unwrap_or_default(),
        )
        .into_response()
    };
    let Some(username) = session_username(&state, &jar) else {
        return Redirect::to("/login").into_response();
    };
    let Some(mut rec) = state.stores.metadata.user_totp(&username).ok().flatten() else {
        return Redirect::to("/settings").into_response();
    };
    if rec.pending.is_empty() {
        return Redirect::to("/settings").into_response();
    }
    let Some(step) = totp::verify_code_step(&rec.pending, &username, &form.code, None) else {
        return fail(
            &state,
            &username,
            "that code did not match; try the next one".into(),
        );
    };
    let codes = totp::generate_backup_codes();
    let hashes = match totp::hash_backup_codes(&codes) {
        Ok(h) => h,
        Err(e) => {
            return fail(
                &state,
                &username,
                format!("could not store backup codes: {e}"),
            )
        }
    };
    rec.secret = rec.pending.clone();
    rec.pending = String::new();
    rec.enabled = true;
    rec.backup_json = totp::backup_hashes_json(&hashes);
    rec.last_step = step;
    if let Err(e) = state.stores.metadata.set_user_totp(&username, &rec) {
        return fail(
            &state,
            &username,
            format!("could not enable authenticator: {e}"),
        );
    }
    Html(
        TotpBackupTemplate { codes }
            .render()
            .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
}

async fn totp_disable_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<TotpDisableForm>,
) -> Response {
    let Some(username) = session_username(&state, &jar) else {
        return Redirect::to("/login").into_response();
    };
    let hash = state.stores.metadata.user_hash(&username).ok().flatten();
    let ok_pass = hash
        .as_deref()
        .map(|h| auth::verify_password(&form.password, h))
        .unwrap_or(false);
    let Some(mut rec) = state.stores.metadata.user_totp(&username).ok().flatten() else {
        return Redirect::to("/settings").into_response();
    };
    let ok_code = rec.enabled && totp::verify_code(&rec.secret, &username, &form.code);
    let ok_backup =
        totp::take_backup_code(&totp::parse_backup_hashes(&rec.backup_json), &form.code).is_some();
    if !ok_pass || !(ok_code || ok_backup) {
        return Redirect::to("/settings?err=totp").into_response();
    }
    rec.secret.clear();
    rec.pending.clear();
    rec.enabled = false;
    rec.backup_json = "[]".into();
    rec.last_step = 0;
    let _ = state.stores.metadata.set_user_totp(&username, &rec);
    Redirect::to("/settings?saved=1").into_response()
}

#[derive(Template)]
#[template(path = "nodes.html")]
struct NodesTemplate {
    nodes: Vec<NodeRow>,
    totp_enabled: bool,
}

#[derive(Serialize)]
struct NodeRow {
    node_id: String,
    hostname: String,
    os: String,
    status: String,
    last_seen: String,
    chips: Vec<FleetChip>,
    alert_count: usize,
}

fn node_status(state: &AppState, n: &keystone_store::NodeRecord) -> String {
    if state.agents.is_connected(&n.node_id) {
        "connected".into()
    } else if n.awaiting_agent() {
        "awaiting agent".into()
    } else if n.online {
        "seen".into()
    } else {
        "offline".into()
    }
}

fn relative_seen(last_seen_unix: i64, awaiting: bool) -> String {
    if awaiting {
        return "never".into();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(last_seen_unix);
    let d = (now - last_seen_unix).max(0);
    if d < 5 {
        "just now".into()
    } else if d < 60 {
        format!("{d}s ago")
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else {
        format!("{}d ago", d / 86400)
    }
}

fn fleet_row(state: &AppState, n: keystone_store::NodeRecord) -> NodeRow {
    let awaiting = n.awaiting_agent();
    let status = node_status(state, &n);
    let settings = NodeSettings::parse_or_default(
        state
            .stores
            .metadata
            .node_settings_json(&n.node_id)
            .ok()
            .flatten()
            .as_deref(),
    );
    let samples = state
        .stores
        .series
        .latest_samples(&n.node_id)
        .unwrap_or_default();
    let chips = fleet_chips(&samples);
    let alert_count = chips.iter().filter(|c| c.is_firing()).count();
    NodeRow {
        hostname: settings.display_host(&n.hostname).to_string(),
        os: if n.os == "awaiting-agent" {
            String::new()
        } else {
            n.os
        },
        status,
        last_seen: relative_seen(n.last_seen_unix, awaiting),
        chips,
        alert_count,
        node_id: n.node_id,
    }
}

#[derive(Serialize)]
struct AlertRow {
    node_id: String,
    hostname: String,
    chip: String,
    label: String,
    severity: String,
    display: String,
    hint: String,
}

fn firing_rows(state: &AppState) -> Vec<AlertRow> {
    let mut out = Vec::new();
    for n in state.stores.metadata.list_nodes().unwrap_or_default() {
        let row = fleet_row(state, n);
        for c in row.chips.into_iter().filter(|c| c.is_firing()) {
            out.push(AlertRow {
                node_id: row.node_id.clone(),
                hostname: row.hostname.clone(),
                chip: c.id,
                label: c.label,
                severity: c.tone,
                display: c.display,
                hint: c.hint,
            });
        }
    }
    out
}

#[derive(Template)]
#[template(path = "alerts.html")]
struct AlertsTemplate {
    alerts: Vec<AlertRow>,
}

#[derive(Serialize)]
struct AlertsApi {
    alerts: Vec<AlertRow>,
}

async fn alerts_page(State(state): State<AppState>) -> impl IntoResponse {
    Html(
        AlertsTemplate {
            alerts: firing_rows(&state),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

async fn alerts_api(State(state): State<AppState>) -> Json<AlertsApi> {
    Json(AlertsApi {
        alerts: firing_rows(&state),
    })
}

const AUDIT_PAGE_LIMIT: i64 = 200;

#[derive(Template)]
#[template(path = "audit.html")]
struct AuditTemplate {
    rows: Vec<AuditRow>,
    limit: i64,
}

struct AuditRow {
    when: String,
    at_rfc3339: String,
    username: String,
    node_id: String,
    op: String,
    target: String,
    ok: bool,
    detail: String,
}

fn unix_rfc3339(at_unix: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(at_unix, 0)
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| at_unix.to_string())
}

async fn audit_page(State(state): State<AppState>) -> impl IntoResponse {
    let rows = state
        .stores
        .metadata
        .recent_audit(AUDIT_PAGE_LIMIT)
        .unwrap_or_default()
        .into_iter()
        .map(|e| AuditRow {
            when: relative_seen(e.at_unix, false),
            at_rfc3339: unix_rfc3339(e.at_unix),
            username: e.username,
            node_id: e.node_id,
            op: e.op,
            target: if e.target.is_empty() {
                "—".into()
            } else {
                e.target
            },
            ok: e.ok,
            detail: e.detail,
        })
        .collect();
    Html(
        AuditTemplate {
            rows,
            limit: AUDIT_PAGE_LIMIT,
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

async fn nodes_page(State(state): State<AppState>) -> impl IntoResponse {
    let nodes = state
        .stores
        .metadata
        .list_nodes()
        .unwrap_or_default()
        .into_iter()
        .map(|n| fleet_row(&state, n))
        .collect();
    let totp_enabled = state
        .stores
        .metadata
        .user_totp_enabled(&state.config.auth.username)
        .unwrap_or(false);
    Html(
        NodesTemplate {
            nodes,
            totp_enabled,
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

#[derive(Serialize)]
struct NodesApi {
    nodes: Vec<NodeRow>,
}

async fn nodes_api(State(state): State<AppState>) -> Json<NodesApi> {
    let nodes = state
        .stores
        .metadata
        .list_nodes()
        .unwrap_or_default()
        .into_iter()
        .map(|n| fleet_row(&state, n))
        .collect();
    Json(NodesApi { nodes })
}

#[derive(Template)]
#[template(path = "node_new.html")]
struct NodeNewTemplate {
    ingest_url: String,
    explicit_ingest_url: String,
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

fn suggested_ingest_url(headers: &HeaderMap, cfg: &keystone_core::config::ServerConfig) -> String {
    let grpc_port = cfg.grpc_listen.rsplit(':').next().unwrap_or("9100");
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1");
    let scheme = if cfg.tls.ingest_https() {
        "https"
    } else {
        "http"
    };
    format!("{scheme}://{}:{grpc_port}", host_without_port(host))
}

/// Packaged / same-LAN default. Ingest TLS needs a hostname that matches
/// the cert, so that path keeps the explicit URL from the Host header.
fn default_agent_ingest_url(
    headers: &HeaderMap,
    cfg: &keystone_core::config::ServerConfig,
) -> String {
    if cfg.tls.ingest_https() {
        suggested_ingest_url(headers, cfg)
    } else {
        "mdns".into()
    }
}

fn node_new_page(
    headers: &HeaderMap,
    cfg: &keystone_core::config::ServerConfig,
    error: String,
) -> NodeNewTemplate {
    NodeNewTemplate {
        ingest_url: default_agent_ingest_url(headers, cfg),
        explicit_ingest_url: suggested_ingest_url(headers, cfg),
        error,
    }
}

fn agent_toml_snippet(ingest_url: &str, explicit: &str, token: &str, node_id: &str) -> String {
    let mut s = format!(
        "ingest_url = \"{ingest_url}\"\ningest_token = \"{token}\"\nnode_id = \"{node_id}\"\nbuffer_dir = \"/var/lib/keystone/agent-buffer\"\n",
    );
    if keystone_core::wants_mdns(ingest_url) {
        s.push_str(&format!(
            "# ingest_url = \"{explicit}\"  # other subnet, or if mDNS is blocked\n",
        ));
    }
    if ingest_url.starts_with("https://") || explicit.starts_with("https://") {
        s.push_str("# tls_ca_file = \"/etc/keystone/ca.pem\"  # private CA or self-signed only\n");
    }
    s
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
        node_new_page(&headers, &state.config, String::new())
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
                    node_new_page(
                        &headers,
                        &state.config,
                        format!("could not register node: {e}"),
                    )
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
                .unwrap_or_else(|| default_agent_ingest_url(&headers, &state.config));
            let qs = format!(
                "/nodes/{node_id}/setup?ingest_url={}&docker={}",
                urlencoding_lite(&ingest),
                docker
            );
            Redirect::to(&qs).into_response()
        }
        Err(error) => Html(
            node_new_page(&headers, &state.config, error)
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
        .unwrap_or_else(|| default_agent_ingest_url(&headers, &state.config));
    let settings = node_settings(&state, &id);
    let docker = q.docker.as_deref() == Some("true") || settings.docker_enabled;
    let token = state.ingest_token();
    let awaiting = node.awaiting_agent();
    let explicit = suggested_ingest_url(&headers, &state.config);
    let agent_toml = agent_toml_snippet(&ingest_url, &explicit, &token, &node.node_id);
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
    docker_reason: String,
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
    sys_enabled: bool,
    sys_manage: bool,
    sys_reason: String,
    sys_json: String,
    sys_error: String,
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
            Ok(mut d) => {
                d.normalize();
                if d.validate().is_ok() {
                    (d, "custom")
                } else {
                    (Dashboard::default_node(), "default")
                }
            }
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
    let settings = node_settings(&state, &id);
    let sys_task = load_sys_tab(&state, &id, connected, settings.sys_enabled);
    let docker_task = load_docker_tabs(&state, &id, connected, settings.docker_enabled);
    let (sys_tab, docker_tab) = tokio::join!(sys_task, docker_task);
    let sys_reason = sys_tab.0;
    let sys_error = sys_tab.1;
    let sys_json = sys_tab.2;
    let docker_reason = docker_tab.0;
    let docker_error = docker_tab.1;
    let containers_json = docker_tab.2;
    let compose_json = docker_tab.3;
    let images_json = docker_tab.4;
    let volumes_json = docker_tab.5;
    let networks_json = docker_tab.6;
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
            docker_reason,
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
            sys_enabled: settings.sys_enabled,
            sys_manage: settings.sys_manage,
            sys_reason,
            sys_json,
            sys_error,
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
    #[serde(default)]
    sys_enabled: String,
    #[serde(default)]
    sys_manage: String,
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
        sys_enabled: form_flag(&form.sys_enabled),
        sys_manage: form_flag(&form.sys_enabled) && form_flag(&form.sys_manage),
    };
    let encoded = serde_json::to_string(&settings).unwrap_or_else(|_| "{}".into());
    let _ = state
        .stores
        .metadata
        .set_node_settings_json(&id, Some(&encoded));
    state.agents.nudge_runtime(&id, &settings);
    Redirect::to(&format!("/nodes/{id}?saved=1&panel=settings")).into_response()
}

async fn load_sys_tab(
    state: &AppState,
    id: &str,
    connected: bool,
    enabled: bool,
) -> (String, String, String) {
    if !connected {
        return ("offline".into(), String::new(), "{}".into());
    }
    if !enabled {
        return ("disabled".into(), String::new(), "{}".into());
    }
    match call_json_op_timeout(
        state,
        id,
        SysOp::Status.as_str(),
        "{}",
        crate::state::PAGE_LIST_TIMEOUT,
    )
    .await
    {
        Ok(body) => (String::new(), String::new(), body),
        Err(e) => (String::new(), e.to_string(), "{}".into()),
    }
}

async fn load_docker_tabs(
    state: &AppState,
    id: &str,
    connected: bool,
    enabled: bool,
) -> (String, String, String, String, String, String, String) {
    if !connected {
        return (
            "offline".into(),
            String::new(),
            "[]".into(),
            "{}".into(),
            "[]".into(),
            "[]".into(),
            "[]".into(),
        );
    }
    if !enabled {
        return (
            "disabled".into(),
            String::new(),
            "[]".into(),
            "{}".into(),
            "[]".into(),
            "[]".into(),
            "[]".into(),
        );
    }
    let (containers_json, compose_json, images_json, volumes_json, networks_json, docker_error) =
        fetch_docker_bundle(state, id).await;
    (
        String::new(),
        docker_error,
        containers_json,
        compose_json,
        images_json,
        volumes_json,
        networks_json,
    )
}

async fn fetch_docker_bundle(
    state: &AppState,
    id: &str,
) -> (String, String, String, String, String, String) {
    let timeout = crate::state::PAGE_LIST_TIMEOUT;
    let deadline = tokio::time::Instant::now() + timeout;
    // container_list first so it does not share docker.sock with images/volumes
    // and the tab can render before those slower calls.
    let c = call_json_op_timeout(state, id, DockerOp::ContainerList.as_str(), "{}", timeout).await;
    let rest = deadline.saturating_duration_since(tokio::time::Instant::now());
    let rest = if rest.is_zero() {
        std::time::Duration::from_millis(50)
    } else {
        rest
    };
    let (p, i, v, n) = tokio::join!(
        call_json_op_timeout(state, id, DockerOp::ComposePs.as_str(), "{}", rest),
        call_json_op_timeout(state, id, DockerOp::ImageList.as_str(), "{}", rest),
        call_json_op_timeout(state, id, DockerOp::VolumeList.as_str(), "{}", rest),
        call_json_op_timeout(state, id, DockerOp::NetworkList.as_str(), "{}", rest),
    );
    let (containers_json, docker_error) = match c {
        Ok(body) => (attach_container_usage(state, id, body), String::new()),
        Err(e) => ("[]".into(), e.to_string()),
    };
    (
        containers_json,
        p.unwrap_or_else(|_| "{}".into()),
        i.unwrap_or_else(|_| "[]".into()),
        v.unwrap_or_else(|_| "[]".into()),
        n.unwrap_or_else(|_| "[]".into()),
        docker_error,
    )
}

fn attach_container_usage(state: &AppState, node_id: &str, raw: String) -> String {
    let Ok(mut rows) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) else {
        return raw;
    };
    let samples = state
        .stores
        .series
        .latest_samples(node_id)
        .unwrap_or_default();
    keystone_core::merge_container_usage(&mut rows, &samples);
    serde_json::to_string(&rows).unwrap_or(raw)
}

async fn container_usage_api(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if state.stores.metadata.get_node(&id).ok().flatten().is_none() {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    }
    let samples = state.stores.series.latest_samples(&id).unwrap_or_default();
    Json(keystone_core::container_usage_by_id(&samples)).into_response()
}

async fn call_json_op(
    state: &AppState,
    node_id: &str,
    op: &str,
    payload: &str,
) -> anyhow::Result<String> {
    call_json_op_timeout(
        state,
        node_id,
        op,
        payload,
        std::time::Duration::from_secs(180),
    )
    .await
}

async fn call_json_op_timeout(
    state: &AppState,
    node_id: &str,
    op: &str,
    payload: &str,
    timeout: std::time::Duration,
) -> anyhow::Result<String> {
    let result = state
        .agents
        .call_timeout(node_id, op, payload.to_string(), timeout)
        .await?;
    if !result.ok {
        anyhow::bail!("{}", result.error);
    }
    Ok(result.payload_json)
}

#[derive(Deserialize)]
struct DockerForm {
    payload: Option<String>,
    name: Option<String>,
    id: Option<String>,
    project: Option<String>,
    #[serde(default)]
    redirect: String,
}

fn docker_form_payload(form: &DockerForm) -> String {
    if let Some(p) = &form.payload {
        let t = p.trim();
        if t.starts_with('{') {
            return t.to_string();
        }
        if !t.is_empty() && form.name.is_none() {
            return serde_json::json!({ "name": t }).to_string();
        }
    }
    let mut map = serde_json::Map::new();
    if let Some(n) = &form.name {
        if !n.trim().is_empty() {
            map.insert("name".into(), serde_json::json!(n.trim()));
        }
    }
    if let Some(id) = &form.id {
        if !id.trim().is_empty() {
            map.insert("id".into(), serde_json::json!(id.trim()));
        }
    }
    if let Some(p) = &form.project {
        if !p.trim().is_empty() {
            map.insert("project".into(), serde_json::json!(p.trim()));
        }
    }
    serde_json::Value::Object(map).to_string()
}

fn panel_for_op(op: DockerOp) -> &'static str {
    match op {
        DockerOp::ImageList
        | DockerOp::ImageInspect
        | DockerOp::ImagePull
        | DockerOp::ImagePrune
        | DockerOp::ImageRemove => "images",
        DockerOp::VolumeList
        | DockerOp::VolumeInspect
        | DockerOp::VolumeCreate
        | DockerOp::VolumeRemove => "volumes",
        DockerOp::NetworkList
        | DockerOp::NetworkInspect
        | DockerOp::NetworkCreate
        | DockerOp::NetworkRemove => "networks",
        DockerOp::ComposePs
        | DockerOp::ComposeUp
        | DockerOp::ComposeDown
        | DockerOp::ComposeLogs
        | DockerOp::ComposePull
        | DockerOp::ComposeUpdate => "compose",
        _ => "containers",
    }
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
    if parsed.streams() {
        return (
            StatusCode::BAD_REQUEST,
            "logs are streamed from the logs page",
        )
            .into_response();
    }
    let payload = docker_form_payload(&form);
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
    if parsed.mutating() {
        let _ = state
            .stores
            .metadata
            .audit(&username, &id, parsed.as_str(), &target, ok, &detail);
    }
    let dest = if form.redirect.is_empty() {
        format!("/nodes/{id}?panel={}", panel_for_op(parsed))
    } else {
        form.redirect
    };
    Redirect::to(&dest).into_response()
}

#[derive(Deserialize)]
struct SysForm {
    payload: Option<String>,
    iface: Option<String>,
    method: Option<String>,
    address: Option<String>,
    prefix: Option<String>,
    gateway: Option<String>,
    dns: Option<String>,
    #[serde(default)]
    redirect: String,
}

fn sys_form_payload(form: &SysForm) -> String {
    if let Some(p) = &form.payload {
        let t = p.trim();
        if t.starts_with('{') {
            return t.to_string();
        }
    }
    let mut map = serde_json::Map::new();
    if let Some(iface) = &form.iface {
        map.insert("iface".into(), serde_json::json!(iface.trim()));
    }
    let method = form
        .method
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("dhcp");
    map.insert("method".into(), serde_json::json!(method));
    if let Some(a) = &form.address {
        map.insert("address".into(), serde_json::json!(a.trim()));
    }
    if let Some(p) = &form.prefix {
        let n: u8 = p.trim().parse().unwrap_or(0);
        map.insert("prefix".into(), serde_json::json!(n));
    }
    if let Some(g) = &form.gateway {
        map.insert("gateway".into(), serde_json::json!(g.trim()));
    }
    if let Some(d) = &form.dns {
        let dns: Vec<String> = d
            .split([',', ' ', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        map.insert("dns".into(), serde_json::json!(dns));
    }
    serde_json::Value::Object(map).to_string()
}

async fn sys_action(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((id, op)): Path<(String, String)>,
    Form(form): Form<SysForm>,
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
    let parsed = match op.parse::<SysOp>() {
        Ok(o) => o,
        Err(_) => return (StatusCode::BAD_REQUEST, "unknown op").into_response(),
    };
    if parsed.streams() {
        let dest = match parsed {
            SysOp::GitlabBackup => format!("/nodes/{id}/sys/gitlab-backup"),
            _ => format!("/nodes/{id}/sys/updates"),
        };
        return Redirect::to(&dest).into_response();
    }
    let payload = sys_form_payload(&form);
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
    if parsed.mutating() {
        let _ = state
            .stores
            .metadata
            .audit(&username, &id, parsed.as_str(), &target, ok, &detail);
    }
    let dest = if form.redirect.is_empty() {
        format!("/nodes/{id}?panel=system")
    } else {
        form.redirect
    };
    Redirect::to(&dest).into_response()
}

async fn sys_updates_page(Path(id): Path<String>) -> impl IntoResponse {
    Html(
        LogsTemplate {
            title: "System updates".into(),
            node_id: id.clone(),
            subtitle: "apt-get upgrade".into(),
            hint: "Streaming apt-get upgrade. Leave this page to stop following (the upgrade keeps running on the node).".into(),
            back_href: format!("/nodes/{id}?panel=system"),
            stream_url: format!("/nodes/{}/sys/updates/stream", urlencoding_path(&id)),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

async fn sys_updates_sse(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
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
    let _ = state
        .stores
        .metadata
        .audit(&username, &id, "updates_apply", "{}", true, "started");
    logs_sse_op(state, id, SysOp::UpdatesApply.as_str(), "{}".into())
}

async fn sys_gitlab_backup_page(Path(id): Path<String>) -> impl IntoResponse {
    Html(
        LogsTemplate {
            title: "GitLab backup".into(),
            node_id: id.clone(),
            subtitle: "gitlab-backup create".into(),
            hint: "Streaming GitLab Omnibus backup. Leave this page to stop following (the backup keeps running on the node). Copy /etc/gitlab next to the archive. Restore is not in this UI.".into(),
            back_href: format!("/nodes/{id}?panel=system"),
            stream_url: format!("/nodes/{}/sys/gitlab-backup/stream", urlencoding_path(&id)),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

async fn sys_gitlab_backup_sse(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
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
    let _ = state
        .stores
        .metadata
        .audit(&username, &id, "gitlab_backup", "{}", true, "started");
    logs_sse_op(state, id, SysOp::GitlabBackup.as_str(), "{}".into())
}

async fn sys_updates_api(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match call_json_op(&state, &id, SysOp::UpdatesList.as_str(), "{}").await {
        Ok(body) => {
            let val: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::json!({ "packages": [] }));
            axum::Json(val).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

fn logs_sse_op(state: AppState, node_id: String, op: &str, payload: String) -> Response {
    let (request_id, rx) = match state.agents.stream(&node_id, op, payload) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };
    let stream = LogSse {
        rx,
        cancel: Some(StreamCancel {
            agents: state.agents.clone(),
            node_id,
            request_id,
        }),
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[derive(Template)]
#[template(path = "logs.html")]
struct LogsTemplate {
    title: String,
    node_id: String,
    subtitle: String,
    hint: String,
    back_href: String,
    stream_url: String,
}

async fn container_logs_page(Path((id, cid)): Path<(String, String)>) -> impl IntoResponse {
    Html(
        LogsTemplate {
            title: "Container logs".into(),
            node_id: id.clone(),
            subtitle: cid.clone(),
            hint: "Live follow, last 200 lines. Leave this page to stop.".into(),
            back_href: format!("/nodes/{id}?panel=containers"),
            stream_url: format!(
                "/nodes/{}/containers/{}/logs/stream?follow=1",
                urlencoding_path(&id),
                urlencoding_path(&cid)
            ),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

async fn compose_logs_page(Path((id, project)): Path<(String, String)>) -> impl IntoResponse {
    Html(
        LogsTemplate {
            title: "Compose logs".into(),
            node_id: id.clone(),
            subtitle: project.clone(),
            hint: "Live follow, last 200 lines. Leave this page to stop.".into(),
            back_href: format!("/nodes/{id}?panel=compose"),
            stream_url: format!(
                "/nodes/{}/compose/{}/logs/stream?follow=1",
                urlencoding_path(&id),
                urlencoding_path(&project)
            ),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

fn urlencoding_path(s: &str) -> String {
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

#[derive(Deserialize)]
struct FollowQuery {
    follow: Option<String>,
}

fn follow_from_query(q: &FollowQuery) -> bool {
    match q.follow.as_deref().map(str::trim) {
        None | Some("") | Some("1") | Some("true") | Some("yes") => true,
        Some("0") | Some("false") | Some("no") => false,
        _ => true,
    }
}

async fn container_logs_sse(
    State(state): State<AppState>,
    Path((id, cid)): Path<(String, String)>,
    Query(q): Query<FollowQuery>,
) -> Response {
    let payload = serde_json::json!({
        "id": cid,
        "tail": "200",
        "follow": follow_from_query(&q),
    })
    .to_string();
    logs_sse(state, id, DockerOp::ContainerLogs, payload)
}

async fn compose_logs_sse(
    State(state): State<AppState>,
    Path((id, project)): Path<(String, String)>,
    Query(q): Query<FollowQuery>,
) -> Response {
    let payload = serde_json::json!({
        "project": project,
        "tail": "200",
        "follow": follow_from_query(&q),
    })
    .to_string();
    logs_sse(state, id, DockerOp::ComposeLogs, payload)
}

fn logs_sse(state: AppState, node_id: String, op: DockerOp, payload: String) -> Response {
    logs_sse_op(state, node_id, op.as_str(), payload)
}

struct StreamCancel {
    agents: crate::state::AgentRegistry,
    node_id: String,
    request_id: String,
}

impl Drop for StreamCancel {
    fn drop(&mut self) {
        self.agents.cancel_stream(&self.node_id, &self.request_id);
    }
}

struct LogSse {
    rx: mpsc::Receiver<StreamChunk>,
    cancel: Option<StreamCancel>,
}

impl Stream for LogSse {
    type Item = Result<Event, std::convert::Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.rx).poll_recv(cx) {
                Poll::Ready(Some(chunk)) => {
                    if chunk.eof {
                        self.cancel.take();
                        return Poll::Ready(Some(Ok(Event::default().event("done").data("eof"))));
                    }
                    if chunk.data.is_empty() {
                        continue;
                    }
                    let t = String::from_utf8_lossy(&chunk.data).into_owned();
                    let data = serde_json::json!({ "t": t }).to_string();
                    return Poll::Ready(Some(Ok(Event::default().data(data))));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

async fn container_stats_json(
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
            let val: serde_json::Value =
                serde_json::from_str(&r.payload_json).unwrap_or(serde_json::Value::Null);
            Json(val).into_response()
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
    let Some(sec) = help::section_by_slug(&slug) else {
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

#[derive(Serialize)]
struct CatalogMetric {
    name: String,
    metric_type: String,
    unit: String,
    help: String,
    labels: Vec<String>,
}

#[derive(Serialize)]
struct CatalogApi {
    metrics: Vec<CatalogMetric>,
}

async fn catalog_api() -> Json<CatalogApi> {
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

#[derive(Serialize)]
struct NodeDashboardApi {
    source: String,
    layout: serde_json::Value,
    widgets: serde_json::Value,
}

async fn dashboard_get(State(state): State<AppState>, Path(id): Path<String>) -> Response {
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

async fn dashboard_put(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let mut dash: Dashboard = match serde_json::from_value(body) {
        Ok(d) => d,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    dash.normalize();
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

async fn dashboard_delete(State(state): State<AppState>, Path(id): Path<String>) -> Response {
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

    #[test]
    fn docker_form_payload_prefers_json_then_fields() {
        let json = DockerForm {
            payload: Some(r#"{"name":"nginx"}"#.into()),
            name: None,
            id: None,
            project: None,
            redirect: String::new(),
        };
        assert_eq!(docker_form_payload(&json), r#"{"name":"nginx"}"#);
        let named = DockerForm {
            payload: None,
            name: Some(" data ".into()),
            id: None,
            project: None,
            redirect: String::new(),
        };
        assert_eq!(docker_form_payload(&named), r#"{"name":"data"}"#);
        let id = DockerForm {
            payload: None,
            name: None,
            id: Some("abc123".into()),
            project: None,
            redirect: String::new(),
        };
        assert_eq!(docker_form_payload(&id), r#"{"id":"abc123"}"#);
    }

    #[test]
    fn panel_for_op_groups_resources() {
        assert_eq!(panel_for_op(DockerOp::ImagePull), "images");
        assert_eq!(panel_for_op(DockerOp::VolumeCreate), "volumes");
        assert_eq!(panel_for_op(DockerOp::NetworkRemove), "networks");
        assert_eq!(panel_for_op(DockerOp::ComposeDown), "compose");
        assert_eq!(panel_for_op(DockerOp::ComposeUpdate), "compose");
        assert_eq!(panel_for_op(DockerOp::ContainerStart), "containers");
    }

    #[test]
    fn urlencoding_path_keeps_unreserved() {
        assert_eq!(urlencoding_path("abc-_.~XYZ"), "abc-_.~XYZ");
        assert_eq!(urlencoding_path("a b"), "a%20b");
    }

    #[test]
    fn relative_seen_buckets() {
        assert_eq!(relative_seen(0, true), "never");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert_eq!(relative_seen(now, false), "just now");
        assert_eq!(relative_seen(now - 12, false), "12s ago");
        assert_eq!(relative_seen(now - 120, false), "2m ago");
    }

    #[test]
    fn unix_rfc3339_is_utc() {
        let s = unix_rfc3339(0);
        assert!(
            s.starts_with("1970-01-01T00:00:00"),
            "audit tooltip must be UTC, got {s}"
        );
    }

    #[test]
    fn mutations_write_audit_lists_do_not() {
        let src = include_str!("http.rs");
        let docker = src
            .split("async fn docker_action")
            .nth(1)
            .expect("docker_action")
            .split("async fn sys_action")
            .next()
            .expect("docker_action body");
        assert!(
            docker.contains(".audit("),
            "Docker mutations must write the audit table"
        );
        assert!(
            docker.contains("parsed.mutating()"),
            "non-mutating Docker POSTs must not write audit"
        );

        let sys = src
            .split("async fn sys_action")
            .nth(1)
            .expect("sys_action")
            .split("async fn sys_updates_page")
            .next()
            .expect("sys_action body");
        assert!(
            sys.contains(".audit("),
            "System mutations must write the audit table"
        );
        assert!(
            sys.contains("parsed.mutating()"),
            "status and updates_list must not write audit"
        );

        let sse = src
            .split("async fn sys_updates_sse")
            .nth(1)
            .expect("sys_updates_sse")
            .split("async fn sys_gitlab_backup_page")
            .next()
            .expect("sys_updates_sse body");
        assert!(
            sse.contains(".audit(") && sse.contains("updates_apply"),
            "apt apply must write audit when the stream starts"
        );

        let gitlab = src
            .split("async fn sys_gitlab_backup_sse")
            .nth(1)
            .expect("sys_gitlab_backup_sse")
            .split("async fn sys_updates_api")
            .next()
            .expect("sys_gitlab_backup_sse body");
        assert!(
            gitlab.contains(".audit(") && gitlab.contains("gitlab_backup"),
            "GitLab backup must write audit when the stream starts"
        );
        assert!(
            src.split("async fn sys_action")
                .nth(1)
                .expect("sys_action")
                .split("async fn sys_updates_page")
                .next()
                .expect("sys_action body")
                .contains("/sys/gitlab-backup"),
            "streaming gitlab_backup must not redirect to apt apply"
        );
    }

    #[test]
    fn ingest_cannot_write_audit() {
        let ingest = include_str!("ingest.rs");
        assert!(
            !ingest.contains(".audit(") && !ingest.contains("recent_audit"),
            "gRPC ingest must not write the UI audit table"
        );
    }

    #[test]
    fn audit_page_limit_matches_docs() {
        assert_eq!(AUDIT_PAGE_LIMIT, 200);
        let src = include_str!("http.rs");
        assert!(src.contains("const AUDIT_PAGE_LIMIT: i64 = 200"));
        let op = include_str!("../../../docs/src/audit.md");
        assert!(
            op.contains("200"),
            "operator Audit chapter must match the cap"
        );
        let api = include_str!("../../../docs/dev/src/http-api.md");
        assert!(api.contains("`/audit`"), "HTTP API must list GET /audit");
        assert!(api.contains("200"), "HTTP API must match the row cap");
    }

    #[test]
    fn audit_ui_chrome() {
        let layout = include_str!("../templates/layout.html");
        assert!(
            layout.contains("href=\"/audit\""),
            "header must link to the mutation audit log"
        );
        let audit = include_str!("../templates/audit.html");
        assert!(
            audit.contains("No mutations yet"),
            "empty audit copy must say where rows come from"
        );
        assert!(
            audit.contains("tone-crit"),
            "failed mutations must use the crit chip"
        );
        let js = include_str!("static/app.js");
        assert!(
            js.contains("nav a[href='/audit']"),
            "welcome tour must point at header Audit"
        );
        let css = include_str!("static/app.css");
        assert!(
            css.contains(".chip.tone-crit"),
            "crit chips must stay coloured after audit CSS"
        );
        assert!(css.contains(".audit-detail"));
    }

    #[test]
    fn overview_page_and_widget_styles() {
        let js = include_str!("static/app.js");
        assert!(js.contains("function widgetStyle"), "per-card draw styles");
        assert!(
            js.contains("function setPage"),
            "page chrome while customizing"
        );
        assert!(js.contains("density-") && js.contains("cards-") && js.contains("accent-"));
        assert!(js.contains("gauge-bar"), "gauge bar style");
        assert!(js.contains("spark-fill"), "sparkline area fill");
        assert!(js.contains("function widgetIsEmpty"), "hide empty cards");
        assert!(js.contains("Card title"), "rename cards while customizing");
        assert!(js.contains("Hide empty"));
        assert!(js.contains("maxLength = 48"));
        let css = include_str!("static/app.css");
        assert!(css.contains(".widget-grid.density-compact"));
        assert!(css.contains(".widget-grid.density-spacious"));
        assert!(css.contains(".widget-grid.cards-flush"));
        assert!(css.contains(".widget-grid.cards-raised"));
        assert!(css.contains(".widget-grid.accent-green"));
        assert!(css.contains(".widget-grid.accent-amber"));
        assert!(css.contains(".widget-grid.accent-rose"));
        assert!(css.contains(".widget.style-compact"));
        assert!(css.contains(".gauge-bar"));
        assert!(css.contains("path.spark-fill"));
        assert!(
            css.contains(".chip.tone-crit"),
            "overview CSS must not drop crit chips"
        );
        let src = include_str!("http.rs");
        let put = src
            .split("async fn dashboard_put")
            .nth(1)
            .expect("dashboard_put")
            .split("async fn dashboard_delete")
            .next()
            .expect("dashboard_put body");
        assert!(
            put.contains(".normalize()"),
            "PUT must clamp page/style before validate"
        );
        let load = src
            .split("fn effective_dashboard")
            .nth(1)
            .expect("effective_dashboard")
            .split("fn node_settings")
            .next()
            .expect("effective_dashboard body");
        assert!(
            load.contains(".normalize()"),
            "loading a saved layout must clamp page/style before validate"
        );
    }

    #[test]
    fn ui_docker_posts_are_mutating_and_skip_exec() {
        let js = include_str!("static/app.js");
        let html = include_str!("../templates/node.html");
        for op in [
            "container_start",
            "container_stop",
            "container_restart",
            "container_kill",
            "container_remove",
            "compose_up",
            "compose_down",
            "compose_pull",
            "compose_update",
            "image_remove",
            "image_pull",
            "image_prune",
            "volume_create",
            "volume_remove",
            "network_create",
            "network_remove",
        ] {
            let parsed: DockerOp = op.parse().expect(op);
            assert!(
                parsed.mutating(),
                "{op} posted by the UI must be a mutation"
            );
            assert!(
                js.contains(op) || html.contains(&format!("docker/{op}")),
                "{op} must appear in the Docker UI"
            );
        }
        assert!(
            !js.contains("container_exec") && !html.contains("container_exec"),
            "interactive exec must stay out of this UI"
        );
        assert!(js.contains("/sys/net_set"));
        assert!(js.contains("/sys/updates"));
        assert!(js.contains("/sys/gitlab-backup"));
        assert!(SysOp::NetSet.mutating());
        assert!(SysOp::UpdatesApply.mutating());
        assert!(SysOp::GitlabBackup.mutating());
        assert!(!SysOp::Status.mutating());
        assert!(!SysOp::UpdatesList.mutating());
    }

    fn host_headers(host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, host.parse().unwrap());
        h
    }

    #[test]
    fn suggested_ingest_uses_grpc_port_not_ui_port() {
        let cfg = keystone_core::config::ServerConfig::default();
        let url = suggested_ingest_url(&host_headers("192.168.1.10:8080"), &cfg);
        assert_eq!(url, "http://192.168.1.10:9100");
        assert!(!url.contains("8080"));
    }

    #[test]
    fn add_node_snippet_is_mdns_plus_token_with_explicit_fallback() {
        let cfg = keystone_core::config::ServerConfig::default();
        let headers = host_headers("192.168.1.10:8080");
        assert_eq!(default_agent_ingest_url(&headers, &cfg), "mdns");
        let explicit = suggested_ingest_url(&headers, &cfg);
        let toml = agent_toml_snippet("mdns", &explicit, "s3cret-from-settings", "lab-pi");
        assert!(toml.contains("ingest_url = \"mdns\""));
        assert!(toml.contains("ingest_token = \"s3cret-from-settings\""));
        assert!(toml.contains("node_id = \"lab-pi\""));
        assert!(toml.contains("http://192.168.1.10:9100"));
        assert!(
            !toml.contains("8080"),
            "snippet must not send agents to the UI port"
        );
    }

    #[test]
    fn dockerhub_api_is_behind_the_session_cookie() {
        let src = include_str!("http.rs");
        let head = src.split("#[cfg(test)]").next().expect("router source");
        let authed_end = head.find(".layer(").expect("session layer");
        let search = head.find("/api/v1/dockerhub/search").expect("search route");
        let tags = head.find("/api/v1/dockerhub/tags").expect("tags route");
        assert!(search < authed_end, "Hub search must require a UI session");
        assert!(tags < authed_end, "Hub tags must require a UI session");
        assert!(
            !head[authed_end..].contains("/api/v1/dockerhub"),
            "Hub lookup must not be on the public router"
        );
        let usage = head
            .find("/api/v1/nodes/{id}/container-usage")
            .expect("container usage API");
        assert!(
            usage < authed_end,
            "container usage must require a UI session"
        );
        let docker_post = head
            .find("/nodes/{id}/docker/{op}")
            .expect("docker mutation POST");
        assert!(
            docker_post < authed_end,
            "Docker mutations must require a UI session"
        );
        let audit = head.find("\"/audit\"").expect("audit page");
        assert!(audit < authed_end, "audit log must require a UI session");
        assert!(
            !head[authed_end..].contains("\"/audit\""),
            "audit must not be on the public router"
        );
    }

    #[test]
    fn sys_routes_are_behind_the_session_cookie() {
        let src = include_str!("http.rs");
        let head = src.split("#[cfg(test)]").next().expect("router source");
        let authed_end = head.find(".layer(").expect("session layer");
        let post = head.find("/nodes/{id}/sys/{op}").expect("sys POST");
        let apply = head.find("/nodes/{id}/sys/updates").expect("sys apply");
        let backup = head
            .find("/nodes/{id}/sys/gitlab-backup")
            .expect("gitlab backup page");
        let api = head
            .find("/api/v1/nodes/{id}/sys/updates")
            .expect("sys updates API");
        assert!(post < authed_end);
        assert!(apply < authed_end);
        assert!(backup < authed_end);
        assert!(api < authed_end);
        let public = head[authed_end..]
            .split("async fn health()")
            .next()
            .expect("public router");
        assert!(
            !public.contains("/nodes/{id}/sys"),
            "sys routes must not be on the public router"
        );
        assert!(
            !public.contains("/api/v1/nodes/{id}/sys"),
            "sys JSON must not be on the public router"
        );
    }

    #[test]
    fn sys_form_payload_static_ipv4() {
        let form = SysForm {
            payload: None,
            iface: Some("eth0".into()),
            method: Some("static".into()),
            address: Some("192.168.0.50".into()),
            prefix: Some("24".into()),
            gateway: Some("192.168.0.1".into()),
            dns: Some("1.1.1.1 8.8.8.8".into()),
            redirect: String::new(),
        };
        let p = sys_form_payload(&form);
        assert!(p.contains("\"iface\":\"eth0\""));
        assert!(p.contains("\"method\":\"static\""));
        assert!(p.contains("192.168.0.50"));
        assert!(!p.contains(';'));
    }

    #[test]
    fn system_ui_confirms_host_mutations() {
        let js = include_str!("static/app.js");
        assert!(js.contains("paintSystem"), "System tab painter missing");
        assert!(
            js.contains("Apply pending apt upgrades"),
            "Apply updates must ask before apt-get upgrade"
        );
        assert!(
            js.contains("Backup GitLab"),
            "System tab must offer GitLab Omnibus backup when detected"
        );
        assert!(
            js.contains("Create a GitLab Omnibus backup"),
            "GitLab backup must ask before gitlab-backup create"
        );
        assert!(
            js.contains("Change IPv4 on this node"),
            "net_set must ask before changing addressing"
        );
        assert!(
            js.contains("ethernetIface"),
            "IPv4 form must skip Wi-Fi/docker NICs"
        );
        assert!(
            js.contains("compose_update"),
            "Compose Update (pull then up) missing from the Docker toolbar"
        );
        assert!(
            js.contains("Observe host updates"),
            "System observe-off must point at Settings, not only the socket unit"
        );
        assert!(
            js.contains("restart keystone-agent"),
            "helper-down copy must cover ProtectSystem until the agent is restarted"
        );
        assert!(
            js.contains("formatCpuRatio") && js.contains("CPU"),
            "Containers tab must show per-container CPU from pushed samples"
        );
        assert!(
            js.contains("container-usage"),
            "Containers tab must poll /api/v1/nodes/{{id}}/container-usage"
        );
        let css = include_str!("static/app.css");
        assert!(
            css.contains("select"),
            "System IPv4 <select> must share the dark input style"
        );
    }

    #[test]
    fn node_page_does_not_wait_for_sys_before_docker() {
        let src = include_str!("http.rs");
        let page = src
            .split("async fn node_page")
            .nth(1)
            .expect("node_page")
            .split("async fn node_settings_post")
            .next()
            .expect("node_page body");
        assert!(
            page.contains("tokio::join!"),
            "sys status must not delay Docker lists on the node page"
        );
        assert!(
            page.contains("load_docker_tabs") && page.contains("load_sys_tab"),
            "Docker and System tabs must load together"
        );
    }

    #[test]
    fn docker_tab_lists_containers_before_other_engine_calls() {
        let src = include_str!("http.rs");
        let fn_src = src
            .split("async fn fetch_docker_bundle")
            .nth(1)
            .expect("fetch_docker_bundle")
            .split("fn attach_container_usage")
            .next()
            .expect("bundle body");
        let list_at = fn_src.find("ContainerList").expect("container_list");
        let join_at = fn_src.find("tokio::join!").expect("remaining lists");
        assert!(
            list_at < join_at,
            "container_list must not share docker.sock with images/volumes on page load"
        );
    }

    #[test]
    fn ingest_tls_snippet_is_https_not_mdns() {
        let cfg = keystone_core::config::ServerConfig {
            tls: keystone_core::config::TlsConfig {
                cert_file: "/c.pem".into(),
                key_file: "/k.pem".into(),
                ingest: true,
            },
            ..keystone_core::config::ServerConfig::default()
        };
        let headers = host_headers("keystone.home.arpa:8080");
        let url = default_agent_ingest_url(&headers, &cfg);
        assert_eq!(url, "https://keystone.home.arpa:9100");
        let toml = agent_toml_snippet(&url, &url, "tok", "n1");
        assert!(toml.contains("ingest_url = \"https://keystone.home.arpa:9100\""));
        assert!(toml.contains("tls_ca_file"));
        assert!(!toml.contains("ingest_url = \"mdns\""));
    }
}
