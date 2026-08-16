// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use std::path::PathBuf;

use anyhow::Context;
use keystone_proto::{AgentToServer, PushFrame};
use prost::Message;

/// Length-delimited protobuf frames on disk when the server is down.
pub struct DiskBuffer {
    dir: PathBuf,
}

impl DiskBuffer {
    pub fn new(dir: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).with_context(|| {
            format!(
                "create buffer dir {} (need write access; examples use .smoke/agent-buffer)",
                dir.display()
            )
        })?;
        Ok(Self { dir })
    }

    pub fn push(&self, frame: &PushFrame) -> anyhow::Result<()> {
        let msg = AgentToServer {
            body: Some(keystone_proto::agent_to_server::Body::Push(frame.clone())),
        };
        let bytes = msg.encode_to_vec();
        let name = format!(
            "{:020}-{}.bin",
            chrono::Utc::now().timestamp_millis(),
            uuid::Uuid::new_v4()
        );
        std::fs::write(self.dir.join(name), bytes)?;
        Ok(())
    }

    /// Oldest buffered push, if any. Used so reconnect does not dump the
    /// whole disk queue onto the ingest stream ahead of CommandResults.
    pub fn pop_oldest(&self) -> anyhow::Result<Option<PushFrame>> {
        let mut files: Vec<_> = std::fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bin"))
            .collect();
        files.sort();
        let Some(path) = files.into_iter().next() else {
            return Ok(None);
        };
        let bytes = std::fs::read(&path)?;
        let _ = std::fs::remove_file(&path);
        if let Ok(msg) = AgentToServer::decode(bytes.as_slice()) {
            if let Some(keystone_proto::agent_to_server::Body::Push(frame)) = msg.body {
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keystone_proto::Heartbeat;

    fn frame(n: &str) -> PushFrame {
        PushFrame {
            heartbeat: Some(Heartbeat {
                node_id: n.into(),
                hostname: n.into(),
                agent_version: "test".into(),
                os: "linux".into(),
                kernel: "test".into(),
                docker_version: String::new(),
                labels: vec![],
            }),
            samples: vec![],
            ingest_token: "t".into(),
        }
    }

    #[test]
    fn pop_oldest_is_fifo_and_leaves_the_rest() {
        let dir = std::env::temp_dir().join(format!("ks-buf-{}", uuid::Uuid::new_v4()));
        let buf = DiskBuffer::new(&dir).unwrap();
        buf.push(&frame("a")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        buf.push(&frame("b")).unwrap();
        let first = buf.pop_oldest().unwrap().expect("a");
        assert_eq!(first.heartbeat.unwrap().node_id, "a");
        let second = buf.pop_oldest().unwrap().expect("b");
        assert_eq!(second.heartbeat.unwrap().node_id, "b");
        assert!(buf.pop_oldest().unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
