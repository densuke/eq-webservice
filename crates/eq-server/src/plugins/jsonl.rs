//! 受信したイベントを JSON Lines で追記保存する (蓄積・後からの解析用)。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::archive_store;
use crate::quake::Event;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonlConfig {
    path: PathBuf,
}

/// 書けなかったときに、やり直すまでの待ち。使い切ったら退避先へ。
const RETRY_DELAYS: [Duration; 3] = [Duration::from_secs(1), Duration::from_secs(5), Duration::from_secs(30)];

/// 記録の sink。handle は sink ごとの 1 本のタスクから直列にしか呼ばれない (plugins::spawn)。
/// やり直しの間は次の報を待たせるので、順序は入れ替わらない (外側の時間切れのあとを除く)。
/// 書き込みに時間切れは掛けない (掛けると、実は書けていた行をやり直しで二重に書く)。
/// 他の sink や配信は別タスクなので、ここが詰まっても止まらない。
pub struct JsonlSink {
    path: PathBuf,
    delays: Vec<Duration>,
    /// 退避先を使った直後。直るまでは待たずに 1 回だけ試し、後ろに報を溜めない
    degraded: AtomicBool,
}

impl JsonlSink {
    pub fn new(cfg: JsonlConfig) -> Self {
        JsonlSink {
            path: cfg.path,
            delays: RETRY_DELAYS.to_vec(),
            degraded: AtomicBool::new(false),
        }
    }

    /// 設定の path の隣の、書いた時刻 (UTC) の日付のファイルに追記する (archive_store)
    pub async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        self.write_at(ev, crate::hub::now_ms()).await
    }

    async fn write_at(&self, ev: &Event, now_ms: u64) -> anyhow::Result<()> {
        let line = serde_json::to_string(ev)?;
        let path = archive_store::daily_path(&self.path, now_ms);
        let mut last = None;
        let retries = if self.degraded.load(Ordering::Relaxed) {
            0
        } else {
            self.delays.len()
        };
        for attempt in 0..=retries {
            if let Some(d) = attempt.checked_sub(1).map(|i| self.delays[i]) {
                tokio::time::sleep(d).await;
            }
            // 2 回目以降は頭に改行を足す。前回が途中まで書けていても、その破片が行として分かれる。
            // 破片は日付ファイルにしか入らない (読み手は壊れた行・空行を飛ばす) ことが安全性の前提
            match append_blocking(&path, &line, attempt > 0).await {
                Ok(()) => {
                    self.degraded.store(false, Ordering::Relaxed);
                    return Ok(());
                }
                Err(e) => {
                    if attempt == 0 {
                        // 以後どこで切られても journal から戻せるよう、最初の失敗で行そのものを残す
                        tracing::warn!(id = %ev.id, %line, "jsonl write failed (line dumped here): {} : {e}", path.display());
                    } else {
                        tracing::warn!(id = %ev.id, attempt, "jsonl write failed: {} : {e}", path.display());
                    }
                    last = Some(e);
                }
            }
        }
        let err = last.expect("at least one attempt");
        self.degraded.store(true, Ordering::Relaxed);
        let failed = failed_path(&self.path);
        match append_blocking(&failed, &line, true).await {
            Ok(()) => {
                tracing::error!(id = %ev.id, "jsonl write gave up; saved to {} : {err}", failed.display());
                Ok(())
            }
            Err(e2) => {
                tracing::error!(id = %ev.id, %line, "jsonl write lost; line dumped here: {err} / {e2}");
                Err(anyhow::anyhow!("jsonl write lost: {err} / {e2}"))
            }
        }
    }
}

/// 書けなかった行の退避先: path の隣の events-failed.jsonl。日付ファイルの名前ではないので archive は読まない
fn failed_path(base: &Path) -> PathBuf {
    let stem = base
        .file_stem()
        .map_or_else(|| "events".into(), |s| s.to_string_lossy());
    let ext = base.extension().map_or_else(|| "jsonl".into(), |s| s.to_string_lossy());
    base.with_file_name(format!("{stem}-failed.{ext}"))
}

