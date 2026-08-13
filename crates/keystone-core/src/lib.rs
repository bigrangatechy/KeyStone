// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Shared types for KeyStone: metric catalog, config, RBAC, Docker ops.
//!
//! Operator docs live in `docs/src/` (served at `/help`). Internals live in
//! `docs/dev/`. Coverage tests fail if the developer pages miss a catalog
//! name, `DockerOp`, or `Permission`.

pub mod config;
pub mod docker;
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

#[cfg(test)]
mod docs_coverage;
