//! 受信したイベントを JSON Lines で追記保存する (蓄積・後からの解析用)。

use std::path::PathBuf;

use crate::quake::Event;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonlConfig {
    path: PathBuf,
}

pub struct JsonlSink {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JsonlSink {
    pub fn new(cfg: JsonlConfig) -> Self {
        JsonlSink {
            path: cfg.path,
            lock: Mutex::new(()),
        }
    }
}

impl JsonlSink {
    pub async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        let mut line = serde_json::to_string(ev)?;
        line.push('\n');
        let _g = self.lock.lock().await;
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(dir).await?;
        }
        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        f.write_all(line.as_bytes()).await?;
        Ok(())
    }
}
