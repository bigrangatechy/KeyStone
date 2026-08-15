// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Docker Hub search/tag JSON mapping. No HTTP — the server fetches.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const HUB_ORIGIN: &str = "https://hub.docker.com";
pub const SEARCH_PAGE_SIZE: u32 = 8;
pub const TAGS_PAGE_SIZE: u32 = 12;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HubError {
    #[error("search query is too short")]
    QueryTooShort,
    #[error("search query is too long")]
    QueryTooLong,
    #[error("search query has invalid characters")]
    QueryInvalid,
    #[error("repository name is invalid")]
    RepoInvalid,
    #[error("Docker Hub response was not usable")]
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HubRepo {
    pub namespace: String,
    pub name: String,
    pub official: bool,
    pub description: String,
    pub stars: u64,
    /// What you would type before `:tag` (`nginx`, or `bitnami/nginx`).
    pub pull_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HubTag {
    pub tag: String,
    pub last_updated: String,
    pub architectures: Vec<String>,
    pub pull_ref: String,
}

pub fn validate_search_query(raw: &str) -> Result<String, HubError> {
    let q = raw.trim();
    if q.chars().count() < 2 {
        return Err(HubError::QueryTooShort);
    }
    if q.chars().count() > 64 {
        return Err(HubError::QueryTooLong);
    }
    if q.contains("://") || q.contains("..") {
        return Err(HubError::QueryInvalid);
    }
    if !q
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ' '))
    {
        return Err(HubError::QueryInvalid);
    }
    Ok(q.to_string())
}

pub fn validate_repo_segment(raw: &str) -> Result<String, HubError> {
    let s = raw.trim().to_ascii_lowercase();
    if is_name_segment(&s) {
        Ok(s)
    } else {
        Err(HubError::RepoInvalid)
    }
}

/// `nginx:1.27` for official/`library`; `bitnami/nginx:1.27` otherwise.
pub fn pull_ref(namespace: &str, name: &str, tag: &str) -> String {
    let image = pull_name(namespace, name);
    if tag.is_empty() {
        image
    } else {
        format!("{image}:{tag}")
    }
}

pub fn pull_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() || namespace == "library" {
        name.to_string()
    } else {
        format!("{namespace}/{name}")
    }
}

pub fn search_url(query: &str) -> Result<String, HubError> {
    let q = validate_search_query(query)?;
    Ok(format!(
        "{HUB_ORIGIN}/v2/search/repositories/?query={}&page_size={SEARCH_PAGE_SIZE}",
        percent_encode(&q)
    ))
}

pub fn tags_url(namespace: &str, name: &str) -> Result<String, HubError> {
    let ns = validate_repo_segment(namespace)?;
    let repo = validate_repo_segment(name)?;
    Ok(format!(
        "{HUB_ORIGIN}/v2/namespaces/{ns}/repositories/{repo}/tags?page_size={TAGS_PAGE_SIZE}"
    ))
}

pub fn parse_search(body: &str) -> Result<Vec<HubRepo>, HubError> {
    let parsed: HubSearchBody = serde_json::from_str(body).map_err(|_| HubError::Response)?;
    let mut out = Vec::new();
    for hit in parsed.results {
        let Some((namespace, name)) = split_repo(&hit.repo_name, hit.is_official) else {
            continue;
        };
        out.push(HubRepo {
            official: hit.is_official,
            description: truncate(&hit.short_description, 160),
            stars: hit.star_count,
            pull_name: pull_name(&namespace, &name),
            namespace,
            name,
        });
        if out.len() >= SEARCH_PAGE_SIZE as usize {
            break;
        }
    }
    Ok(out)
}

pub fn parse_tags(namespace: &str, name: &str, body: &str) -> Result<Vec<HubTag>, HubError> {
    let ns = validate_repo_segment(namespace)?;
    let repo = validate_repo_segment(name)?;
    let parsed: HubTagsBody = serde_json::from_str(body).map_err(|_| HubError::Response)?;
    let mut out = Vec::new();
    for hit in parsed.results {
        if !is_tag_name(&hit.name) {
            continue;
        }
        if hit.tag_status.as_deref() == Some("inactive") {
            continue;
        }
        out.push(HubTag {
            last_updated: date_ymd(&hit.last_updated),
            architectures: architectures(&hit.images),
            pull_ref: pull_ref(&ns, &repo, &hit.name),
            tag: hit.name,
        });
        if out.len() >= TAGS_PAGE_SIZE as usize {
            break;
        }
    }
    Ok(out)
}

fn is_name_segment(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    let b = s.as_bytes();
    if !b[0].is_ascii_lowercase() && !b[0].is_ascii_digit() {
        return false;
    }
    let mut prev_sep = false;
    for &c in b {
        match c {
            b'a'..=b'z' | b'0'..=b'9' => prev_sep = false,
            b'.' | b'_' | b'-' if !prev_sep => prev_sep = true,
            _ => return false,
        }
    }
    !prev_sep
}

