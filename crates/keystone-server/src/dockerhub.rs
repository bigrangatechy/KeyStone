// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Cookie-authed Docker Hub lookup. The server never opens docker.sock;
//! pull still runs on the agent as `image_pull`.

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use keystone_core::dockerhub::{
    parse_search, parse_tags, search_url, tags_url, HubError, HubRepo, HubTag,
};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

const USER_AGENT: &str = concat!(
    "KeyStone/",
    env!("CARGO_PKG_VERSION"),
    " (+http://git.bigrangatech.com/Ranga/keystone)"
);

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
}

#[derive(Deserialize)]
pub struct TagsQuery {
    namespace: Option<String>,
    name: Option<String>,
}

#[derive(Serialize)]
struct SearchBody {
    results: Vec<HubRepo>,
}

#[derive(Serialize)]
struct TagsBody {
    results: Vec<HubTag>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub async fn search(State(state): State<AppState>, Query(q): Query<SearchQuery>) -> Response {
    let raw = q.q.unwrap_or_default();
    let url = match search_url(&raw) {
        Ok(u) => u,
        Err(e) => return json_err(StatusCode::BAD_REQUEST, hub_query_msg(e)),
    };
    match hub_get(&state.http, &url).await {
        Ok(body) => match parse_search(&body) {
            Ok(results) => Json(SearchBody { results }).into_response(),
            Err(_) => json_err(
                StatusCode::BAD_GATEWAY,
                "Docker Hub response was not usable. Type the image name to pull.",
            ),
        },
        Err(resp) => resp,
    }
}

pub async fn tags(State(state): State<AppState>, Query(q): Query<TagsQuery>) -> Response {
    let ns = q.namespace.unwrap_or_default();
    let name = q.name.unwrap_or_default();
    let url = match tags_url(&ns, &name) {
        Ok(u) => u,
        Err(_) => {
            return json_err(StatusCode::BAD_REQUEST, "repository name is invalid");
        }
    };
    match hub_get(&state.http, &url).await {
        Ok(body) => match parse_tags(&ns, &name, &body) {
            Ok(results) => Json(TagsBody { results }).into_response(),
            Err(_) => json_err(
                StatusCode::BAD_GATEWAY,
                "Docker Hub response was not usable. Type the image name to pull.",
            ),
        },
        Err(resp) => resp,
    }
}

async fn hub_get(client: &reqwest::Client, url: &str) -> Result<String, Response> {
    debug_assert!(url.starts_with("https://hub.docker.com/"));
    let res = client
        .get(url)
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                json_err(
                    StatusCode::GATEWAY_TIMEOUT,
                    "Docker Hub timed out. Type the image name to pull.",
                )
            } else {
                tracing::warn!(error = %e, "docker hub request failed");
                json_err(
                    StatusCode::BAD_GATEWAY,
                    "Could not reach Docker Hub. Type the image name to pull.",
                )
            }
        })?;
    let status = res.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(json_err(
            StatusCode::TOO_MANY_REQUESTS,
            "Docker Hub rate-limited this server. Type the image name to pull.",
        ));
    }
    if !status.is_success() {
        tracing::warn!(%status, "docker hub HTTP error");
        return Err(json_err(
            StatusCode::BAD_GATEWAY,
            format!("Docker Hub returned {status}. Type the image name to pull."),
        ));
    }
    res.text().await.map_err(|_| {
        json_err(
            StatusCode::BAD_GATEWAY,
            "Docker Hub response was not usable. Type the image name to pull.",
        )
    })
}

fn hub_query_msg(err: HubError) -> &'static str {
    match err {
        HubError::QueryTooShort => "type at least two characters",
        HubError::QueryTooLong => "search query is too long",
        HubError::QueryInvalid => "search query has invalid characters",
        HubError::RepoInvalid => "repository name is invalid",
        HubError::Response => "Docker Hub response was not usable. Type the image name to pull.",
    }
}

fn json_err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorBody { error: msg.into() })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use keystone_core::dockerhub::{parse_search, parse_tags, search_url, tags_url};

    #[test]
    fn hub_urls_are_fixed_origin() {
        let s = search_url("nginx").unwrap();
        let t = tags_url("library", "nginx").unwrap();
        assert!(s.starts_with("https://hub.docker.com/"));
        assert!(t.starts_with("https://hub.docker.com/"));
        assert!(!s.contains("docker.sock"));
        assert!(!t.contains("docker.sock"));
        assert!(!s.contains("token"));
        assert!(!t.contains("token"));
    }

    #[test]
    fn operator_path_official_tag_fills_pull_ref() {
        let search = parse_search(
            r#"{"results":[{"repo_name":"nginx","short_description":"Official build of Nginx.","star_count":1,"is_official":true}]}"#,
        )
        .unwrap();
        assert_eq!(search[0].pull_name, "nginx");
        assert!(search[0].official);
        let tags = parse_tags(
            "library",
            "nginx",
            r#"{"results":[{"name":"1.27.3","last_updated":"2026-08-12T00:00:00Z","tag_status":"active","images":[{"architecture":"amd64","os":"linux"},{"architecture":"arm64","os":"linux","variant":"v8"}]}]}"#,
        )
        .unwrap();
        assert_eq!(tags[0].pull_ref, "nginx:1.27.3");
        assert_eq!(tags[0].architectures, vec!["amd64", "arm64/v8"]);
    }

    #[test]
    fn rate_limit_copy_tells_operator_to_type_the_name() {
        let resp = json_err(
            StatusCode::TOO_MANY_REQUESTS,
            "Docker Hub rate-limited this server. Type the image name to pull.",
        );
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
