// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use keystone_core::config::{self, ServerConfig};
use keystone_core::docs;
use keystone_server::auth;
use keystone_server::cli::{Command, ServerCli};
use keystone_server::state::AppState;
use keystone_server::{help, http, ingest, scrape};
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
                match section.as_str() {
                    "metrics" => print!("{}", docs::metrics_markdown()),
                    "permissions" => print!("{}", docs::permissions_markdown()),
                    "docker" => print!("{}", docs::docker_ops_markdown()),
                    "widgets" => print!("{}", docs::widgets_markdown()),
                    "agent-config" => print!("{}", docs::agent_config_markdown()),
                    "server-config" => print!("{}", docs::server_config_markdown()),
                    other => anyhow::bail!("unknown docs section {other}"),
                }
            }
            Ok(())
        }
        Command::Serve { config } => serve(config).await,
    }
}

async fn serve(config_path: PathBuf) -> anyhow::Result<()> {
    let mut cfg: ServerConfig = if config_path.exists() {
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
    if let Ok(token) = std::env::var("KEYSTONE_INGEST_TOKEN") {
        if !token.is_empty() {
            cfg.ingest_token = token;
        }
    }

    std::fs::create_dir_all(&cfg.data_dir)?;
    let stores = Stores::open(std::path::Path::new(&cfg.data_dir), cfg.retention_hours)?;
    auth::ensure_admin(
        &stores.metadata,
        &cfg.auth.username,
        &cfg.auth.password_hash,
    )?;
    let http_addr: SocketAddr = cfg.http_listen.parse().context("http_listen")?;
    let grpc_addr: SocketAddr = cfg.grpc_listen.parse().context("grpc_listen")?;
    let state = AppState::new(cfg, stores);
    scrape::spawn(state.clone());

    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    tracing::info!("HTTP UI on http://{http_addr}");
    tracing::info!("gRPC ingest on {grpc_addr}");

    let http = axum::serve(listener, http::router(state.clone()));
    let grpc = tonic::transport::Server::builder()
        .add_service(ingest::service(state))
        .serve(grpc_addr);

    tokio::select! {
        r = http => r.context("http server")?,
        r = grpc => r.context("grpc server")?,
    }
    Ok(())
}
