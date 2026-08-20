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
pub struct TotpRecord {
    pub secret: String,
    pub pending: String,
    pub enabled: bool,
    pub backup_json: String,
    pub last_step: i64,
}

impl Default for TotpRecord {
    fn default() -> Self {
        Self {
            secret: String::new(),
            pending: String::new(),
            enabled: false,
            backup_json: "[]".into(),
            last_step: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: String,
    pub username: String,
    pub expires_unix: i64,
    pub pending_2fa: bool,
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
                password_hash TEXT NOT NULL,
                must_change_password INTEGER NOT NULL DEFAULT 0,
                totp_secret TEXT NOT NULL DEFAULT '',
                totp_pending TEXT NOT NULL DEFAULT '',
                totp_enabled INTEGER NOT NULL DEFAULT 0,
                totp_backup_json TEXT NOT NULL DEFAULT '[]',
                totp_last_step INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                expires_unix INTEGER NOT NULL,
                pending_2fa INTEGER NOT NULL DEFAULT 0
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
            CREATE TABLE IF NOT EXISTS kv (
                k TEXT PRIMARY KEY,
                v TEXT NOT NULL
            );
            ",
        )?;
        let _ = conn.execute("ALTER TABLE nodes ADD COLUMN dashboard_json TEXT", []);
        let _ = conn.execute("ALTER TABLE nodes ADD COLUMN settings_json TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE users ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE users ADD COLUMN totp_secret TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE users ADD COLUMN totp_pending TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE users ADD COLUMN totp_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE users ADD COLUMN totp_backup_json TEXT NOT NULL DEFAULT '[]'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE users ADD COLUMN totp_last_step INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE sessions ADD COLUMN pending_2fa INTEGER NOT NULL DEFAULT 0",
            [],
        );
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn set_user_password(
        &self,
        username: &str,
        password_hash: &str,
        must_change: bool,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO users (username, password_hash, must_change_password)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(username) DO UPDATE SET
                password_hash = excluded.password_hash,
                must_change_password = excluded.must_change_password",
            params![username, password_hash, must_change as i64],
        )?;
        Ok(())
    }

    pub fn user_must_change_password(&self, username: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock();
        let flag: Option<i64> = conn
            .query_row(
                "SELECT must_change_password FROM users WHERE username = ?1",
                params![username],
                |r| r.get(0),
            )
            .optional()?;
        Ok(flag.unwrap_or(0) != 0)
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

    pub fn user_totp_enabled(&self, username: &str) -> anyhow::Result<bool> {
        Ok(self
            .user_totp(username)?
            .map(|t| t.enabled)
            .unwrap_or(false))
    }

    pub fn user_totp(&self, username: &str) -> anyhow::Result<Option<TotpRecord>> {
        let conn = self.conn.lock();
        let rec = conn
            .query_row(
                "SELECT totp_secret, totp_pending, totp_enabled, totp_backup_json, totp_last_step
                 FROM users WHERE username = ?1",
                params![username],
                |r| {
                    Ok(TotpRecord {
                        secret: r.get(0)?,
                        pending: r.get(1)?,
                        enabled: r.get::<_, i64>(2)? != 0,
                        backup_json: r.get(3)?,
                        last_step: r.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(rec)
    }

    pub fn set_user_totp(&self, username: &str, totp: &TotpRecord) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE users SET totp_secret = ?1, totp_pending = ?2, totp_enabled = ?3,
                    totp_backup_json = ?4, totp_last_step = ?5
             WHERE username = ?6",
            params![
                totp.secret,
                totp.pending,
                totp.enabled as i64,
                totp.backup_json,
                totp.last_step,
                username
            ],
        )?;
        if n == 0 {
            anyhow::bail!("unknown user {username}");
        }
        Ok(())
    }

    pub fn put_session(
        &self,
        id: &str,
        username: &str,
        expires_unix: i64,
        pending_2fa: bool,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO sessions (id, username, expires_unix, pending_2fa)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, username, expires_unix, pending_2fa as i64],
        )?;
        Ok(())
    }

    pub fn get_session(&self, id: &str) -> anyhow::Result<Option<SessionRecord>> {
        let now = now_unix();
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE expires_unix < ?1", params![now])?;
        let rec = conn
            .query_row(
                "SELECT id, username, expires_unix, pending_2fa FROM sessions WHERE id = ?1",
                params![id],
                |r| {
                    Ok(SessionRecord {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        expires_unix: r.get(2)?,
                        pending_2fa: r.get::<_, i64>(3)? != 0,
                    })
                },
            )
            .optional()?;
        Ok(rec)
    }

    /// Sliding idle expiry for a finished login. `pending_2fa` rows stay on
    /// their original clock (password step is five minutes).
    pub fn touch_session(&self, id: &str, expires_unix: i64) -> anyhow::Result<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE sessions SET expires_unix = ?1 WHERE id = ?2 AND pending_2fa = 0",
            params![expires_unix, id],
        )?;
        Ok(n > 0)
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
        let limit = limit.clamp(1, 500);
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