async fn append_blocking(path: &Path, line: &str, lead_newline: bool) -> std::io::Result<()> {
    let (path, line) = (path.to_path_buf(), line.to_owned());
    tokio::task::spawn_blocking(move || {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        let nl = if lead_newline { "\n" } else { "" };
        f.write_all(format!("{nl}{line}\n").as_bytes())
    })
    .await
    .map_err(std::io::Error::other)?
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

    fn sink(base: PathBuf, delays: &[u64]) -> JsonlSink {
        JsonlSink {
            path: base,
            delays: delays.iter().map(|&m| Duration::from_millis(m)).collect(),
            degraded: AtomicBool::new(false),
        }
    }

    const T: u64 = 19_723 * 86_400_000; // 2024-01-01T00:00:00Z

    fn ids(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<Event>(l).unwrap().id)
            .collect()
    }

    #[tokio::test]
    async fn writes_to_the_file_of_the_write_time_date_not_the_event_time() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("sub/events.jsonl");
        let s = sink(base.clone(), &[]);
        s.write_at(&ev("a"), T - 1).await.unwrap();
        s.write_at(&ev("b"), T).await.unwrap();
        s.write_at(&ev("c"), T + 5).await.unwrap();
        let read = |n: &str| std::fs::read_to_string(dir.path().join("sub").join(n)).unwrap();
        assert_eq!(read("events-2023-12-31.jsonl").lines().count(), 1);
        assert_eq!(read("events-2024-01-01.jsonl").lines().count(), 2);
        assert!(!base.exists());
    }

    #[tokio::test]
    async fn retries_and_writes_once_in_order_after_the_failure_clears() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("events.jsonl");
        let daily = dir.path().join("events-2024-01-01.jsonl");
        std::fs::create_dir(&daily).unwrap(); // 開けない
        let d2 = daily.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(60)).await;
            std::fs::remove_dir(d2).unwrap();
        });
        let s = sink(base.clone(), &[40, 100, 500]);
        s.write_at(&ev("a"), T).await.unwrap();
        s.write_at(&ev("b"), T).await.unwrap();
        assert_eq!(ids(&daily), ["a", "b"]);
        assert!(!dir.path().join("events-failed.jsonl").exists());
    }

    #[tokio::test]
    async fn gives_up_into_the_failed_file_and_keeps_going() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("events.jsonl");
        std::fs::create_dir(dir.path().join("events-2024-01-01.jsonl")).unwrap();
        let s = sink(base, &[1, 1]);
        s.write_at(&ev("a"), T).await.unwrap(); // 退避できたので Ok
        s.write_at(&ev("b"), T).await.unwrap();
        assert_eq!(ids(&dir.path().join("events-failed.jsonl")), ["a", "b"]);
        // 退避先は archive の読む対象ではない
        let got = archive_store::files_in_range(&dir.path().join("events.jsonl"), 0, u64::MAX).unwrap();
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn skips_the_waits_while_degraded_and_recovers_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let daily = dir.path().join("events-2024-01-01.jsonl");
        std::fs::create_dir(&daily).unwrap();
        let s = sink(dir.path().join("events.jsonl"), &[600_000]); // 待つなら test が終わらない
        s.degraded.store(true, Ordering::Relaxed);
        s.write_at(&ev("a"), T).await.unwrap();
        assert_eq!(ids(&dir.path().join("events-failed.jsonl")), ["a"]);
        std::fs::remove_dir(&daily).unwrap();
        s.write_at(&ev("b"), T).await.unwrap();
        assert_eq!(ids(&daily), ["b"]);
        assert!(!s.degraded.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn errors_when_even_the_failed_file_cannot_be_written() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("events.jsonl");
        std::fs::create_dir(dir.path().join("events-2024-01-01.jsonl")).unwrap();
        std::fs::create_dir(dir.path().join("events-failed.jsonl")).unwrap();
        assert!(sink(base, &[1]).write_at(&ev("a"), T).await.is_err());
    }
}
