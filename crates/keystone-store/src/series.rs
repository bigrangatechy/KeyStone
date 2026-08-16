// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use keystone_core::sample::Sample;
use redb::{Database, ReadableTable, TableDefinition};

const LATEST: TableDefinition<&str, &[u8]> = TableDefinition::new("latest");
const SERIES: TableDefinition<&str, f64> = TableDefinition::new("series");

const PRUNE_EVERY_MS: i64 = 60_000;

#[derive(Clone)]
pub struct RedbSeries {
    db: Arc<Database>,
    retention_ms: Arc<AtomicI64>,
    last_prune_ms: Arc<AtomicI64>,
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
            retention_ms: Arc::new(AtomicI64::new(retention_ms(retention_hours))),
            last_prune_ms: Arc::new(AtomicI64::new(0)),
        })
    }

    pub fn set_retention_hours(&self, hours: u32) {
        self.retention_ms
            .store(retention_ms(hours), Ordering::Relaxed);
    }

    pub fn retention_hours(&self) -> u32 {
        (self.retention_ms.load(Ordering::Relaxed) / 3600 / 1000) as u32
    }

    pub fn write_samples(&self, node_id: &str, samples: &[Sample]) -> anyhow::Result<()> {
        <Self as SeriesStore>::write(self, node_id, samples)
    }

    pub fn latest_samples(&self, node_id: &str) -> anyhow::Result<Vec<Sample>> {
        <Self as SeriesStore>::latest(self, node_id)
    }

    /// Points for one series, oldest first, at or after `since_ms`.
    pub fn history(
        &self,
        node_id: &str,
        metric: &str,
        labels_key: &str,
        since_ms: i64,
    ) -> anyhow::Result<Vec<(i64, f64)>> {
        let start = format!("{node_id}\0{metric}\0{labels_key}\0{since_ms}");
        let end = format!("{node_id}\0{metric}\0{labels_key}\0{}", i64::MAX);
        let tx = self.db.begin_read()?;
        let series = tx.open_table(SERIES)?;
        let mut points = Vec::new();
        for entry in series.range(start.as_str()..end.as_str())? {
            let (k, v) = entry?;
            let ts = k
                .value()
                .rsplit('\0')
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            points.push((ts, v.value()));
        }
        Ok(points)
    }

    /// All labeled series for a metric, oldest first per labels_key.
    pub fn history_all(
        &self,
        node_id: &str,
        metric: &str,
        since_ms: i64,
    ) -> anyhow::Result<HashMap<String, Vec<(i64, f64)>>> {
        let start = format!("{node_id}\0{metric}\0");
        let end = format!("{node_id}\0{metric}\u{0001}");
        let tx = self.db.begin_read()?;
        let series = tx.open_table(SERIES)?;
        let mut by_labels: HashMap<String, Vec<(i64, f64)>> = HashMap::new();
        for entry in series.range(start.as_str()..end.as_str())? {
            let (k, v) = entry?;
            let key = k.value();
            let mut parts = key.split('\0');
            let _node = parts.next();
            let _metric = parts.next();
            let labels = parts.next().unwrap_or("").to_string();
            let ts = parts
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            if ts < since_ms {
                continue;
            }
            by_labels.entry(labels).or_default().push((ts, v.value()));
        }
        Ok(by_labels)
    }

    fn prune(&self, tx: &redb::WriteTransaction) -> anyhow::Result<()> {
        let cutoff = now_ms().saturating_sub(self.retention_ms.load(Ordering::Relaxed));
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
        // Full-table prune on every 1s heartbeat blocked ingest from reading
        // CommandResults, so the node page hit agent command timed out.
        let now = now_ms();
        let last = self.last_prune_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= PRUNE_EVERY_MS
            && self
                .last_prune_ms
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            self.prune(&tx)?;
        }
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

fn retention_ms(hours: u32) -> i64 {
    let hours = hours.max(1);
    i64::from(hours) * 3600 * 1000
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
        let ts = now_ms();
        store
            .write(
                "n1",
                &[
                    Sample::new("node_load1", 1.0, ts - 2000),
                    Sample::new("node_load1", 2.0, ts - 1000),
                    Sample::new("node_load1", 3.0, ts),
                ],
            )
            .unwrap();
        let hist = store.history("n1", "node_load1", "", ts - 1500).unwrap();
        assert!(hist.len() >= 2);
        assert!(hist.iter().all(|(t, _)| *t >= ts - 1500));
        store.set_retention_hours(48);
        assert_eq!(store.retention_hours(), 48);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_does_not_full_scan_on_every_push() {
        let dir = std::env::temp_dir().join(format!("ks-series-prune-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = RedbSeries::open(&dir.join("s.redb"), 24).unwrap();
        let ts = now_ms();
        for i in 0..2500 {
            store
                .write(
                    "n1",
                    &[Sample::new("node_load1", 1.0, ts - i64::from(i) * 1000)],
                )
                .unwrap();
        }
        let start = std::time::Instant::now();
        store
            .write("n1", &[Sample::new("node_load1", 2.0, ts)])
            .unwrap();
        assert!(
            start.elapsed() < std::time::Duration::from_millis(400),
            "prune must not scan series.redb on every ingest push, took {:?}",
            start.elapsed()
        );
        let src = include_str!("series.rs");
        assert!(
            src.contains("PRUNE_EVERY_MS"),
            "retention prune belongs on a timer, not the CommandResult path"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
