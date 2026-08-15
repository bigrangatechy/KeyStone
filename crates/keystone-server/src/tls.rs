// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Optional rustls for the HTTP UI and gRPC ingest.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use axum::Router;
use keystone_core::config::TlsConfig;
use tonic::transport::{Identity, ServerTlsConfig};

pub fn install_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Clone)]
pub struct TlsPem {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
}

impl TlsPem {
    pub fn from_config(tls: &TlsConfig) -> anyhow::Result<Option<Self>> {
        match tls.pem_paths() {
            Ok(None) => Ok(None),
            Err(e) => anyhow::bail!("{e}"),
            Ok(Some((cert, key))) => {
                anyhow::ensure!(Path::new(cert).is_file(), "tls.cert_file not found: {cert}");
                anyhow::ensure!(Path::new(key).is_file(), "tls.key_file not found: {key}");
                Ok(Some(Self {
                    cert_file: PathBuf::from(cert),
                    key_file: PathBuf::from(key),
                }))
            }
        }
    }

    pub fn tonic_identity(&self) -> anyhow::Result<Identity> {
        let cert = std::fs::read(&self.cert_file)
            .with_context(|| format!("read {}", self.cert_file.display()))?;
        let key = std::fs::read(&self.key_file)
            .with_context(|| format!("read {}", self.key_file.display()))?;
        Ok(Identity::from_pem(cert, key))
    }
}

pub async fn serve_http(addr: SocketAddr, app: Router, tls: Option<&TlsPem>) -> anyhow::Result<()> {
    match tls {
        None => {
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("bind HTTP {addr}"))?;
            axum::serve(listener, app).await.context("HTTP UI")
        }
        Some(tls) => {
            let config =
                axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert_file, &tls.key_file)
                    .await
                    .with_context(|| {
                        format!(
                            "load TLS {} + {}",
                            tls.cert_file.display(),
                            tls.key_file.display()
                        )
                    })?;
            axum_server::bind_rustls(addr, config)
                .serve(app.into_make_service())
                .await
                .context("HTTPS UI")
        }
    }
}

pub fn grpc_tls_config(tls: &TlsPem) -> anyhow::Result<ServerTlsConfig> {
    Ok(ServerTlsConfig::new().identity(tls.tonic_identity()?))
}
