//! 受信したイベントを JSON Lines で追記保存する (蓄積・後からの解析用)。

use std::path::PathBuf;

use crate::archive_store;
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
    /// 設定の path の隣の、書いた時刻 (UTC) の日付のファイルに追記する (archive_store)
    pub async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        self.write_at(ev, crate::hub::now_ms()).await
    }

    async fn write_at(&self, ev: &Event, now_ms: u64) -> anyhow::Result<()> {
        let mut line = serde_json::to_string(ev)?;
        line.push('\n');
        let path = archive_store::daily_path(&self.path, now_ms);
        let _g = self.lock.lock().await;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(dir).await?;
        }
        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        f.write_all(line.as_bytes()).await?;
        f.flush().await?; // tokio の File は drop だけでは書き終わりを待たない
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str) -> Event {
        serde_json::from_str(&format!(
            r#"{{"id":"{id}","source":"wolfx","received_at_ms":1,"kind":"eew_detection","detection_type":"Full"}}"#
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn writes_to_the_file_of_the_write_time_date_not_the_event_time() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("sub/events.jsonl");
        let sink = JsonlSink::new(JsonlConfig { path: base.clone() });
        let day = 86_400_000u64;
        let t = 19_723 * day; // 2024-01-01T00:00:00Z
        sink.write_at(&ev("a"), t - 1).await.unwrap();
        sink.write_at(&ev("b"), t).await.unwrap();
        sink.write_at(&ev("c"), t + 5).await.unwrap();
        let read = |n: &str| std::fs::read_to_string(dir.path().join("sub").join(n)).unwrap();
        assert_eq!(read("events-2023-12-31.jsonl").lines().count(), 1);
        assert_eq!(read("events-2024-01-01.jsonl").lines().count(), 2);
        assert!(!base.exists());
    }
}
