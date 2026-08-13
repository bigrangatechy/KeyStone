// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use chrono::{DateTime, Utc};
use keystone_core::node::NodeIdentity;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub node_id: String,
    pub hostname: String,
    pub agent_version: String,
    pub os: String,
    pub kernel: String,
    pub docker_version: Option<String>,
    pub labels_json: String,
    pub last_seen_unix: i64,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: i64,
    pub at_unix: i64,
    pub username: String,
    pub node_id: String,
    pub op: String,
    pub target: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: String,
    pub username: String,
    pub expires_unix: i64,
}

#[derive(Clone)]
pub struct Metadata {
    conn: Arc<Mutex<Connection>>,
}

impl Metadata {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS nodes (
                node_id TEXT PRIMARY KEY,
                hostname TEXT NOT NULL,
                agent_version TEXT NOT NULL,
                os TEXT NOT NULL,
                kernel TEXT NOT NULL,
                docker_version TEXT,
                labels_json TEXT NOT NULL,
                last_seen_unix INTEGER NOT NULL,
                online INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS users (
                username TEXT PRIMARY KEY,
                password_hash TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                expires_unix INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                at_unix INTEGER NOT NULL,
                username TEXT NOT NULL,
                node_id TEXT NOT NULL,
                op TEXT NOT NULL,
                target TEXT NOT NULL,
                ok INTEGER NOT NULL,
                detail TEXT NOT NULL
            );
            ",
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn upsert_user(&self, username: &str, password_hash: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO users (username, password_hash) VALUES (?1, ?2)
             ON CONFLICT(username) DO UPDATE SET password_hash = excluded.password_hash",
            params![username, password_hash],
        )?;
        Ok(())
    }

    pub fn user_hash(&self, username: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock();
        let hash = conn
            .query_row(
                "SELECT password_hash FROM users WHERE username = ?1",
                params![username],
                |r| r.get(0),
            )
            .optional()?;
        Ok(hash)
    }

    pub fn put_session(&self, id: &str, username: &str, expires_unix: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO sessions (id, username, expires_unix) VALUES (?1, ?2, ?3)",
            params![id, username, expires_unix],
        )?;
        Ok(())
    }

    pub fn get_session(&self, id: &str) -> anyhow::Result<Option<SessionRecord>> {
        let now = now_unix();
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE expires_unix < ?1", params![now])?;
        let rec = conn
            .query_row(
                "SELECT id, username, expires_unix FROM sessions WHERE id = ?1",
                params![id],
                |r| {
                    Ok(SessionRecord {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        expires_unix: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(rec)
    }

    pub fn delete_session(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn upsert_heartbeat(&self, id: &NodeIdentity, online: bool) -> anyhow::Result<()> {
        let labels = serde_json::to_string(&id.labels).unwrap_or_else(|_| "[]".into());
        let now = now_unix();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO nodes (
                node_id, hostname, agent_version, os, kernel, docker_version,
                labels_json, last_seen_unix, online
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(node_id) DO UPDATE SET
                hostname=excluded.hostname,
                agent_version=excluded.agent_version,
                os=excluded.os,
                kernel=excluded.kernel,
                docker_version=excluded.docker_version,
                labels_json=excluded.labels_json,
                last_seen_unix=excluded.last_seen_unix,
                online=excluded.online",
            params![
                id.node_id,
                id.hostname,
                id.agent_version,
                id.os,
                id.kernel,
                id.docker_version,
                labels,
                now,
                online as i64
            ],
        )?;
        Ok(())
    }

    pub fn set_online(&self, node_id: &str, online: bool) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE nodes SET online = ?1 WHERE node_id = ?2",
            params![online as i64, node_id],
        )?;
        Ok(())
    }

    pub fn list_nodes(&self) -> anyhow::Result<Vec<NodeRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT node_id, hostname, agent_version, os, kernel, docker_version,
                    labels_json, last_seen_unix, online
             FROM nodes ORDER BY hostname COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(NodeRecord {
                node_id: r.get(0)?,
                hostname: r.get(1)?,
                agent_version: r.get(2)?,
                os: r.get(3)?,
                kernel: r.get(4)?,
                docker_version: r.get(5)?,
                labels_json: r.get(6)?,
                last_seen_unix: r.get(7)?,
                online: r.get::<_, i64>(8)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_node(&self, node_id: &str) -> anyhow::Result<Option<NodeRecord>> {
        let conn = self.conn.lock();
        let rec = conn
            .query_row(
                "SELECT node_id, hostname, agent_version, os, kernel, docker_version,
                        labels_json, last_seen_unix, online
                 FROM nodes WHERE node_id = ?1",
                params![node_id],
                |r| {
                    Ok(NodeRecord {
                        node_id: r.get(0)?,
                        hostname: r.get(1)?,
                        agent_version: r.get(2)?,
                        os: r.get(3)?,
                        kernel: r.get(4)?,
                        docker_version: r.get(5)?,
                        labels_json: r.get(6)?,
                        last_seen_unix: r.get(7)?,
                        online: r.get::<_, i64>(8)? != 0,
                    })
                },
            )
            .optional()?;
        Ok(rec)
    }

    pub fn audit(
        &self,
        username: &str,
        node_id: &str,
        op: &str,
        target: &str,
        ok: bool,
        detail: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO audit (at_unix, username, node_id, op, target, ok, detail)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![now_unix(), username, node_id, op, target, ok as i64, detail],
        )?;
        Ok(())
    }

    pub fn recent_audit(&self, limit: i64) -> anyhow::Result<Vec<AuditEvent>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, at_unix, username, node_id, op, target, ok, detail
             FROM audit ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok(AuditEvent {
                id: r.get(0)?,
                at_unix: r.get(1)?,
                username: r.get(2)?,
                node_id: r.get(3)?,
                op: r.get(4)?,
                target: r.get(5)?,
                ok: r.get::<_, i64>(6)? != 0,
                detail: r.get(7)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

impl NodeRecord {
    pub fn last_seen(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.last_seen_unix, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keystone_core::node::NodeIdentity;

    #[test]
    fn no_node_cap_on_registry() {
        let dir = std::env::temp_dir().join(format!("ks-meta-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Metadata::open(&dir.join("t.sqlite")).unwrap();
        for i in 0..50 {
            let id = NodeIdentity {
                node_id: format!("n{i}"),
                hostname: format!("h{i}"),
                agent_version: "0.1.0".into(),
                os: "linux".into(),
                kernel: "1".into(),
                docker_version: None,
                labels: vec![],
            };
            db.upsert_heartbeat(&id, true).unwrap();
        }
        assert_eq!(db.list_nodes().unwrap().len(), 50);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn uuid_like() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }
}
