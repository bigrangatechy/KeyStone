// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Metric catalog. The running agent and server drop names that are not listed
//! here. Keep `docs/dev/src/metrics.md` in sync (coverage test). Operator
//! meaning lives in `docs/src/metrics.md`.

use std::sync::OnceLock;

pub use inventory;

/// How a metric is expected to behave over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    Counter,
    Gauge,
}

/// Whether operators may rely on the name and labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    Stable,
    Experimental,
}

/// One catalog entry. Submitted with [`define_metric!`].
#[derive(Debug, Clone, Copy)]
pub struct MetricDef {
    pub name: &'static str,
    pub metric_type: MetricType,
    pub unit: &'static str,
    pub help: &'static str,
    pub labels: &'static [&'static str],
    pub stability: Stability,
}

inventory::collect!(MetricDef);

/// Register a metric in the allowlist catalog.
#[macro_export]
macro_rules! define_metric {
    (
        name: $name:literal,
        ty: $ty:expr,
        unit: $unit:literal,
        help: $help:literal,
        labels: [$($label:literal),* $(,)?],
        stability: $stab:expr $(,)?
    ) => {
        $crate::metrics::inventory::submit! {
            $crate::metrics::MetricDef {
                name: $name,
                metric_type: $ty,
                unit: $unit,
                help: $help,
                labels: &[$($label),*],
                stability: $stab,
            }
        }
    };
}

mod container;
mod host;
mod snmp;

fn catalog_vec() -> Vec<&'static MetricDef> {
    let mut defs: Vec<&'static MetricDef> = inventory::iter::<MetricDef>.into_iter().collect();
    defs.sort_by_key(|d| d.name);
    defs
}

/// Sorted catalog. Unknown names must not be stored.
pub fn catalog() -> &'static [&'static MetricDef] {
    static CATALOG: OnceLock<Vec<&'static MetricDef>> = OnceLock::new();
    CATALOG.get_or_init(catalog_vec).as_slice()
}

pub fn is_known_metric(name: &str) -> bool {
    catalog().iter().any(|d| d.name == name)
}

pub fn lookup(name: &str) -> Option<&'static MetricDef> {
    catalog().iter().copied().find(|d| d.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_names_are_unique() {
        let mut seen = HashSet::new();
        for def in catalog() {
            assert!(seen.insert(def.name), "duplicate metric {}", def.name);
            assert!(!def.name.is_empty());
            assert!(!def.help.is_empty());
        }
        assert!(!seen.is_empty());
    }
}
