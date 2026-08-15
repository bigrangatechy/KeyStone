// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "keystone",
    about = "KeyStone server: homelab metrics and per-node Docker (Portainer + Netdata)",
    version
)]
pub struct ServerCli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the HTTP UI and gRPC ingest (default)
    Serve {
        /// Path to TOML config
        #[arg(
            short,
            long,
            env = "KEYSTONE_SERVER_CONFIG",
            default_value = "/etc/keystone/server.toml"
        )]
        config: PathBuf,
    },
    /// Print operator documentation (same text as /help) to stdout
    Docs {
        /// Chapter slug (`introduction`, `install`, …) or `all`
        #[arg(long, default_value = "all")]
        section: String,
    },
    /// Hash a password for `auth.password_hash` (reads KEYSTONE_ADMIN_PASSWORD or prompt via stdin)
    HashPassword,
}
