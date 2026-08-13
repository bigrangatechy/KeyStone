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
                "create buffer dir {} (need write access; examples use /tmp/keystone/agent-buffer)",
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

    pub fn drain(&self) -> anyhow::Result<Vec<PushFrame>> {
        let mut files: Vec<_> = std::fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bin"))
            .collect();
        files.sort();
        let mut out = Vec::new();
        for path in files {
            let bytes = std::fs::read(&path)?;
            if let Ok(msg) = AgentToServer::decode(bytes.as_slice()) {
                if let Some(keystone_proto::agent_to_server::Body::Push(frame)) = msg.body {
                    out.push(frame);
                }
            }
            let _ = std::fs::remove_file(&path);
        }
        Ok(out)
    }
}
