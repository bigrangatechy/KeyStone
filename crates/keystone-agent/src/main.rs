// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use anyhow::Context;
use clap::Parser;
use keystone_agent::cli::AgentCli;
use keystone_core::config::{self, AgentConfig};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = AgentCli::parse();
    let mut cfg: AgentConfig = if cli.config.exists() {
        config::load_toml(&cli.config)
            .with_context(|| format!("config {}", cli.config.display()))?
    } else {
        tracing::warn!(
            "config {} missing, using defaults (localhost ingest)",
            cli.config.display()
        );
        AgentConfig::default()
    };
    if let Ok(token) = std::env::var("KEYSTONE_INGEST_TOKEN") {
        if !token.is_empty() {
            cfg.ingest_token = token;
        }
    }
    keystone_agent::session::run(cfg).await
}
