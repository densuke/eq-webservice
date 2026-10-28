//! 上流 (情報の取得元)。取得元を増やす場合はここにモジュールを追加し、
//! `crate::quake::Event` に変換して `Hub::publish` すればよい。

pub mod p2pquake;
pub mod replay;
pub mod wolfx;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::SourceConfig;
use crate::hub::Hub;

/// 取得元を起動する (戻らない)。起動時の履歴取り込みもここで行う。
pub async fn run(cfg: SourceConfig, hub: Arc<Hub>, on_seed: impl FnOnce(&[Arc<crate::quake::Event>])) {
    match cfg {
        SourceConfig::P2pquake {
            url,
            history_url,
            history_limit,
            tsunami_url,
            eew_url,
        } => {
            let mut events = Vec::new();
            if !history_url.is_empty() && history_limit > 0 {
                match p2pquake::fetch_history(&history_url, history_limit).await {
                    Ok(evs) => events = evs,
                    Err(e) => tracing::warn!("failed to load history: {e:#}"),
                }
            }
            if !tsunami_url.is_empty() {
                match p2pquake::fetch_latest_tsunami(&tsunami_url).await {
                    Ok(ev) => events.extend(ev),
                    Err(e) => tracing::warn!("failed to load tsunami: {e:#}"),
                }
            }
            let seeded = hub.seed(events);
            tracing::info!(count = seeded.len(), "loaded history");
            on_seed(&seeded);
            let urls = p2pquake::Urls {
                ws: &url,
                history: &history_url,
                history_limit,
                tsunami: &tsunami_url,
            };
            tokio::join!(p2pquake::run(urls, &hub), wolfx::run(&eew_url, &hub));
        }
        SourceConfig::Replay {
            path,
            speed,
            max_gap_ms,
            rebase_time,
            r#loop,
        } => {
            let opts = replay::Options {
                speed,
                max_gap_ms,
                rebase_time,
                repeat: r#loop,
            };
            if let Err(e) = replay::run(&path, &opts, &hub).await {
                tracing::error!("replay failed: {e:#}");
            }
        }
    }
}

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// これだけ続いた接続のあとは、待ち時間を最短に戻す (つながってから切れたのに 60 秒待つと、その間の報を落とす)
const STABLE: Duration = Duration::from_secs(10);

/// 次の待ち時間。今回の待ち時間 backoff を、接続が lasted だけ続いたあとに決める
fn next_backoff(backoff: Duration, lasted: Duration) -> Duration {
    if lasted >= STABLE {
        MIN_BACKOFF
    } else {
        backoff
    }
}

/// session を繰り返す (戻らない)。切断・エラーのたびに指数バックオフで再接続する。
async fn reconnecting<F, Fut>(name: &str, mut session: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let mut backoff = MIN_BACKOFF;
    loop {
        tracing::info!(name, "connecting");
        let started = Instant::now();
        match session().await {
            Ok(()) => {
                tracing::warn!(name, "upstream closed the connection");
                backoff = MIN_BACKOFF;
            }
            Err(e) => tracing::warn!(name, "upstream error: {e:#}"),
        }
        backoff = next_backoff(backoff, started.elapsed());
        tracing::info!(name, secs = backoff.as_secs(), "reconnecting later");
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_resets_only_after_a_lasting_connection() {
        let sixty = Duration::from_secs(60);
        assert_eq!(next_backoff(sixty, Duration::from_secs(90)), MIN_BACKOFF);
        assert_eq!(next_backoff(sixty, Duration::from_secs(1)), sixty);
    }
}
