// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "keystone-agent",
    about = "KeyStone node agent: push catalog metrics and optional Docker control",
    version
)]
pub struct AgentCli {
    /// Path to TOML config
    #[arg(
        short,
        long,
        env = "KEYSTONE_AGENT_CONFIG",
        default_value = "/etc/keystone/agent.toml"
    )]
    pub config: PathBuf,
}

pub fn markdown_help() -> String {
    clap_markdown::help_markdown::<AgentCli>()
}