fn is_tag_name(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 128 {
        return false;
    }
    if !b[0].is_ascii_alphanumeric() && b[0] != b'_' {
        return false;
    }
    b.iter()
        .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

fn split_repo(repo_name: &str, official: bool) -> Option<(String, String)> {
    let raw = repo_name
        .trim()
        .trim_start_matches('/')
        .to_ascii_lowercase();
    if raw.is_empty() {
        return None;
    }
    let (ns, name) = match raw.split_once('/') {
        Some((a, b)) if !b.is_empty() && !b.contains('/') => (a.to_string(), b.to_string()),
        None => ("library".into(), raw),
        _ => return None,
    };
    let ns = if official && !repo_name.contains('/') {
        "library".into()
    } else {
        ns
    };
    if is_name_segment(&ns) && is_name_segment(&name) {
        Some((ns, name))
    } else {
        None
    }
}

fn architectures(images: &[HubImage]) -> Vec<String> {
    let mut labels = Vec::new();
    for img in images {
        let Some(label) = arch_label(img) else {
            continue;
        };
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels.sort_by(|a, b| arch_rank(a).cmp(&arch_rank(b)).then_with(|| a.cmp(b)));
    labels.truncate(8);
    labels
}

fn arch_label(img: &HubImage) -> Option<String> {
    let arch = img.architecture.trim();
    let os = img.os.trim();
    if arch.is_empty() || arch == "unknown" || os == "unknown" {
        return None;
    }
    let mut s = arch.to_string();
    if let Some(v) = img
        .variant
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        s.push('/');
        s.push_str(v);
    }
    Some(s)
}

fn arch_rank(label: &str) -> u8 {
    if label == "amd64" || label.starts_with("amd64/") {
        0
    } else if label == "arm64" || label.starts_with("arm64/") || label.starts_with("aarch64") {
        1
    } else {
        2
    }
}

fn date_ymd(iso: &str) -> String {
    let t = iso.trim();
    if t.len() >= 10 && t.as_bytes().get(4) == Some(&b'-') && t.as_bytes().get(7) == Some(&b'-') {
        t[..10].to_string()
    } else {
        String::new()
    }
}

fn truncate(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn percent_encode(s: &str) -> String {
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
struct HubSearchBody {
    #[serde(default)]
    results: Vec<HubSearchHit>,
}

#[derive(Deserialize)]
struct HubSearchHit {
    #[serde(default)]
    repo_name: String,
    #[serde(default)]
    short_description: String,
    #[serde(default)]
    star_count: u64,
    #[serde(default)]
    is_official: bool,
}

#[derive(Deserialize)]
struct HubTagsBody {
    #[serde(default)]
    results: Vec<HubTagHit>,
}

#[derive(Deserialize)]
struct HubTagHit {
    #[serde(default)]
    name: String,
    #[serde(default)]
    last_updated: String,
    #[serde(default)]
    tag_status: Option<String>,
    #[serde(default)]
    images: Vec<HubImage>,
}

#[derive(Deserialize)]
struct HubImage {
    #[serde(default)]
    architecture: String,
    #[serde(default)]
    os: String,
    variant: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH_NGINX: &str = r#"{
      "count": 291220,
      "next": "https://evil.example/steal",
      "results": [
        {
          "repo_name": "nginx",
          "short_description": "Official build of Nginx.",
          "star_count": 21357,
          "pull_count": 1,
          "repo_owner": "",
          "is_automated": false,
          "is_official": true
        },
        {
          "repo_name": "nginx/nginx-ingress",
          "short_description": "NGINX and NGINX Plus Ingress Controllers for Kubernetes",
          "star_count": 122,
          "is_official": false
        },
        {
          "repo_name": "bitnami/nginx",
          "short_description": "Bitnami nginx",
          "star_count": 50,
          "is_official": false
        }
      ]
    }"#;

    const TAGS_NGINX: &str = r#"{
      "count": 1283,
      "results": [
        {
          "name": "1.27.3",
          "last_updated": "2026-08-12T01:51:00.648Z",
          "tag_status": "active",
          "images": [
            {"architecture": "amd64", "os": "linux", "variant": null},
            {"architecture": "arm64", "os": "linux", "variant": "v8"},
            {"architecture": "unknown", "os": "unknown", "variant": null},
            {"architecture": "arm", "os": "linux", "variant": "v7"}
          ]
        },
        {
          "name": "latest",
          "last_updated": "2026-08-12T01:50:00Z",
          "tag_status": "active",
          "images": [{"architecture": "amd64", "os": "linux"}]
        },
        {
          "name": "skip me",
          "last_updated": "2026-08-01T00:00:00Z",
          "tag_status": "active",
          "images": [{"architecture": "amd64", "os": "linux"}]
        }
      ]
    }"#;

    #[test]
    fn search_maps_official_nginx_to_unprefixed_pull_name() {
        let repos = parse_search(SEARCH_NGINX).unwrap();
        assert_eq!(repos[0].namespace, "library");
        assert_eq!(repos[0].name, "nginx");
        assert!(repos[0].official);
        assert_eq!(repos[0].pull_name, "nginx");
        assert_eq!(repos[0].description, "Official build of Nginx.");
        assert_eq!(repos[0].stars, 21357);
        assert_eq!(repos[1].pull_name, "nginx/nginx-ingress");
        assert!(!repos[1].official);
        assert_eq!(repos[2].pull_name, "bitnami/nginx");
    }

    #[test]
    fn search_ignores_hub_next_url() {
        let repos = parse_search(SEARCH_NGINX).unwrap();
        let blob = serde_json::to_string(&repos).unwrap();
        assert!(!blob.contains("evil.example"));
        assert!(!blob.contains("steal"));
    }

    #[test]
    fn tags_fill_nginx_version_not_library_prefix() {
        let tags = parse_tags("library", "nginx", TAGS_NGINX).unwrap();
        assert_eq!(tags[0].tag, "1.27.3");
        assert_eq!(tags[0].pull_ref, "nginx:1.27.3");
        assert_eq!(tags[0].last_updated, "2026-08-12");
        assert_eq!(tags[0].architectures, vec!["amd64", "arm64/v8", "arm/v7"]);
        assert!(!tags[0].architectures.iter().any(|a| a.contains("unknown")));
        assert_eq!(tags[1].pull_ref, "nginx:latest");
        assert_eq!(tags.len(), 2, "invalid tag names are dropped");
    }

    #[test]
    fn tags_user_repo_keeps_namespace() {
        let tags = parse_tags("bitnami", "nginx", TAGS_NGINX).unwrap();
        assert_eq!(tags[0].pull_ref, "bitnami/nginx:1.27.3");
    }

    #[test]
    fn pull_ref_drops_library() {
        assert_eq!(pull_ref("library", "nginx", "1.27"), "nginx:1.27");
        assert_eq!(pull_ref("library", "nginx", ""), "nginx");
        assert_eq!(
            pull_ref("grafana", "grafana", "11.1.0"),
            "grafana/grafana:11.1.0"
        );
    }

    #[test]
    fn search_query_rejects_urls_and_noise() {
        assert_eq!(validate_search_query("n"), Err(HubError::QueryTooShort));
        assert!(validate_search_query("nginx").is_ok());
        assert!(validate_search_query("bitnami/nginx").is_ok());
        assert_eq!(
            validate_search_query("https://hub.docker.com/r/nginx"),
            Err(HubError::QueryInvalid)
        );
        assert_eq!(
            validate_search_query("../etc/passwd"),
            Err(HubError::QueryInvalid)
        );
        assert_eq!(
            validate_search_query("nginx;curl"),
            Err(HubError::QueryInvalid)
        );
    }

    #[test]
    fn urls_stay_on_hub_docker_com() {
        let search = search_url("nginx 1").unwrap();
        assert!(search.starts_with("https://hub.docker.com/v2/search/repositories/?query="));
        assert!(search.contains("query=nginx%201"));
        assert!(search.contains("page_size=8"));
        assert!(!search.contains("token"));
        let tags = tags_url("library", "nginx").unwrap();
        assert_eq!(
            tags,
            "https://hub.docker.com/v2/namespaces/library/repositories/nginx/tags?page_size=12"
        );
        assert!(tags_url("library", "../nginx").is_err());
        let sneaky = tags_url("https", "nginx").unwrap();
        assert!(sneaky.starts_with("https://hub.docker.com/v2/namespaces/https/"));
        assert!(!sneaky.contains("://nginx"));
        assert!(search_url(&"n".repeat(65)).is_err());
    }

    #[test]
    fn garbage_json_is_response_error() {
        assert_eq!(parse_search("not json"), Err(HubError::Response));
        assert_eq!(
            parse_tags("library", "nginx", "<html>"),
            Err(HubError::Response)
        );
    }

    #[test]
    fn operator_docker_doc_describes_hub_as_a_pull_helper() {
        let docker = include_str!("../../../docs/src/docker.md");
        assert!(docker.contains("Search Docker Hub"));
        assert!(docker.contains("fills"));
        assert!(docker.contains("image_pull"));
        assert!(docker.contains("not an app store"));
        assert!(docker.contains("rate-limited"));
    }
}
