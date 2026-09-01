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
use keystone_core::docker::{
    audit_docker_target, docker_ref_ok, summarize_container_inspect, DockerOp,
};
use keystone_core::fleet::{fleet_chips, FleetChip};
use keystone_core::metrics::catalog;
use keystone_core::sys::{
    audit_sys_target, journal_unit, parse_password_auth, parse_restore_backup, validate_wifi_iface,
    SysOp,
};
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
/// Idle lifetime for a finished login. Each UI hit slides this window so
/// an open dashboard stays signed in; a copied cookie dies after this
/// much quiet. Pending 2FA stays on [`PENDING_2FA_SECS`].
const SESSION_IDLE_SECS: i64 = 2 * 60 * 60;
/// Do not rewrite SQLite/Set-Cookie on every 1s fleet poll.
const SESSION_TOUCH_EVERY_SECS: i64 = 10 * 60;
const PENDING_2FA_SECS: i64 = 5 * 60;

fn forwarded_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.eq_ignore_ascii_case("https"))
}

fn cookie_secure(headers: &HeaderMap, ui_https: bool) -> bool {
    ui_https || forwarded_https(headers)
}

fn session_cookie(id: String, secure: bool, max_age_secs: Option<i64>) -> Cookie<'static> {
    let mut cookie = Cookie::new(SESSION_COOKIE, id);
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    if let Some(secs) = max_age_secs {
        cookie.set_max_age(cookie::time::Duration::seconds(secs));
    }
    if secure {
        cookie.set_secure(true);
    }
    cookie
}

fn session_needs_touch(expires_unix: i64, now: i64) -> bool {
    expires_unix.saturating_sub(now) <= SESSION_IDLE_SECS - SESSION_TOUCH_EVERY_SECS
}

