//! events が URL のときの、記録の取り方 (docs/replay-video.md 6.2)。
//! Mac は止まっている時間が長い (スリープ・停止・オフライン) ので、前に見終えた時刻 (checkpoint) をファイルに残し、
//! 次の見直しでは、そこから今までを 1 時間ずつ取る。全部取れて、キューに積めたときだけ checkpoint を進める
//! (途中で失敗したら、次の見直しで同じところからやり直す)。

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::super::source::fetch_chunks;
use super::config::WorkerConfig;
use crate::quake::Event;

const HOUR_MS: u64 = 3_600_000;

/// どこからどこまで取るかの規則
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// checkpoint が無い (最初の) ときにさかのぼる長さ
    pub lookback_ms: u64,
    /// 止まっていた間をさかのぼる上限
    pub catchup_max_ms: u64,
    /// checkpoint より前から取り直す長さ。checkpoint の時点で開いていたまとまりの始まりを含めるため、
    /// 1 つのまとまりの最長 (max_group_hours) + 閉じるまでの静かな時間 (quiet_min)
    pub overlap_ms: u64,
}

impl From<&WorkerConfig> for Window {
    fn from(c: &WorkerConfig) -> Self {
        Window {
            lookback_ms: c.lookback_hours * HOUR_MS,
            catchup_max_ms: c.catchup_max_hours * HOUR_MS,
            overlap_ms: c.max_group_hours * HOUR_MS + c.quiet_min * 60_000,
        }
    }
}

impl Window {
    /// 取り始める時刻 (取り終わりは今)。checkpoint があれば、overlap だけ手前から。ただし catchup_max より古くはしない
    pub fn start_ms(&self, now_ms: u64, checkpoint: Option<u64>) -> u64 {
        let from = match checkpoint {
            Some(c) => c
                .saturating_sub(self.overlap_ms)
                .max(now_ms.saturating_sub(self.catchup_max_ms)),
            None => now_ms.saturating_sub(self.lookback_ms),
        };
        // 時計が戻って checkpoint が未来にあっても、空の範囲にならないようにする
        from.min(now_ms)
    }
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    scanned_until_ms: u64,
}

/// checkpoint を読む。無い・壊れているときは None (最初と同じ扱い)
pub fn load(path: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Checkpoint>(&text) {
        Ok(c) => Some(c.scanned_until_ms),
        Err(e) => {
            tracing::warn!("replay-worker: checkpoint が壊れています ({}): {e}", path.display());
            None
        }
    }
}

/// checkpoint を書く (書きかけを残さないよう、別名で書いて置き換える)
pub fn save(path: &Path, scanned_until_ms: u64) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec(&Checkpoint { scanned_until_ms })?;
    std::fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

