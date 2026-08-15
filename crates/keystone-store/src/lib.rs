// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

mod metadata;
mod series;

pub use metadata::{AuditEvent, Metadata, NodeRecord, SessionRecord, TotpRecord};
pub use series::{RedbSeries, SeriesStore};

use std::path::Path;

use anyhow::Context;

#[derive(Clone)]
pub struct Stores {
    pub metadata: Metadata,
    pub series: RedbSeries,
}

impl Stores {
    pub fn open(data_dir: &Path, retention_hours: u32) -> anyhow::Result<Self> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("create data dir {}", data_dir.display()))?;
        let metadata = Metadata::open(&data_dir.join("keystone.sqlite"))?;
        let series = RedbSeries::open(&data_dir.join("series.redb"), retention_hours)?;
        Ok(Self { metadata, series })
    }
}