fn attach_session_cookie(response: &mut Response, id: String, secure: bool) {
    let cookie = session_cookie(id, secure, None);
    if let Ok(value) = header::HeaderValue::from_str(&cookie.to_string()) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
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
        .route("/nodes/{id}/sys/autoremove", get(sys_autoremove_page))
        .route("/nodes/{id}/sys/autoremove/stream", get(sys_autoremove_sse))
        .route("/nodes/{id}/sys/gitlab-backup", get(sys_gitlab_backup_page))
        .route(
            "/nodes/{id}/sys/gitlab-backup/stream",
            get(sys_gitlab_backup_sse),
        )
        .route(
            "/nodes/{id}/sys/gitlab-restore",
            get(sys_gitlab_restore_page),
        )
        .route(
            "/nodes/{id}/sys/gitlab-restore/stream",
            get(sys_gitlab_restore_sse),
        )
        .route("/nodes/{id}/sys/journal/{unit}", get(sys_journal_page))
        .route(
            "/nodes/{id}/sys/journal/{unit}/stream",
            get(sys_journal_sse),
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
        .route("/api/v1/session", get(session_api))
        .route("/api/v1/alerts", get(alerts_api))
        .route("/api/v1/nodes", get(nodes_api))
        .route("/api/v1/dockerhub/search", get(crate::dockerhub::search))
        .route("/api/v1/dockerhub/tags", get(crate::dockerhub::tags))
        .route("/api/v1/nodes/{id}/sys/updates", get(sys_updates_api))
        .route("/api/v1/nodes/{id}/sys/wifi", get(sys_wifi_api))
        .route(
            "/api/v1/nodes/{id}/container-usage",
            get(container_usage_api),
        )
        .route(
            "/api/v1/nodes/{id}/containers/{cid}",
            get(container_inspect_api),
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
        .route("/static/logo.svg", get(logo))
        .merge(authed)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn session_api() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
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

async fn logo() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/svg+xml")],
        include_bytes!("static/logo.svg").as_slice(),
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
                return next.run(request).await;
            }
            if state
                .stores
                .metadata
                .user_must_change_password(&sess.username)
                .unwrap_or(false)
                && path != "/password"
                && path != "/logout"
            {
                return Redirect::to("/password").into_response();
            }
            let slide = path != "/logout" && session_needs_touch(sess.expires_unix, expiry_unix(0));
            let sid = sess.id.clone();
            let secure = cookie_secure(request.headers(), state.config.tls.ui_https());
            let mut response = next.run(request).await;
            if slide
                && state
                    .stores
                    .metadata
                    .touch_session(&sid, expiry_unix(SESSION_IDLE_SECS))
                    .unwrap_or(false)
            {
                attach_session_cookie(&mut response, sid, secure);
            }
            response
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
    let (expires, pending, next, cookie_age) = if totp_on {
        (
            expiry_unix(PENDING_2FA_SECS),
            true,
            "/login/totp",
            Some(PENDING_2FA_SECS),
        )
    } else if state
        .stores
        .metadata
        .user_must_change_password(&form.username)
        .unwrap_or(false)
    {
        (expiry_unix(SESSION_IDLE_SECS), false, "/password", None)
    } else {
        (expiry_unix(SESSION_IDLE_SECS), false, "/", None)
    };
    let _ = state
        .stores
        .metadata
        .put_session(&sid, &form.username, expires, pending);
    if !pending {
        state.login_gate.lock().clear(&form.username);
    }
    (
        jar.add(session_cookie(
            sid,
            cookie_secure(&headers, state.config.tls.ui_https()),
            cookie_age,
        )),
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
    let _ = state.stores.metadata.put_session(
        &sid,
        &sess.username,
        expiry_unix(SESSION_IDLE_SECS),
        false,
    );
    let mut old = Cookie::from(SESSION_COOKIE);
    old.set_path("/");
    (
        jar.remove(old).add(session_cookie(
            sid,
            cookie_secure(&headers, state.config.tls.ui_https()),
            None,
        )),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepUpError {
    Locked,
    Denied,
}

/// When `needs` and TOTP is on: current 6-digit code, one window, no
/// backup codes. TOTP off (or `needs` false) is confirm-only.
fn consume_step_up(
    state: &AppState,
    username: &str,
    needs: bool,
    code: &str,
) -> Result<(), StepUpError> {
    if !needs {
        return Ok(());
    }
    if username == "unknown" {
        return Err(StepUpError::Denied);
    }
    let Some(mut rec) = state.stores.metadata.user_totp(username).ok().flatten() else {
        return Ok(());
    };
    if !rec.enabled {
        return Ok(());
    }
    if state.login_gate.lock().locked(username) {
        return Err(StepUpError::Locked);
    }
    if let Some(step) = totp::verify_code_step(&rec.secret, username, code, Some(rec.last_step)) {
        rec.last_step = step;
        let _ = state.stores.metadata.set_user_totp(username, &rec);
        state.login_gate.lock().clear(username);
        return Ok(());
    }
    if totp::normalize_totp(code).is_some() {
        state.login_gate.lock().record_fail(username);
    }
    Err(StepUpError::Denied)
}

fn step_up_denied(
    state: &AppState,
    username: &str,
    node_id: &str,
    op: &str,
    target: &str,
    mutating: bool,
    err: StepUpError,
    panel: &str,
) -> Response {
    if mutating {
        let detail = match err {
            StepUpError::Locked => "too many authenticator attempts",
            StepUpError::Denied => "authenticator code required",
        };
        let _ = state
            .stores
            .metadata
            .audit(username, node_id, op, target, false, detail);
    }
    let q = match err {
        StepUpError::Locked => "step-up-locked",
        StepUpError::Denied => "step-up",
    };
    Redirect::to(&format!(
        "/nodes/{}?panel={panel}&err={q}",
        urlencoding_path(node_id)
    ))
    .into_response()
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
        Some("totp-pw") => {
            "that password did not match; authenticator setup was not started".into()
        }
        Some("totp") => {
            "password or authenticator/backup code did not match; 2FA was not disabled".into()
        }
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
        return Redirect::to("/settings?err=totp-pw").into_response();
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

/// Compare node id / hostname to this UI process, ignoring `.local`.
fn host_token_eq(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        s.trim()
            .trim_end_matches('.')
            .trim_end_matches(".local")
            .to_ascii_lowercase()
    }
    let a = norm(a);
    let b = norm(b);
    !a.is_empty() && a == b
}

fn node_is_this_ui_host(node_id: &str, hostname: &str) -> bool {
    let me = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default();
    host_token_eq(&me, node_id) || host_token_eq(&me, hostname)
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
    sys_ui_host: bool,
    totp_enabled: bool,
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
    let sys_ui_host = node_is_this_ui_host(&id, &node.hostname);
    let totp_enabled = state
        .stores
        .metadata
        .user_totp_enabled(&state.config.auth.username)
        .unwrap_or(false);
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
            sys_ui_host,
            totp_enabled,
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

async fn container_inspect_api(
    State(state): State<AppState>,
    Path((id, cid)): Path<(String, String)>,
) -> Response {
    if state.stores.metadata.get_node(&id).ok().flatten().is_none() {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    }
    if !docker_ref_ok(&cid) {
        return (StatusCode::BAD_REQUEST, "unknown container").into_response();
    }
    let payload = serde_json::json!({ "id": cid }).to_string();
    match call_json_op(&state, &id, DockerOp::ContainerInspect.as_str(), &payload).await {
        Ok(body) => {
            let raw: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            Json(summarize_container_inspect(&raw)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
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
    registry: Option<String>,
    username: Option<String>,
    password: Option<String>,
    #[serde(default)]
    totp: String,
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
    if let Some(r) = &form.registry {
        if !r.trim().is_empty() {
            map.insert("registry".into(), serde_json::json!(r.trim()));
        }
    }
    if let Some(u) = &form.username {
        if !u.trim().is_empty() {
            map.insert("username".into(), serde_json::json!(u.trim()));
        }
    }
    if let Some(p) = &form.password {
        if !p.is_empty() {
            map.insert("password".into(), serde_json::json!(p));
        }
    }
    serde_json::Value::Object(map).to_string()
}

fn panel_for_op(op: DockerOp) -> &'static str {
    match op {
        DockerOp::ImageList
        | DockerOp::ImageInspect
        | DockerOp::ImagePull
        | DockerOp::ImageLogin
        | DockerOp::ImagePrune
        | DockerOp::ImageRemove => "images",
        DockerOp::VolumeList
        | DockerOp::VolumeInspect
        | DockerOp::VolumeCreate
        | DockerOp::VolumeRemove
        | DockerOp::VolumePrune => "volumes",
        DockerOp::NetworkList
        | DockerOp::NetworkInspect
        | DockerOp::NetworkCreate
        | DockerOp::NetworkRemove
        | DockerOp::NetworkPrune => "networks",
        DockerOp::ComposePs
        | DockerOp::ComposeUp
        | DockerOp::ComposeStop
        | DockerOp::ComposeStart
        | DockerOp::ComposeRestart
        | DockerOp::ComposeDown
        | DockerOp::ComposeLogs
        | DockerOp::ComposePull
        | DockerOp::ComposeUpdate => "compose",
        DockerOp::ContainerList
        | DockerOp::ContainerInspect
        | DockerOp::ContainerStart
        | DockerOp::ContainerStop
        | DockerOp::ContainerRestart
        | DockerOp::ContainerKill
        | DockerOp::ContainerRemove
        | DockerOp::ContainerPause
        | DockerOp::ContainerUnpause
        | DockerOp::ContainerPrune
        | DockerOp::ContainerLogs
        | DockerOp::ContainerStats
        | DockerOp::ContainerExec => "containers",
    }
}

async fn docker_action(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((id, op)): Path<(String, String)>,
    Form(form): Form<DockerForm>,
) -> Response {
    let username = session_username(&state, &jar).unwrap_or_else(|| "unknown".into());
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
    let target = audit_docker_target(parsed, &payload);
    if let Err(err) = consume_step_up(&state, &username, parsed.needs_step_up(), &form.totp) {
        return step_up_denied(
            &state,
            &username,
            &id,
            parsed.as_str(),
            &target,
            parsed.mutating(),
            err,
            panel_for_op(parsed),
        );
    }
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
    ipv6_method: Option<String>,
    ipv6_address: Option<String>,
    ipv6_prefix: Option<String>,
    ipv6_gateway: Option<String>,
    ipv6_dns: Option<String>,
    unit: Option<String>,
    name: Option<String>,
    vlan: Option<String>,
    ssid: Option<String>,
    psk: Option<String>,
    password_auth: Option<String>,
    #[serde(default)]
    totp: String,
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
    let ipv6_method = form
        .ipv6_method
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto");
    map.insert("ipv6_method".into(), serde_json::json!(ipv6_method));
    if let Some(a) = &form.ipv6_address {
        map.insert("ipv6_address".into(), serde_json::json!(a.trim()));
    }
    if let Some(p) = &form.ipv6_prefix {
        let n: u8 = p.trim().parse().unwrap_or(0);
        map.insert("ipv6_prefix".into(), serde_json::json!(n));
    }
    if let Some(g) = &form.ipv6_gateway {
        map.insert("ipv6_gateway".into(), serde_json::json!(g.trim()));
    }
    if let Some(d) = &form.ipv6_dns {
        let dns: Vec<String> = d
            .split([',', ' ', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        map.insert("ipv6_dns".into(), serde_json::json!(dns));
    }
    if let Some(unit) = &form.unit {
        map.insert("unit".into(), serde_json::json!(unit.trim()));
    }
    if let Some(name) = &form.name {
        map.insert("name".into(), serde_json::json!(name.trim()));
    }
    if let Some(v) = &form.vlan {
        let n: u16 = v.trim().parse().unwrap_or(0);
        map.insert("vlan".into(), serde_json::json!(n));
    }
    if let Some(s) = &form.ssid {
        map.insert("ssid".into(), serde_json::json!(s.trim()));
    }
    if let Some(p) = &form.psk {
        map.insert("psk".into(), serde_json::json!(p.trim()));
    }
    if let Some(p) = &form.password_auth {
        if let Ok(v) = parse_password_auth(p) {
            map.insert("password_auth".into(), serde_json::json!(v));
        }
    }
    serde_json::Value::Object(map).to_string()
}

async fn sys_action(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((id, op)): Path<(String, String)>,
    Form(form): Form<SysForm>,
) -> Response {
    let username = session_username(&state, &jar).unwrap_or_else(|| "unknown".into());
    let parsed = match op.parse::<SysOp>() {
        Ok(o) => o,
        Err(_) => return (StatusCode::BAD_REQUEST, "unknown op").into_response(),
    };
    if parsed.streams() {
        if parsed.needs_step_up() {
            let armed = sys_form_payload(&form);
            let target = armed.clone();
            if parsed == SysOp::GitlabRestore && parse_restore_backup(&armed).is_err() {
                return (StatusCode::BAD_REQUEST, "invalid backup name").into_response();
            }
            if let Err(err) = consume_step_up(&state, &username, true, &form.totp) {
                return step_up_denied(
                    &state,
                    &username,
                    &id,
                    parsed.as_str(),
                    &target,
                    parsed.mutating(),
                    err,
                    "system",
                );
            }
            state
                .stream_arms
                .lock()
                .arm(&username, &id, parsed.as_str(), armed);
            let dest = match parsed {
                SysOp::GitlabRestore => format!("/nodes/{id}/sys/gitlab-restore"),
                _ => format!("/nodes/{id}?panel=system"),
            };
            return Redirect::to(&dest).into_response();
        }
        let dest = match parsed {
            SysOp::GitlabBackup => format!("/nodes/{id}/sys/gitlab-backup"),
            SysOp::Journal => format!("/nodes/{id}?panel=system"),
            SysOp::UpdatesAutoremove => format!("/nodes/{id}/sys/autoremove"),
            _ => format!("/nodes/{id}/sys/updates"),
        };
        return Redirect::to(&dest).into_response();
    }
    let payload = sys_form_payload(&form);
    let target = audit_sys_target(parsed, &payload);
    if let Err(err) = consume_step_up(&state, &username, parsed.needs_step_up(), &form.totp) {
        return step_up_denied(
            &state,
            &username,
            &id,
            parsed.as_str(),
            &target,
            parsed.mutating(),
            err,
            "system",
        );
    }
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

async fn sys_autoremove_page(Path(id): Path<String>) -> impl IntoResponse {
    Html(
        LogsTemplate {
            title: "Autoremove".into(),
            node_id: id.clone(),
            subtitle: "apt-get autoremove".into(),
            hint: "Streaming apt-get autoremove. Leave this page to stop following (the command keeps running on the node). This is not dist-upgrade.".into(),
            back_href: format!("/nodes/{id}?panel=system"),
            stream_url: format!("/nodes/{}/sys/autoremove/stream", urlencoding_path(&id)),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

async fn sys_autoremove_sse(
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
    let _ =
        state
            .stores
            .metadata
            .audit(&username, &id, "updates_autoremove", "{}", true, "started");
    logs_sse_op(state, id, SysOp::UpdatesAutoremove.as_str(), "{}".into())
}

async fn sys_gitlab_backup_page(Path(id): Path<String>) -> impl IntoResponse {
    Html(
        LogsTemplate {
            title: "GitLab backup".into(),
            node_id: id.clone(),
            subtitle: "gitlab-backup create".into(),
            hint: "Streaming GitLab Omnibus backup. Leave this page to stop following (the backup keeps running on the node). Copy /etc/gitlab next to the archive — it is not in the tar.".into(),
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

async fn sys_gitlab_restore_page(Path(id): Path<String>) -> impl IntoResponse {
    Html(
        LogsTemplate {
            title: "GitLab restore".into(),
            node_id: id.clone(),
            subtitle: "gitlab-backup restore".into(),
            hint: "Streaming GitLab Omnibus restore. Leaving this page only stops following — the restore keeps running on the node. This replaces GitLab data. /etc/gitlab is not in the tar.".into(),
            back_href: format!("/nodes/{id}?panel=system"),
            stream_url: format!("/nodes/{}/sys/gitlab-restore/stream", urlencoding_path(&id)),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
}

async fn sys_gitlab_restore_sse(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Response {
    let username = session_username(&state, &jar).unwrap_or_else(|| "unknown".into());
    let Some(arm) = state
        .stream_arms
        .lock()
        .take(&username, &id, SysOp::GitlabRestore.as_str())
    else {
        return (StatusCode::FORBIDDEN, "restore was not confirmed").into_response();
    };
    let _ = state.stores.metadata.audit(
        &username,
        &id,
        "gitlab_restore",
        &arm.payload_json,
        true,
        "started",
    );
    logs_sse_op(state, id, SysOp::GitlabRestore.as_str(), arm.payload_json)
}

async fn sys_journal_page(Path((id, unit)): Path<(String, String)>) -> Response {
    let Ok(unit) = journal_unit(&unit) else {
        return (StatusCode::BAD_REQUEST, "unknown journal unit").into_response();
    };
    Html(
        LogsTemplate {
            title: "Journal".into(),
            node_id: id.clone(),
            subtitle: unit.to_string(),
            hint: "Live follow, last 200 lines. Leave this page to stop. Not a shell.".into(),
            back_href: format!("/nodes/{id}?panel=system"),
            stream_url: format!(
                "/nodes/{}/sys/journal/{}/stream",
                urlencoding_path(&id),
                urlencoding_path(unit)
            ),
        }
        .render()
        .unwrap_or_else(|e| e.to_string()),
    )
    .into_response()
}

async fn sys_journal_sse(
    State(state): State<AppState>,
    Path((id, unit)): Path<(String, String)>,
) -> Response {
    let Ok(unit) = journal_unit(&unit) else {
        return (StatusCode::BAD_REQUEST, "unknown journal unit").into_response();
    };
    let payload = serde_json::json!({ "unit": unit }).to_string();
    logs_sse_op(state, id, SysOp::Journal.as_str(), payload)
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

#[derive(Deserialize)]
struct WifiScanQuery {
    iface: String,
}

async fn sys_wifi_api(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<WifiScanQuery>,
) -> Response {
    let Ok(iface) = validate_wifi_iface(&q.iface) else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "invalid interface" })),
        )
            .into_response();
    };
    let payload = serde_json::json!({ "iface": iface }).to_string();
    match call_json_op(&state, &id, SysOp::WifiScan.as_str(), &payload).await {
        Ok(body) => {
            let val: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::json!({ "ssids": [] }));
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
    fn ui_host_token_matches_mdns_suffix() {
        assert!(host_token_eq("ranga", "ranga.local"));
        assert!(host_token_eq("Ranga", "ranga"));
        assert!(!host_token_eq("ranga", "pi"));
        assert!(!host_token_eq("", "ranga"));
        let me = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_default();
        if me.is_empty() {
            return;
        }
        assert!(node_is_this_ui_host(&me, "other-node"));
        assert!(node_is_this_ui_host("other-id", &me));
        assert!(!node_is_this_ui_host(
            "definitely-not-this-ui-host",
            "also-not-this-ui-host"
        ));
    }

    #[test]
    fn docker_form_payload_prefers_json_then_fields() {
        let json = DockerForm {
            payload: Some(r#"{"name":"nginx"}"#.into()),
            name: None,
            id: None,
            project: None,
            registry: None,
            username: None,
            password: None,
            totp: "123456".into(),
            redirect: String::new(),
        };
        assert_eq!(docker_form_payload(&json), r#"{"name":"nginx"}"#);
        assert!(
            !docker_form_payload(&json).contains("123456"),
            "authenticator code must not go to the Docker payload"
        );
        let named = DockerForm {
            payload: None,
            name: Some(" data ".into()),
            id: None,
            project: None,
            registry: None,
            username: None,
            password: None,
            totp: String::new(),
            redirect: String::new(),
        };
        assert_eq!(docker_form_payload(&named), r#"{"name":"data"}"#);
        let id = DockerForm {
            payload: None,
            name: None,
            id: Some("abc123".into()),
            project: None,
            registry: None,
            username: None,
            password: None,
            totp: String::new(),
            redirect: String::new(),
        };
        assert_eq!(docker_form_payload(&id), r#"{"id":"abc123"}"#);
        let login = DockerForm {
            payload: None,
            name: None,
            id: None,
            project: None,
            registry: Some(" ghcr.io ".into()),
            username: Some(" alice ".into()),
            password: Some("ghp_notarealtoken".into()),
            totp: String::new(),
            redirect: String::new(),
        };
        let p = docker_form_payload(&login);
        assert!(p.contains("\"registry\":\"ghcr.io\""));
        assert!(p.contains("alice"));
        assert!(p.contains("ghp_notarealtoken"));
        let redacted = keystone_core::docker::audit_docker_target(DockerOp::ImageLogin, &p);
        assert!(!redacted.contains("ghp_notarealtoken"));
        assert!(redacted.contains("alice"));
    }

    #[test]
    fn panel_for_op_groups_resources() {
        for op in DockerOp::all() {
            let panel = panel_for_op(op);
            let name = op.as_str();
            if name.starts_with("compose_") {
                assert_eq!(panel, "compose", "{name}");
            } else if name.starts_with("image_") {
                assert_eq!(panel, "images", "{name}");
            } else if name.starts_with("volume_") {
                assert_eq!(panel, "volumes", "{name}");
            } else if name.starts_with("network_") {
                assert_eq!(panel, "networks", "{name}");
            } else {
                assert_eq!(panel, "containers", "{name}");
            }
        }
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
            docker.contains("audit_docker_target"),
            "registry login password must not be the audit target"
        );
        assert!(
            docker.contains("parsed.mutating()"),
            "non-mutating Docker POSTs must not write audit"
        );
        assert!(
            docker.contains("consume_step_up") && docker.contains("needs_step_up"),
            "Docker POSTs must share step-up with System"
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
        assert!(
            sys.contains("consume_step_up") && sys.contains("needs_step_up"),
            "System POSTs must share step-up with Docker"
        );
        assert!(
            sys.contains("audit_sys_target"),
            "Wi-Fi join must not audit the PSK"
        );

        let sse = src
            .split("async fn sys_updates_sse")
            .nth(1)
            .expect("sys_updates_sse")
            .split("async fn sys_autoremove_page")
            .next()
            .expect("sys_updates_sse body");
        assert!(
            sse.contains(".audit(") && sse.contains("updates_apply"),
            "apt apply must write audit when the stream starts"
        );

        let autoremove = src
            .split("async fn sys_autoremove_sse")
            .nth(1)
            .expect("sys_autoremove_sse")
            .split("async fn sys_gitlab_backup_page")
            .next()
            .expect("sys_autoremove_sse body");
        assert!(
            autoremove.contains(".audit(") && autoremove.contains("updates_autoremove"),
            "autoremove must write audit when the stream starts"
        );

        let gitlab = src
            .split("async fn sys_gitlab_backup_sse")
            .nth(1)
            .expect("sys_gitlab_backup_sse")
            .split("async fn sys_gitlab_restore_page")
            .next()
            .expect("sys_gitlab_backup_sse body");
        assert!(
            gitlab.contains(".audit(") && gitlab.contains("gitlab_backup"),
            "GitLab backup must write audit when the stream starts"
        );
        let restore = src
            .split("async fn sys_gitlab_restore_sse")
            .nth(1)
            .expect("sys_gitlab_restore_sse")
            .split("async fn sys_journal_page")
            .next()
            .expect("sys_gitlab_restore_sse body");
        assert!(
            restore.contains("stream_arms")
                && restore.contains(".take(")
                && restore.contains(".audit(")
                && restore.contains("gitlab_restore"),
            "GitLab restore SSE must consume the step-up ticket then audit started"
        );
        assert!(
            restore.contains("FORBIDDEN") || restore.contains("restore was not confirmed"),
            "restore stream without a ticket must not start gitlab-backup restore"
        );
        let journal = src
            .split("async fn sys_journal_sse")
            .nth(1)
            .expect("sys_journal_sse")
            .split("async fn sys_updates_api")
            .next()
            .expect("sys_journal_sse body");
        assert!(
            !journal.contains(".audit("),
            "journal follow is observe and must not write audit"
        );
        let streams = src
            .split("async fn sys_action")
            .nth(1)
            .expect("sys_action")
            .split("async fn sys_updates_page")
            .next()
            .expect("sys_action body")
            .split("if parsed.streams()")
            .nth(1)
            .expect("sys streams")
            .split("let payload = sys_form_payload")
            .next()
            .expect("sys streams body");
        assert!(
            streams.contains("/sys/gitlab-backup"),
            "streaming gitlab_backup must not redirect to apt apply"
        );
        assert!(
            streams.contains("stream_arms") && streams.contains("gitlab-restore"),
            "gitlab_restore POST must arm a ticket then go to the follow page"
        );
        assert!(
            streams.contains("consume_step_up"),
            "streaming restore must not start SSE until step-up is accepted"
        );
        let journal_arm = streams
            .split("SysOp::Journal")
            .nth(1)
            .expect("Journal arm")
            .split("=>")
            .nth(1)
            .expect("Journal dest")
            .split(',')
            .next()
            .expect("Journal dest expr");
        assert!(
            journal_arm.contains("?panel=system"),
            "journal POST must return to the System tab, got {journal_arm}"
        );
        assert!(
            !journal_arm.contains("/sys/updates"),
            "journal must not redirect to apt apply, got {journal_arm}"
        );
        let autoremove_arm = streams
            .split("SysOp::UpdatesAutoremove")
            .nth(1)
            .expect("UpdatesAutoremove arm")
            .split("=>")
            .nth(1)
            .expect("UpdatesAutoremove dest")
            .split(',')
            .next()
            .expect("UpdatesAutoremove dest expr");
        assert!(
            autoremove_arm.contains("/sys/autoremove"),
            "autoremove POST must go to the follow page, got {autoremove_arm}"
        );
        assert!(
            !autoremove_arm.contains("/sys/updates"),
            "autoremove must not redirect to apt apply, got {autoremove_arm}"
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
    fn totp_setup_password_mismatch_is_not_a_code_error() {
        assert!(
            settings_err_message(Some("totp-pw")).contains("setup was not started"),
            "enroll must not blame an authenticator code"
        );
        assert!(
            !settings_err_message(Some("totp-pw")).contains("code"),
            "setup only asked for the password"
        );
        assert!(settings_err_message(Some("totp")).contains("disabled"));
        let start = include_str!("http.rs")
            .split("async fn totp_start_post")
            .nth(1)
            .expect("totp_start_post")
            .split("async fn totp_confirm_post")
            .next()
            .expect("totp_start_post body");
        assert!(
            start.contains("err=totp-pw"),
            "wrong password on Set up authenticator must use totp-pw"
        );
        assert!(
            !start.contains("err=totp\""),
            "setup must not reuse the disable error"
        );
    }

    fn scratch_admin() -> (std::path::PathBuf, crate::state::AppState) {
        let dir = std::env::temp_dir().join(format!("ks-step-{}", uuid::Uuid::new_v4()));
        let stores = keystone_store::Stores::open(&dir, 24).unwrap();
        let hash = crate::auth::hash_password("test-pass-ok").unwrap();
        crate::auth::ensure_admin(&stores.metadata, "admin", &hash).unwrap();
        let cfg = keystone_core::config::ServerConfig {
            data_dir: dir.to_string_lossy().into(),
            ..keystone_core::config::ServerConfig::default()
        };
        (dir, crate::state::AppState::for_test(cfg, stores))
    }

    fn enable_totp(state: &crate::state::AppState, codes: &[String]) -> String {
        let secret = totp::new_secret();
        let hashes = totp::hash_backup_codes(codes).unwrap();
        state
            .stores
            .metadata
            .set_user_totp(
                "admin",
                &keystone_store::TotpRecord {
                    secret: secret.clone(),
                    pending: String::new(),
                    enabled: true,
                    backup_json: totp::backup_hashes_json(&hashes),
                    last_step: 0,
                },
            )
            .unwrap();
        secret
    }

    #[test]
    fn step_up_skips_when_not_required_or_totp_off() {
        let (dir, state) = scratch_admin();
        assert!(consume_step_up(&state, "admin", false, "").is_ok());
        assert!(consume_step_up(&state, "admin", true, "").is_ok());
        assert_eq!(
            consume_step_up(&state, "unknown", true, "123456"),
            Err(StepUpError::Denied)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn step_up_requires_fresh_code_not_backup() {
        let (dir, state) = scratch_admin();
        let codes = totp::generate_backup_codes();
        let secret = enable_totp(&state, &codes);
        assert_eq!(
            consume_step_up(&state, "admin", true, ""),
            Err(StepUpError::Denied)
        );
        assert_eq!(
            consume_step_up(&state, "admin", true, &codes[0]),
            Err(StepUpError::Denied)
        );
        let code = totp::code_now(&secret, "admin");
        assert!(consume_step_up(&state, "admin", true, &code).is_ok());
        assert_eq!(
            consume_step_up(&state, "admin", true, &code),
            Err(StepUpError::Denied)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn step_up_shares_login_gate() {
        let (dir, state) = scratch_admin();
        let secret = enable_totp(&state, &totp::generate_backup_codes());
        for _ in 0..8 {
            assert_eq!(
                consume_step_up(&state, "admin", true, "000000"),
                Err(StepUpError::Denied)
            );
        }
        let code = totp::code_now(&secret, "admin");
        assert_eq!(
            consume_step_up(&state, "admin", true, &code),
            Err(StepUpError::Locked)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn step_up_rejects_backup_codes_in_source() {
        let consume = include_str!("http.rs")
            .split("fn consume_step_up")
            .nth(1)
            .expect("consume_step_up")
            .split("fn step_up_denied")
            .next()
            .expect("consume_step_up body");
        assert!(
            consume.contains("verify_code_step"),
            "step-up must consume a TOTP window"
        );
        assert!(
            !consume.contains("take_backup_code"),
            "backup codes are for sign-in only"
        );
    }

    #[test]
    fn logo_is_public_and_in_chrome() {
        let src = include_str!("http.rs");
        let head = src.split("#[cfg(test)]").next().expect("router source");
        let authed_end = head.find(".layer(").expect("session layer");
        let logo = head.find("/static/logo.svg").expect("logo route");
        assert!(
            logo > authed_end,
            "logo must be public so login can show it"
        );
        let bytes = include_bytes!("static/logo.svg");
        assert!(bytes.len() > 1024, "logo file is missing or empty");
        assert!(
            bytes.starts_with(b"<?xml") || bytes.windows(4).any(|w| w == b"<svg"),
            "logo must be SVG"
        );
        let layout = include_str!("../templates/layout.html");
        assert!(layout.contains("/static/logo.svg"));
        assert!(layout.contains("a class=\"brand\""));
        let login = include_str!("../templates/login.html");
        assert!(login.contains("/static/logo.svg"));
        let css = include_str!("static/app.css");
        assert!(css.contains(".brand img"));
        assert!(css.contains(".login-mark"));
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
        for op in DockerOp::all() {
            if !op.mutating() {
                continue;
            }
            let name = op.as_str();
            if op == DockerOp::ContainerExec {
                assert!(
                    !js.contains(name) && !html.contains(name),
                    "interactive exec must stay out of this UI"
                );
                continue;
            }
            assert!(
                js.contains(name) || html.contains(&format!("docker/{name}")),
                "{name} must appear in the Docker UI"
            );
        }
        assert!(js.contains("/sys/net_set"));
        assert!(js.contains("/sys/vlan_add"));
        assert!(js.contains("/sys/wifi_join"));
        assert!(js.contains("/sys/ssh_password"));
        assert!(js.contains("/sys/updates"));
        assert!(js.contains("/sys/autoremove"));
        assert!(js.contains("/sys/gitlab-backup"));
        assert!(js.contains("/sys/gitlab_restore"));
        assert!(js.contains("/sys/reboot"));
        assert!(SysOp::NetSet.mutating());
        assert!(SysOp::VlanAdd.mutating());
        assert!(SysOp::WifiJoin.mutating());
        assert!(SysOp::WifiJoin.needs_step_up());
        assert!(SysOp::SshPassword.mutating());
        assert!(SysOp::SshPassword.needs_step_up());
        assert!(!SysOp::SshPassword.streams());
        assert!(!SysOp::WifiScan.mutating());
        assert!(!SysOp::WifiScan.needs_step_up());
        assert!(!SysOp::WifiJoin.streams());
        assert!(SysOp::VlanAdd.needs_step_up());
        assert!(!SysOp::VlanAdd.streams());
        assert!(SysOp::UpdatesApply.mutating());
        assert!(SysOp::UpdatesAutoremove.mutating());
        assert!(SysOp::GitlabBackup.mutating());
        assert!(SysOp::GitlabRestore.mutating());
        assert!(SysOp::GitlabRestore.streams());
        assert!(SysOp::GitlabRestore.needs_step_up());
        assert!(!SysOp::GitlabBackup.needs_step_up());
        assert!(SysOp::Reboot.mutating());
        assert!(SysOp::UnitRestart.mutating());
        assert!(!SysOp::Status.mutating());
        assert!(!SysOp::UpdatesList.mutating());
        assert!(!SysOp::Journal.mutating());
        assert!(!SysOp::Reboot.streams());
        assert!(!SysOp::UnitRestart.streams());
        assert!(SysOp::UnitRestart.needs_step_up());
        assert!(SysOp::Journal.streams());
        assert!(SysOp::UpdatesAutoremove.streams());
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
    fn session_idle_slides_and_is_not_a_week() {
        let src = include_str!("http.rs");
        let head = src.split("#[cfg(test)]").next().expect("router source");
        assert!(
            head.contains("SESSION_IDLE_SECS: i64 = 2 * 60 * 60"),
            "finished logins idle out after two hours"
        );
        assert!(
            !head.contains("86400 * 7"),
            "sessions must not last a week without UI traffic"
        );
        let require = head
            .split("async fn require_session")
            .nth(1)
            .expect("require_session")
            .split("async fn login_page")
            .next()
            .expect("require_session body");
        assert!(
            require.contains("touch_session") && require.contains("session_needs_touch"),
            "authed hits must slide idle expiry so an open UI stays signed in"
        );
        assert!(
            require.contains("pending_2fa"),
            "pending 2FA must not get the two-hour idle window"
        );
        let now = 1_700_000_000;
        assert!(
            !session_needs_touch(now + SESSION_IDLE_SECS, now),
            "fresh login must not rewrite SQLite on every poll"
        );
        assert!(session_needs_touch(
            now + SESSION_IDLE_SECS - SESSION_TOUCH_EVERY_SECS,
            now
        ));
        let cookie = session_cookie("sid".into(), false, None);
        let set = cookie.to_string().to_ascii_lowercase();
        assert!(
            !set.contains("max-age"),
            "finished login is a session cookie so the browser drops it on close, got {set}"
        );
        assert!(set.contains("httponly"));
        let pending = session_cookie("p".into(), false, Some(PENDING_2FA_SECS));
        assert!(
            pending
                .to_string()
                .to_ascii_lowercase()
                .contains("max-age=300"),
            "pending 2FA still expires in five minutes"
        );
    }

    #[test]
    fn pagehide_must_not_logout_and_heartbeat_keeps_ui_alive() {
        let js = include_str!("static/app.js");
        assert!(
            !js.contains("sendBeacon(\"/logout\""),
            "pagehide must not POST logout; tab switch and Chrome discard fire it"
        );
        assert!(
            !js.contains("keystone_tabs"),
            "last-tab localStorage must not drive logout"
        );
        assert!(
            js.contains("/api/v1/session") && js.contains("visibilitychange"),
            "an open UI must heartbeat so idle does not kick a sitting operator"
        );
    }

    #[test]
    fn hub_images_ui_is_cards_filling_pull() {
        let js = include_str!("static/app.js");
        let css = include_str!("static/app.css");
        let html = include_str!("../templates/node.html");
        assert!(
            html.contains("hub-query") && html.contains("image-pull-name"),
            "Images toolbar must keep Search next to Pull"
        );
        assert!(
            html.contains("docker/image_login")
                && html.contains("ghcr.io")
                && html.contains("docker.io")
                && html.contains("not in KeyStone's database"),
            "Images toolbar must offer Hub/GHCR login on the node, not a server-side store"
        );
        assert!(
            !html.contains("hub.docker.com/v2") && !js.contains("ghcr.io/v2"),
            "login is not GHCR or Hub browse"
        );
        assert!(
            js.contains("hub-card") && js.contains("hub-detail") && js.contains("hub-tag"),
            "Hub results must be glance cards with a tag detail pane"
        );
        assert!(
            js.contains("nameField.value") && js.contains("pull_ref"),
            "a tag must fill the existing Pull field"
        );
        assert!(
            js.contains("not an app store"),
            "Hub cards must not become a CasaOS-style shop"
        );
        assert!(
            !js.contains("hub-list"),
            "stacked Hub rows were replaced by cards"
        );
        assert!(css.contains(".hub-card") && css.contains(".hub-grid"));
        assert!(
            !js.contains("/v2/search/repositories"),
            "the browser must not call Hub directly"
        );
    }

    #[test]
    fn compose_ui_is_cards_then_detail() {
        let js = include_str!("static/app.js");
        let css = include_str!("static/app.css");
        assert!(
            js.contains("compose-card")
                && js.contains("compose-detail")
                && js.contains("data-project")
                && js.contains(" running")
                && js.contains(" exited"),
            "Compose tab must be glance cards with running/exited counts"
        );
        assert!(
            js.contains("compose_update")
                && js.contains("compose_up")
                && js.contains("compose_down")
                && js.contains("/compose/")
                && js.contains("/logs"),
            "Compose detail must keep Up/Down/Update and Logs"
        );
        assert!(
            js.contains("Service") && js.contains("Ports"),
            "Compose detail must keep the service table"
        );
        assert!(
            js.contains("not a CasaOS shop"),
            "Compose cards must not become a CasaOS-style shop"
        );
        assert!(css.contains(".compose-card") && css.contains(".compose-grid"));
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
        let session_api = head.find("/api/v1/session").expect("session heartbeat");
        assert!(
            session_api < authed_end,
            "session heartbeat must require a UI cookie"
        );
        let usage = head
            .find("/api/v1/nodes/{id}/container-usage")
            .expect("container usage API");
        assert!(
            usage < authed_end,
            "container usage must require a UI session"
        );
        let inspect = head
            .find("/api/v1/nodes/{id}/containers/{cid}")
            .expect("container inspect API");
        assert!(
            inspect < authed_end,
            "container inspect must require a UI session"
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
        let autoremove = head
            .find("/nodes/{id}/sys/autoremove")
            .expect("sys autoremove page");
        let backup = head
            .find("/nodes/{id}/sys/gitlab-backup")
            .expect("gitlab backup page");
        let restore = head
            .find("/nodes/{id}/sys/gitlab-restore")
            .expect("gitlab restore page");
        let journal = head
            .find("/nodes/{id}/sys/journal/{unit}")
            .expect("journal follow page");
        let api = head
            .find("/api/v1/nodes/{id}/sys/updates")
            .expect("sys updates API");
        let wifi = head
            .find("/api/v1/nodes/{id}/sys/wifi")
            .expect("sys wifi API");
        assert!(post < authed_end);
        assert!(apply < authed_end);
        assert!(autoremove < authed_end);
        assert!(backup < authed_end);
        assert!(restore < authed_end);
        assert!(journal < authed_end);
        assert!(api < authed_end);
        assert!(wifi < authed_end);
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
            ipv6_method: None,
            ipv6_address: None,
            ipv6_prefix: None,
            ipv6_gateway: None,
            ipv6_dns: None,
            unit: None,
            name: None,
            vlan: None,
            ssid: None,
            psk: None,
            password_auth: None,
            totp: "123456".into(),
            redirect: String::new(),
        };
        let p = sys_form_payload(&form);
        assert!(p.contains("\"iface\":\"eth0\""));
        assert!(p.contains("\"method\":\"static\""));
        assert!(p.contains("192.168.0.50"));
        assert!(p.contains("\"ipv6_method\":\"auto\""));
        assert!(!p.contains(';'));
        assert!(
            !p.contains("totp") && !p.contains("123456"),
            "authenticator code must not go to the helper payload"
        );
        let restart = SysForm {
            payload: None,
            iface: None,
            method: None,
            address: None,
            prefix: None,
            gateway: None,
            dns: None,
            ipv6_method: None,
            ipv6_address: None,
            ipv6_prefix: None,
            ipv6_gateway: None,
            ipv6_dns: None,
            unit: Some(" docker.service ".into()),
            name: None,
            vlan: None,
            ssid: None,
            psk: None,
            password_auth: None,
            totp: "000000".into(),
            redirect: String::new(),
        };
        let r = sys_form_payload(&restart);
        assert!(r.contains("\"unit\":\"docker.service\""));
        assert!(!r.contains("000000"));
        let restore = SysForm {
            payload: None,
            iface: None,
            method: None,
            address: None,
            prefix: None,
            gateway: None,
            dns: None,
            ipv6_method: None,
            ipv6_address: None,
            ipv6_prefix: None,
            ipv6_gateway: None,
            ipv6_dns: None,
            unit: None,
            name: Some(" 1712345678_gitlab_backup.tar ".into()),
            vlan: None,
            ssid: None,
            psk: None,
            password_auth: None,
            totp: "654321".into(),
            redirect: String::new(),
        };
        let g = sys_form_payload(&restore);
        assert!(g.contains("\"name\":\"1712345678_gitlab_backup.tar\""));
        assert!(!g.contains("654321"));
        let restore_v6 = SysForm {
            payload: None,
            iface: Some("eth0".into()),
            method: Some("dhcp".into()),
            address: None,
            prefix: None,
            gateway: None,
            dns: None,
            ipv6_method: Some("static".into()),
            ipv6_address: Some(" 2001:db8::10 ".into()),
            ipv6_prefix: Some("64".into()),
            ipv6_gateway: Some("2001:db8::1".into()),
            ipv6_dns: Some("2001:db8::53".into()),
            unit: None,
            name: None,
            vlan: None,
            ssid: None,
            psk: None,
            password_auth: None,
            totp: "111111".into(),
            redirect: String::new(),
        };
        let v6 = sys_form_payload(&restore_v6);
        assert!(v6.contains("\"ipv6_method\":\"static\""));
        assert!(v6.contains("2001:db8::10"));
        assert!(!v6.contains('%') && !v6.contains("111111"));
        let vlan = SysForm {
            payload: None,
            iface: Some(" eth0 ".into()),
            method: None,
            address: None,
            prefix: None,
            gateway: None,
            dns: None,
            ipv6_method: None,
            ipv6_address: None,
            ipv6_prefix: None,
            ipv6_gateway: None,
            ipv6_dns: None,
            unit: None,
            name: None,
            vlan: Some(" 10 ".into()),
            ssid: None,
            psk: None,
            password_auth: None,
            totp: "222222".into(),
            redirect: String::new(),
        };
        let v = sys_form_payload(&vlan);
        assert!(v.contains("\"iface\":\"eth0\""));
        assert!(v.contains("\"vlan\":10"));
        assert!(!v.contains("222222"));
        let wifi = SysForm {
            payload: None,
            iface: Some("wlan0".into()),
            method: None,
            address: None,
            prefix: None,
            gateway: None,
            dns: None,
            ipv6_method: None,
            ipv6_address: None,
            ipv6_prefix: None,
            ipv6_gateway: None,
            ipv6_dns: None,
            unit: None,
            name: None,
            vlan: None,
            ssid: Some(" Home Lab ".into()),
            psk: Some("testpass1".into()),
            password_auth: None,
            totp: "333333".into(),
            redirect: String::new(),
        };
        let w = sys_form_payload(&wifi);
        assert!(w.contains("\"ssid\":\"Home Lab\""));
        assert!(w.contains("testpass1"));
        assert!(!w.contains("333333"));
        let redacted = keystone_core::sys::audit_sys_target(SysOp::WifiJoin, &w);
        assert!(!redacted.contains("testpass1"));
        assert!(redacted.contains("Home Lab"));
        let ssh = SysForm {
            payload: None,
            iface: None,
            method: None,
            address: None,
            prefix: None,
            gateway: None,
            dns: None,
            ipv6_method: None,
            ipv6_address: None,
            ipv6_prefix: None,
            ipv6_gateway: None,
            ipv6_dns: None,
            unit: None,
            name: None,
            vlan: None,
            ssid: None,
            psk: None,
            password_auth: Some(" no ".into()),
            totp: "444444".into(),
            redirect: String::new(),
        };
        let s = sys_form_payload(&ssh);
        assert!(s.contains("\"password_auth\":false"));
        assert!(!s.contains("444444"));
        assert!(!s.contains("yes;rm"));
        let junk = SysForm {
            payload: None,
            iface: None,
            method: None,
            address: None,
            prefix: None,
            gateway: None,
            dns: None,
            ipv6_method: None,
            ipv6_address: None,
            ipv6_prefix: None,
            ipv6_gateway: None,
            ipv6_dns: None,
            unit: None,
            name: None,
            vlan: None,
            ssid: None,
            psk: None,
            password_auth: Some("yes;rm".into()),
            totp: "555555".into(),
            redirect: String::new(),
        };
        let j = sys_form_payload(&junk);
        assert!(!j.contains("password_auth"));
        assert!(!j.contains("yes;rm"));
        assert!(!j.contains("555555"));
    }

    #[test]
    fn system_ui_confirms_host_mutations() {
        let js = include_str!("static/app.js");
        let src = include_str!("http.rs");
        assert!(js.contains("paintSystem"), "System tab painter missing");
        assert!(
            js.contains("Apply pending apt upgrades"),
            "Apply updates must ask before apt-get upgrade"
        );
        assert!(
            js.contains("Remove unused packages with apt-get autoremove"),
            "Autoremove must ask before apt-get autoremove"
        );
        assert!(
            js.contains("This does not dist-upgrade"),
            "Autoremove confirm must say this is not dist-upgrade"
        );
        assert!(js.contains("/sys/autoremove"));
        assert!(
            js.contains("Unattended upgrades"),
            "System tab must show whether unattended-upgrades is enabled"
        );
        assert!(
            js.contains("No unattended run on disk") && js.contains("Last unattended run"),
            "System tab must show unattended last-run age"
        );
        assert!(
            !js.contains("unattended-enable") && !js.contains("20auto-upgrades"),
            "UI must not edit unattended-upgrades config"
        );
        assert!(
            js.contains("Ubuntu will not auto-restart docker or ssh"),
            "Apply confirm must say needrestart will not bounce docker during upgrade"
        );
        assert!(
            js.contains("Services still using old libraries"),
            "System tab must list leftover needrestart services"
        );
        assert!(
            js.contains("/sys/unit_restart")
                && js.contains("systemctl restart")
                && js.contains("sys-restart-totp"),
            "leftover/failed units must offer listed-name restart behind step-up"
        );
        assert!(
            !js.contains("this tab does not restart individual units"),
            "leftover restart is in; do not tell operators to use a shell"
        );
        assert!(
            js.contains("Failed systemd units"),
            "System tab must list failed units"
        );
        assert!(
            js.contains("Clock synchronized") && js.contains("Clock not synchronized"),
            "System tab must show NTP yes/no"
        );
        assert!(
            js.contains("/sys/journal/"),
            "System tab must link to journal follow pages"
        );
        for unit in keystone_core::sys::JOURNAL_UNITS {
            assert!(
                js.contains(unit),
                "System tab must offer {unit} (no unit-name textbox)"
            );
        }
        assert!(
            js.contains("Last dump") && js.contains("No dump on disk"),
            "GitLab Omnibus block must show dump age"
        );
        assert!(
            js.contains("Reboot this node now?"),
            "reboot must ask before systemctl reboot"
        );
        assert!(
            js.contains("This node is serving the KeyStone UI"),
            "rebooting the UI host must warn that the session will drop"
        );
        assert!(js.contains("/sys/reboot"));
        assert!(!js.contains("/sys/poweroff"));
        assert!(js.contains("/sys/unit_restart"));
        let html = include_str!("../templates/node.html");
        assert!(
            html.contains("data-ui-host"),
            "System tab must know if this node serves the UI"
        );
        assert!(
            html.contains("data-totp"),
            "System tab must know whether TOTP is on"
        );
        assert!(
            html.contains("leftover restart"),
            "Settings manage checkbox must mention leftover unit restart"
        );
        assert!(
            html.contains("GitLab restore"),
            "Settings manage checkbox must mention GitLab restore"
        );
        assert!(
            html.contains("VLAN"),
            "Settings manage checkbox must mention VLAN create"
        );
        assert!(
            html.contains("Wi-Fi"),
            "Settings manage checkbox must mention Wi-Fi join"
        );
        assert!(
            html.contains("SSH password"),
            "Settings manage checkbox must mention SSH password logins"
        );
        assert!(
            html.contains("and reboot"),
            "Settings manage checkbox must mention reboot"
        );
        assert!(
            html.contains("autoremove"),
            "Settings manage checkbox must mention apt autoremove"
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
            js.contains("/sys/gitlab_restore")
                && js.contains("sys-gitlab-restore-totp")
                && js.contains("not a path textbox"),
            "listed Omnibus dumps must offer restore behind step-up, not a path field"
        );
        assert!(
            js.contains("This replaces GitLab data"),
            "restore must warn that it replaces GitLab data"
        );
        assert!(
            !js.contains("Restore is not in this UI"),
            "Omnibus restore is in this slice"
        );
        assert!(
            js.contains("Change addressing on this node"),
            "net_set must ask before changing addressing"
        );
        assert!(
            js.contains("/sys/vlan_add")
                && js.contains("Create VLAN")
                && js.contains("Not a name textbox")
                && js.contains(r#"vlanId.name = "vlan""#),
            "VLAN create must be parent + numeric id, not a name field"
        );
        assert!(
            js.contains("/sys/wifi_join")
                && js.contains("/api/v1/nodes/")
                && js.contains("sys/wifi")
                && js.contains("Join Wi-Fi")
                && js.contains("not an SSID textbox")
                && js.contains("wifiIface"),
            "Wi-Fi join must scan listed SSIDs, not a free SSID field"
        );
        assert!(
            js.contains("/sys/ssh_password")
                && js.contains("password_auth")
                && js.contains("keys only")
                && js.contains("not a user editor"),
            "SSH password must be a yes/no toggle, not a user editor"
        );
        assert!(
            !js.contains("PermitRootLogin")
                && !js.contains("useradd")
                && !js.contains("/sys/ufw")
                && !js.contains("/sys/timezone"),
            "SSH password slice must not grow a user, firewall, or timezone editor"
        );
        assert!(
            js.contains("data-totp")
                && js.contains("one-time-code")
                && js.contains("Backup codes are for sign-in only"),
            "IPv4 must collect a current authenticator code when TOTP is on"
        );
        assert!(
            js.contains("ethernetIface") && js.contains("ipv6_method"),
            "Addressing form must skip Wi-Fi/docker NICs and include IPv6"
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
        assert!(
            js.contains("container-card")
                && js.contains("/api/v1/nodes/")
                && js.contains("/containers/"),
            "Containers tab must be clickable cards that load inspect"
        );
        assert!(
            js.contains("sys-split") && js.contains("Health") && js.contains("Actions"),
            "System tab must split health vs actions"
        );
        assert!(
            html.contains("System Manage can change this host"),
            "Settings must warn before System Manage"
        );
        let inspect = src
            .split("async fn container_inspect_api")
            .nth(1)
            .expect("container_inspect_api")
            .split("async fn call_json_op")
            .next()
            .expect("container_inspect_api body");
        assert!(
            inspect.contains("summarize_container_inspect") && inspect.contains("docker_ref_ok"),
            "inspect API must strip Engine JSON and reject junk ids"
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