/// checkpoint から今までを 1 時間ずつ取り (`fetch`)、`process` に渡す。process が true (全部キューに積めた) を返したときだけ、
/// checkpoint を今に進める。取るのが 1 つでも失敗したら Err で、checkpoint は動かさない
pub async fn catch_up<F, Fut>(
    checkpoint: &Path,
    now_ms: u64,
    window: &Window,
    pause: Duration,
    fetch: F,
    process: impl FnOnce(Vec<Event>) -> bool,
) -> anyhow::Result<()>
where
    F: FnMut(u64, u64) -> Fut,
    Fut: Future<Output = anyhow::Result<Vec<Event>>>,
{
    let from = window.start_ms(now_ms, load(checkpoint));
    let events = fetch_chunks(from, now_ms, pause, fetch).await?;
    if process(events) {
        save(checkpoint, now_ms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = HOUR_MS;
    const NOW: u64 = 1_000 * H;

    fn window() -> Window {
        Window::from(&WorkerConfig {
            catchup_max_hours: 168,
            ..Default::default()
        })
    }

    #[test]
    fn the_window_follows_the_config() {
        // 既定: lookback 6 時間・max_group 3 時間・quiet 60 分
        assert_eq!(
            window(),
            Window {
                lookback_ms: 6 * H,
                catchup_max_ms: 168 * H,
                overlap_ms: 4 * H
            }
        );
    }

    #[test]
    fn without_a_checkpoint_the_window_is_the_lookback() {
        assert_eq!(window().start_ms(NOW, None), NOW - 6 * H);
    }

    #[test]
    fn with_a_checkpoint_the_window_starts_an_overlap_before_it() {
        // 5 分前に見終えた: overlap (4 時間) の分だけ手前から
        assert_eq!(window().start_ms(NOW, Some(NOW - 5 * 60_000)), NOW - 5 * 60_000 - 4 * H);
        // 2 日止まっていた: 2 日 + overlap をさかのぼって取り戻す
        assert_eq!(window().start_ms(NOW, Some(NOW - 48 * H)), NOW - 52 * H);
    }

    #[test]
    fn the_catchup_never_goes_back_further_than_the_cap() {
        // 30 日止まっていても、7 日まで
        assert_eq!(window().start_ms(NOW, Some(NOW - 720 * H)), NOW - 168 * H);
        // 上限の手前では、overlap より上限が優先
        assert_eq!(window().start_ms(NOW, Some(NOW - 167 * H)), NOW - 168 * H);
        // 時計が戻って checkpoint が未来でも、今を超えない
        assert_eq!(window().start_ms(NOW, Some(NOW + 100 * H)), NOW);
        // 起動直後で今が小さくても、引き算であふれない
        assert_eq!(window().start_ms(H, Some(0)), 0);
    }

    #[test]
    fn a_checkpoint_round_trips_and_a_broken_one_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("checkpoint.json");
        assert_eq!(load(&p), None);
        save(&p, 1234).unwrap();
        assert_eq!(load(&p), Some(1234));
        save(&p, 5678).unwrap();
        assert_eq!(load(&p), Some(5678));
        std::fs::write(&p, "{").unwrap();
        assert_eq!(load(&p), None);
    }

    fn fetch_ok(
        calls: &std::sync::Mutex<Vec<(u64, u64)>>,
    ) -> impl FnMut(u64, u64) -> std::future::Ready<anyhow::Result<Vec<Event>>> + '_ {
        |a, b| {
            calls.lock().unwrap().push((a, b));
            std::future::ready(Ok(vec![]))
        }
    }

    #[tokio::test]
    async fn the_checkpoint_advances_to_now_after_everything_was_fetched_and_queued() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("checkpoint.json");
        save(&p, NOW - 2 * H).unwrap();
        let calls = std::sync::Mutex::new(vec![]);
        catch_up(&p, NOW, &window(), Duration::ZERO, fetch_ok(&calls), |_| true)
            .await
            .unwrap();
        assert_eq!(load(&p), Some(NOW));
        // checkpoint の 2 時間前 + overlap 4 時間 = 6 時間前から、1 時間ずつ今まで
        let calls = calls.into_inner().unwrap();
        assert_eq!((calls[0].0, calls[calls.len() - 1].1), (NOW - 6 * H, NOW));
        assert_eq!(calls.len(), 6);
    }

    #[tokio::test]
    async fn the_checkpoint_stays_when_a_fetch_fails_or_the_queue_could_not_be_written() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("checkpoint.json");
        save(&p, NOW - 2 * H).unwrap();
        // 途中の範囲が取れない (ネットワークが切れた)
        let n = std::sync::atomic::AtomicUsize::new(0);
        let failed = catch_up(
            &p,
            NOW,
            &window(),
            Duration::ZERO,
            |_, _| {
                let i = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::ready(if i == 3 {
                    Err(anyhow::anyhow!("offline"))
                } else {
                    Ok(vec![])
                })
            },
            |_| panic!("取れていないのに、キューに積もうとした"),
        )
        .await;
        assert!(failed.is_err());
        assert_eq!(load(&p), Some(NOW - 2 * H));
        // 取れたが、キューに積めなかった
        let calls = std::sync::Mutex::new(vec![]);
        catch_up(&p, NOW, &window(), Duration::ZERO, fetch_ok(&calls), |_| false)
            .await
            .unwrap();
        assert_eq!(load(&p), Some(NOW - 2 * H));
    }
}
