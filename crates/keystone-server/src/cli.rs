// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "keystone",
    about = "KeyStone server: unlimited-node monitoring, per-node Docker, living /help",
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
    /// Print living documentation (same text as /help) to stdout
    Docs {
        #[arg(long, default_value = "all")]
        section: String,
    },
    /// Hash a password for `auth.password_hash` (reads KEYSTONE_ADMIN_PASSWORD or prompt via stdin)
    HashPassword,
}

pub fn markdown_help() -> String {
    clap_markdown::help_markdown::<ServerCli>()
}
