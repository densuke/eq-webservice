//! 配信の「地震の画面か平時か」を小さなファイルに書く (docs/replay-video.md 5.2 の 1)。
//! 再現動画を作る係 (replay/worker) が、地震の間は作らず、平時の長さを見るために読む。
//! 切り替わったときと 30 秒ごとに書き直す。書けなくても配信は止めない (警告を 1 度出すだけ)。
//! 作る係は `updated_ms` が古ければ「わからない」とみなす (screen)。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// 書き直す間隔
const WRITE_EVERY: Duration = Duration::from_secs(30);
/// これより古い書き込みは、配信の状態がわからないものとして扱う
pub const STALE_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// 平時か (true) 地震の画面か (false)
    pub calm: bool,
    /// 今の状態になった時刻 (epoch ミリ秒。配信を始めたときも更新される)
    pub since_ms: u64,
    /// 書いた時刻 (epoch ミリ秒)
    pub updated_ms: u64,
}

/// 作る係から見た配信の画面
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    /// 平時。since_ms から続いている
    Calm {
        since_ms: u64,
    },
    Quake,
    /// 読めない・古い。地震の画面と同じ慎重な側に倒す
    Unknown,
}

/// 状態のファイルの中身から、今の画面を決める
pub fn screen(snapshot: Option<&Snapshot>, now_ms: u64) -> Screen {
    match snapshot {
        Some(s) if now_ms.saturating_sub(s.updated_ms) <= STALE_MS => {
            if s.calm {
                Screen::Calm { since_ms: s.since_ms }
            } else {
                Screen::Quake
            }
        }
        _ => Screen::Unknown,
    }
}

/// 既定の置き場所: `$XDG_STATE_HOME/eq-broadcast/state.json` (無ければ `~/.local/state/...`)
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    base.join("eq-broadcast/state.json")
}

/// 設定の値 (空なら既定) から、置き場所を決める
pub fn path_of(configured: &str) -> PathBuf {
    if configured.is_empty() {
        default_path()
    } else {
        PathBuf::from(configured)
    }
}

/// ファイルを読む。無い・壊れているときは None
pub fn read(path: &Path) -> Option<Snapshot> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// 途中まで書いたファイルを読ませないよう、隣に書いてから置き換える
async fn write(path: &Path, snapshot: &Snapshot) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let body = serde_json::to_vec(snapshot).map_err(std::io::Error::other)?;
    tokio::fs::write(&tmp, body).await?;
    tokio::fs::rename(&tmp, path).await
}

/// calm の変化を見て、状態のファイルを書く。最初の画面が描けるまでは書かない
/// (calm の初期値は、データを受ける前の仮の値なので、平時と書かないため)。
/// calm の送り手が消えたら (配信のやり直し) 終わる。古くなったファイルは、作る係が「わからない」とみなす
pub fn spawn(path: PathBuf, mut calm: watch::Receiver<bool>, frames: watch::Receiver<Arc<Vec<u8>>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut current = *calm.borrow_and_update();
        let mut since_ms = now_ms();
        let mut warned = false;
        let mut tick = tokio::time::interval(WRITE_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                r = calm.changed() => {
                    if r.is_err() {
                        return;
                    }
                    let next = *calm.borrow_and_update();
                    if next != current {
                        current = next;
                        since_ms = now_ms();
                    }
                }
                _ = tick.tick() => {}
            }
            if frames.borrow().is_empty() {
                continue;
            }
            let snapshot = Snapshot {
                calm: current,
                since_ms,
                updated_ms: now_ms(),
            };
            match write(&path, &snapshot).await {
                Ok(()) => warned = false,
                Err(e) if !warned => {
                    warned = true;
                    tracing::warn!(path = %path.display(), "broadcast: 状態のファイルを書けません (配信は続けます): {e}");
                }
                Err(_) => {}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(calm: bool, since: u64, updated: u64) -> Snapshot {
        Snapshot {
            calm,
            since_ms: since,
            updated_ms: updated,
        }
    }

    #[test]
    fn a_fresh_snapshot_tells_calm_or_quake() {
        let now = 1_000_000;
        assert_eq!(
            screen(Some(&snap(true, 500, now - 10)), now),
            Screen::Calm { since_ms: 500 }
        );
        assert_eq!(screen(Some(&snap(false, 500, now - 10)), now), Screen::Quake);
    }

    #[test]
    fn a_missing_or_stale_snapshot_is_unknown() {
        let now = 1_000_000;
        assert_eq!(screen(None, now), Screen::Unknown);
        assert_eq!(
            screen(Some(&snap(true, 0, now - STALE_MS)), now),
            Screen::Calm { since_ms: 0 }
        );
        assert_eq!(screen(Some(&snap(true, 0, now - STALE_MS - 1)), now), Screen::Unknown);
    }

    #[tokio::test]
    async fn the_file_round_trips_and_a_broken_file_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/state.json");
        let s = snap(false, 7, 9);
        write(&path, &s).await.unwrap();
        assert_eq!(read(&path), Some(s));
        std::fs::write(&path, "{").unwrap();
        assert_eq!(read(&path), None);
        assert_eq!(read(&dir.path().join("none.json")), None);
    }

    #[test]
    fn the_configured_path_wins_and_empty_means_the_default() {
        assert_eq!(path_of("/x/y.json"), PathBuf::from("/x/y.json"));
        assert!(path_of("").ends_with("eq-broadcast/state.json"));
    }

    #[tokio::test]
    async fn it_writes_after_the_first_frame_and_on_a_change_and_survives_a_bad_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let (calm_tx, calm) = watch::channel(true);
        let (frames_tx, frames) = watch::channel(Arc::new(Vec::new()));
        let task = spawn(path.clone(), calm, frames);
        // 最初の画面の前は書かない (最初の tick は、すぐ来る)
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(read(&path), None);
        frames_tx.send(Arc::new(vec![1])).unwrap();
        calm_tx.send(false).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!read(&path).unwrap().calm);
        // 送り手が消えたら終わる
        drop(calm_tx);
        task.await.unwrap();

        // 書けない場所でも落ちない (親がファイルなので作れない)
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, "x").unwrap();
        let (calm_tx, calm) = watch::channel(true);
        let (_frames_tx, frames) = watch::channel(Arc::new(vec![1]));
        let task = spawn(blocker.join("state.json"), calm, frames);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!task.is_finished());
        drop(calm_tx);
        task.await.unwrap();
    }
}
