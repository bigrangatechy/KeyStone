// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Shared types for KeyStone: metric catalog, config, RBAC, Docker ops.
//!
//! Reference documentation is rendered from these types (`docs` module).
//! Do not duplicate metric or permission lists in hand-written markdown.

pub mod config;
pub mod docker;
pub mod docs;
pub mod gpu;
pub mod metrics;
pub mod net;
pub mod node;
pub mod rbac;
pub mod sample;
pub mod settings;
pub mod temp;
pub mod widgets;

pub use config::{
    AgentConfig, DockerConfig, PrometheusScrape, ServerAuth, ServerConfig, SnmpScrape,
};
pub use docker::DockerOp;
pub use metrics::{catalog, is_known_metric, MetricDef, MetricType, Stability};
pub use node::NodeIdentity;
pub use rbac::Permission;
pub use sample::{Label, Sample};
pub use settings::{AgentRuntime, NodeSettings, ServerSettings};
pub use widgets::{
    presets, presets_for_samples, Dashboard, WidgetInstance, WidgetKind, WidgetPreset,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
