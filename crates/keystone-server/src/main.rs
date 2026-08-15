// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use keystone_core::config::{self, ServerConfig};
use keystone_server::auth;
use keystone_server::cli::{Command, ServerCli};
use keystone_server::state::AppState;
use keystone_server::{help, http, ingest, mdns, scrape, tls};
use keystone_store::Stores;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = ServerCli::parse();
    match cli.command.unwrap_or(Command::Serve {
        config: PathBuf::from("/etc/keystone/server.toml"),
    }) {
        Command::HashPassword => {
            let password = match std::env::var("KEYSTONE_ADMIN_PASSWORD") {
                Ok(p) if !p.is_empty() => p,
                _ => {
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim().to_string()
                }
            };
            println!("{}", auth::hash_password(&password)?);
            Ok(())
        }
        Command::Docs { section } => {
            if section == "all" {
                print!("{}", help::all_markdown());
            } else {
                let Some(sec) = help::section_by_slug(&section) else {
                    let slugs: Vec<_> = help::sections().into_iter().map(|s| s.slug).collect();
                    anyhow::bail!(
                        "unknown docs section {section}; try one of: all, {}",
                        slugs.join(", ")
                    );
                };
                print!("{}", sec.markdown);
            }
            Ok(())
        }
        Command::Serve { config } => serve(config).await,
    }
}

async fn serve(config_path: PathBuf) -> anyhow::Result<()> {
    let cfg: ServerConfig = if config_path.exists() {
        config::load_toml(&config_path)
            .with_context(|| format!("config {}", config_path.display()))?
    } else {
        tracing::warn!(
            "config {} missing, using defaults (data in .smoke)",
            config_path.display()
        );
        ServerConfig {
            data_dir: ".smoke".into(),
            ..ServerConfig::default()
        }
    };
    std::fs::create_dir_all(&cfg.data_dir)?;
    let stores = Stores::open(std::path::Path::new(&cfg.data_dir), cfg.retention_hours)?;
    auth::ensure_admin(
        &stores.metadata,
        &cfg.auth.username,
        &cfg.auth.password_hash,
    )?;
    let http_addr = cfg.http_addr().map_err(|e| anyhow::anyhow!(e))?;
    let grpc_addr = cfg.grpc_addr().map_err(|e| anyhow::anyhow!(e))?;
    tls::install_provider();
    let ingest_tls = cfg.tls.ingest;
    let pem = tls::TlsPem::from_config(&cfg.tls)?;
    let http_tls = pem.is_some();
    let grpc_pem = if ingest_tls { pem.clone() } else { None };
    if http_tls {
        tracing::info!("TLS cert {} key {}", cfg.tls.cert_file, cfg.tls.key_file);
    }
    let grpc_listen = cfg.grpc_listen.clone();
    let state = AppState::new(cfg, stores);
    state.seed_server_settings()?;
    scrape::spawn(state.clone());

    let http_scheme = if http_tls { "https" } else { "http" };
    tracing::info!("HTTP UI on {http_scheme}://{http_addr}");
    if grpc_pem.is_some() {
        tracing::info!("gRPC ingest TLS on {grpc_addr}");
    } else {
        tracing::info!("gRPC ingest on {grpc_addr} (plaintext)");
    }
    mdns::advertise_ingest(&grpc_listen, ingest_tls);

    let app = http::router(state.clone());
    let http = tls::serve_http(http_addr, app, pem.as_ref());
    let grpc = async move {
        let mut builder = tonic::transport::Server::builder();
        if let Some(t) = grpc_pem.as_ref() {
            builder = builder
                .tls_config(tls::grpc_tls_config(t)?)
                .context("gRPC TLS")?;
        }
        builder
            .add_service(ingest::service(state))
            .serve(grpc_addr)
            .await
            .with_context(|| keystone_core::listen_bind_context("gRPC ingest", grpc_addr))
    };

    tokio::select! {
        r = http => r,
        r = grpc => r,
    }
}
