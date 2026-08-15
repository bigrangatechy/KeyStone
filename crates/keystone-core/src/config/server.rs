// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Process configuration (listen addresses, data directory, bootstrap).
///
/// After first start, ingest token, series retention, and scrape jobs are
/// stored in the Settings UI (SQLite). TOML values seed that row once.
/// `KEYSTONE_INGEST_TOKEN` always overrides the stored token. Listen
/// addresses, `data_dir`, and auth username stay in this file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// HTTP UI and API listen address.
    #[serde(default = "default_http")]
    pub http_listen: String,
    /// gRPC ingest listen address.
    #[serde(default = "default_grpc")]
    pub grpc_listen: String,
    /// Data directory for SQLite, sessions, and the series store.
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    /// Bootstrap ingest token. Copied into Settings on first start. After
    /// that, change it in the UI. `KEYSTONE_INGEST_TOKEN` always wins.
    #[serde(default)]
    pub ingest_token: String,
    /// Bootstrap series retention in hours (default 24). Copied into
    /// Settings on first start; change it in the UI afterwards.
    #[serde(default = "default_retention")]
    pub retention_hours: u32,
    #[serde(default)]
    pub auth: ServerAuth,
    /// Bootstrap Prometheus scrape jobs. Copied into Settings on first start.
    #[serde(default)]
    pub prometheus_scrape: Vec<PrometheusScrape>,
    /// Bootstrap SNMP scrape jobs. Copied into Settings on first start.
    #[serde(default)]
    pub snmp_scrape: Vec<SnmpScrape>,
    /// Optional TLS for the UI (and ingest when `tls.ingest` is true).
    /// Empty `cert_file` / `key_file` is plaintext — existing installs
    /// and local smoke stay HTTP.
    #[serde(default)]
    pub tls: TlsConfig,
}

/// PEM certificate + key on disk. Homelab: Let's Encrypt fullchain, or a
/// self-signed cert. Same files can wrap the UI and gRPC ingest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Server certificate PEM (leaf + intermediates).
    #[serde(default)]
    pub cert_file: String,
    /// Private key PEM (PKCS#8 or RSA).
    #[serde(default)]
    pub key_file: String,
    /// Also terminate TLS on `grpc_listen`. Default true once certs are
    /// set. Set false to keep agents on `http://` while the UI is HTTPS.
    #[serde(default = "default_tls_ingest")]
    pub ingest: bool,
}

fn default_tls_ingest() -> bool {
    true
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            cert_file: String::new(),
            key_file: String::new(),
            ingest: default_tls_ingest(),
        }
    }
}

impl TlsConfig {
    /// Both PEM paths, or `None` for plaintext. Error if only one is set.
    pub fn pem_paths(&self) -> Result<Option<(&str, &str)>, String> {
        let cert = self.cert_file.trim();
        let key = self.key_file.trim();
        match (cert.is_empty(), key.is_empty()) {
            (true, true) => Ok(None),
            (false, false) => Ok(Some((cert, key))),
            _ => Err("tls.cert_file and tls.key_file must both be set, or both left empty".into()),
        }
    }

    pub fn ui_https(&self) -> bool {
        matches!(self.pem_paths(), Ok(Some(_)))
    }

    pub fn ingest_https(&self) -> bool {
        self.ingest && self.ui_https()
    }
}

impl ServerConfig {
    pub fn http_addr(&self) -> Result<SocketAddr, String> {
        parse_listen(&self.http_listen, "http_listen")
    }

    pub fn grpc_addr(&self) -> Result<SocketAddr, String> {
        parse_listen(&self.grpc_listen, "grpc_listen")
    }
}

fn parse_listen(raw: &str, key: &str) -> Result<SocketAddr, String> {
    raw.parse()
        .map_err(|e| format!("{key} {raw:?} is not host:port ({e})"))
}

/// Bind failure copy. Packaged units use 8080/9100; cargo smoke must not.
pub fn listen_bind_context(kind: &str, addr: SocketAddr) -> String {
    format!(
        "bind {kind} {addr}: address already in use. Packaged keystone-server listens on 0.0.0.0:8080 and :9100. Cargo smoke uses examples/server.toml on loopback 18080/19100 so both can run."
    )
}

fn default_http() -> String {
    "0.0.0.0:8080".into()
}

fn default_grpc() -> String {
    "0.0.0.0:9100".into()
}

fn default_data_dir() -> String {
    "/var/lib/keystone".into()
}

fn default_retention() -> u32 {
    24
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_listen: default_http(),
            grpc_listen: default_grpc(),
            data_dir: default_data_dir(),
            ingest_token: String::new(),
            retention_hours: default_retention(),
            auth: ServerAuth::default(),
            prometheus_scrape: Vec::new(),
            snmp_scrape: Vec::new(),
            tls: TlsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerAuth {
    /// Local admin username.
    #[serde(default = "default_user")]
    pub username: String,
    /// Argon2id password hash. If empty, `KEYSTONE_ADMIN_PASSWORD` is hashed
    /// on first start, or `changeme` if that env is unset. First UI login
    /// must choose a new password.
    #[serde(default)]
    pub password_hash: String,
}

fn default_user() -> String {
    "admin".into()
}

impl Default for ServerAuth {
    fn default() -> Self {
        Self {
            username: default_user(),
            password_hash: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrometheusScrape {
    /// Job name stored as the node_id if `node_id` is empty.
    pub name: String,
    /// Exposition URL, for example `http://127.0.0.1:9100/metrics`.
    pub url: String,
    #[serde(default = "default_scrape_interval")]
    pub interval_secs: u64,
    /// Optional node id to attach samples to.
    #[serde(default)]
    pub node_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnmpScrape {
    pub name: String,
    /// `host:port` (port defaults to 161 if omitted).
    pub target: String,
    #[serde(default = "default_community")]
    pub community: String,
    #[serde(default = "default_scrape_interval")]
    pub interval_secs: u64,
    #[serde(default)]
    pub node_id: String,
}

fn default_scrape_interval() -> u64 {
    30
}

fn default_community() -> String {
    "public".into()
}