    /// Enrol a node from the UI before its agent has connected. No seat limit.
    pub fn register_node(
        &self,
        node_id: &str,
        hostname: &str,
        labels_json: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO nodes (
                node_id, hostname, agent_version, os, kernel, docker_version,
                labels_json, last_seen_unix, online
             ) VALUES (?1, ?2, '', 'awaiting-agent', '', NULL, ?3, 0, 0)
             ON CONFLICT(node_id) DO UPDATE SET
                hostname = excluded.hostname
                WHERE nodes.last_seen_unix = 0",
            params![node_id, hostname, labels_json],
        )?;
        Ok(())
    }

    pub fn node_dashboard_json(&self, node_id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock();
        let json = conn
            .query_row(
                "SELECT dashboard_json FROM nodes WHERE node_id = ?1",
                params![node_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(json)
    }

    pub fn set_node_dashboard_json(&self, node_id: &str, json: Option<&str>) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE nodes SET dashboard_json = ?1 WHERE node_id = ?2",
            params![json, node_id],
        )?;
        if n == 0 {
            anyhow::bail!("node not found");
        }
        Ok(())
    }

    pub fn node_settings_json(&self, node_id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock();
        let json = conn
            .query_row(
                "SELECT settings_json FROM nodes WHERE node_id = ?1",
                params![node_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(json)
    }

    pub fn set_node_settings_json(&self, node_id: &str, json: Option<&str>) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE nodes SET settings_json = ?1 WHERE node_id = ?2",
            params![json, node_id],
        )?;
        if n == 0 {
            anyhow::bail!("node not found");
        }
        Ok(())
    }

    pub fn kv_get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock();
        let v = conn
            .query_row("SELECT v FROM kv WHERE k = ?1", params![key], |r| r.get(0))
            .optional()?;
        Ok(v)
    }

    pub fn kv_set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO kv (k, v) VALUES (?1, ?2)
             ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            params![key, value],
        )?;
        Ok(())
    }
}

impl NodeRecord {
    pub fn last_seen(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.last_seen_unix, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
    }

    pub fn awaiting_agent(&self) -> bool {
        self.last_seen_unix == 0
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
        db.register_node("pi-hole", "pi-hole", "[]").unwrap();
        assert_eq!(db.list_nodes().unwrap().len(), 51);
        let pending = db.get_node("pi-hole").unwrap().unwrap();
        assert!(pending.awaiting_agent());
        db.upsert_heartbeat(
            &NodeIdentity {
                node_id: "pi-hole".into(),
                hostname: "pi-hole".into(),
                agent_version: "0.1.0".into(),
                os: "linux".into(),
                kernel: "1".into(),
                docker_version: None,
                labels: vec![],
            },
            true,
        )
        .unwrap();
        assert!(!db.get_node("pi-hole").unwrap().unwrap().awaiting_agent());
        db.set_node_dashboard_json("pi-hole", Some(r#"{"version":1,"widgets":[]}"#))
            .unwrap();
        assert!(db
            .node_dashboard_json("pi-hole")
            .unwrap()
            .unwrap()
            .contains("widgets"));
        db.kv_set("server", r#"{"retention_hours":24}"#).unwrap();
        assert!(db.kv_get("server").unwrap().unwrap().contains("24"));
        db.set_user_password("admin", "hash", true).unwrap();
        assert!(db.user_must_change_password("admin").unwrap());
        db.set_user_password("admin", "hash2", false).unwrap();
        assert!(!db.user_must_change_password("admin").unwrap());
        assert!(!db.user_totp_enabled("admin").unwrap());
        let mut totp = db.user_totp("admin").unwrap().unwrap();
        totp.enabled = true;
        totp.secret = "MFRGGZDFMZTWQ2LK".into();
        db.set_user_totp("admin", &totp).unwrap();
        assert!(db.user_totp_enabled("admin").unwrap());
        db.set_user_password("admin", "hash3", false).unwrap();
        assert!(db.user_totp_enabled("admin").unwrap());
        db.put_session("sid", "admin", now_unix() + 60, true)
            .unwrap();
        let sess = db.get_session("sid").unwrap().unwrap();
        assert!(sess.pending_2fa);
        let pending_exp = sess.expires_unix;
        assert!(
            !db.touch_session("sid", now_unix() + 7200).unwrap(),
            "idle slide must not extend a pending 2FA row"
        );
        assert_eq!(
            db.get_session("sid").unwrap().unwrap().expires_unix,
            pending_exp
        );
        db.put_session("live", "admin", now_unix() + 60, false)
            .unwrap();
        let later = now_unix() + 7200;
        assert!(db.touch_session("live", later).unwrap());
        assert_eq!(db.get_session("live").unwrap().unwrap().expires_unix, later);
        db.put_session("stale", "admin", now_unix() - 1, false)
            .unwrap();
        assert!(
            db.get_session("stale").unwrap().is_none(),
            "expired sessions must be dropped on read"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_audit_is_newest_first() {
        let dir = std::env::temp_dir().join(format!("ks-audit-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Metadata::open(&dir.join("t.sqlite")).unwrap();
        db.audit("admin", "ranga", "container_start", "abc", true, "ok")
            .unwrap();
        db.audit("admin", "ranga", "container_stop", "abc", false, "timeout")
            .unwrap();
        let log = db.recent_audit(10).unwrap();
        assert_eq!(log[0].op, "container_stop");
        assert!(!log[0].ok);
        assert_eq!(log[0].detail, "timeout");
        assert_eq!(log[1].op, "container_start");
        assert!(log[1].ok);
        assert_eq!(db.recent_audit(1).unwrap().len(), 1);
        assert_eq!(db.recent_audit(0).unwrap().len(), 1);
        db.audit("admin", "pi", "net_set", r#"{"iface":"eth0"}"#, true, "ok")
            .unwrap();
        let many = db.recent_audit(5000).unwrap();
        assert_eq!(many.len(), 3);
        assert_eq!(many[0].node_id, "pi");
        assert_eq!(many[0].target, r#"{"iface":"eth0"}"#);
        assert_eq!(many[0].username, "admin");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn uuid_like() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }
}
