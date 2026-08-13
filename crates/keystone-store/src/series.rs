// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use keystone_core::sample::Sample;
use redb::{Database, ReadableTable, TableDefinition};

const LATEST: TableDefinition<&str, &[u8]> = TableDefinition::new("latest");
const SERIES: TableDefinition<&str, f64> = TableDefinition::new("series");

#[derive(Clone)]
pub struct RedbSeries {
    db: Arc<Database>,
    retention_ms: i64,
}

pub trait SeriesStore {
    fn write(&self, node_id: &str, samples: &[Sample]) -> anyhow::Result<()>;
    fn latest(&self, node_id: &str) -> anyhow::Result<Vec<Sample>>;
}

impl RedbSeries {
    pub fn open(path: &Path, retention_hours: u32) -> anyhow::Result<Self> {
        let db = Database::create(path).with_context(|| format!("create {}", path.display()))?;
        {
            let tx = db.begin_write()?;
            let _ = tx.open_table(LATEST)?;
            let _ = tx.open_table(SERIES)?;
            tx.commit()?;
        }
        Ok(Self {
            db: Arc::new(db),
            retention_ms: i64::from(retention_hours) * 3600 * 1000,
        })
    }

    pub fn write_samples(&self, node_id: &str, samples: &[Sample]) -> anyhow::Result<()> {
        <Self as SeriesStore>::write(self, node_id, samples)
    }

    pub fn latest_samples(&self, node_id: &str) -> anyhow::Result<Vec<Sample>> {
        <Self as SeriesStore>::latest(self, node_id)
    }

    fn prune(&self, tx: &redb::WriteTransaction) -> anyhow::Result<()> {
        let cutoff = now_ms().saturating_sub(self.retention_ms);
        let mut series = tx.open_table(SERIES)?;
        let keys: Vec<String> = series
            .iter()?
            .filter_map(|r| r.ok())
            .filter_map(|(k, _)| {
                let key = k.value();
                let ts = key.rsplit('\0').next()?.parse::<i64>().ok()?;
                if ts < cutoff {
                    Some(key.to_string())
                } else {
                    None
                }
            })
            .collect();
        for k in keys {
            series.remove(k.as_str())?;
        }
        Ok(())
    }
}

impl SeriesStore for RedbSeries {
    fn write(&self, node_id: &str, samples: &[Sample]) -> anyhow::Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut latest = tx.open_table(LATEST)?;
            let mut grouped: Vec<Sample> = latest
                .get(node_id)?
                .map(|v| serde_json::from_slice(v.value()).unwrap_or_default())
                .unwrap_or_default();
            for s in samples {
                if let Some(existing) = grouped
                    .iter_mut()
                    .find(|e| e.metric == s.metric && e.labels_key() == s.labels_key())
                {
                    *existing = s.clone();
                } else {
                    grouped.push(s.clone());
                }
            }
            let encoded = serde_json::to_vec(&grouped)?;
            latest.insert(node_id, encoded.as_slice())?;
        }
        {
            let mut series = tx.open_table(SERIES)?;
            for s in samples {
                let key = format!(
                    "{node_id}\0{}\0{}\0{}",
                    s.metric,
                    s.labels_key(),
                    s.timestamp_unix_ms
                );
                series.insert(key.as_str(), s.value)?;
            }
        }
        self.prune(&tx)?;
        tx.commit()?;
        Ok(())
    }

    fn latest(&self, node_id: &str) -> anyhow::Result<Vec<Sample>> {
        let tx = self.db.begin_read()?;
        let latest = tx.open_table(LATEST)?;
        match latest.get(node_id)? {
            Some(v) => Ok(serde_json::from_slice(v.value()).unwrap_or_default()),
            None => Ok(Vec::new()),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keystone_core::sample::Sample;

    #[test]
    fn write_and_latest() {
        let dir = std::env::temp_dir().join(format!("ks-series-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = RedbSeries::open(&dir.join("s.redb"), 24).unwrap();
        store
            .write("n1", &[Sample::new("node_load1", 1.5, now_ms())])
            .unwrap();
        let got = store.latest("n1").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].value, 1.5);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
