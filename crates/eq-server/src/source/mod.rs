//! 上流 (情報の取得元)。取得元を増やす場合はここにモジュールを追加し、
//! `crate::quake::Event` に変換して `Hub::publish` すればよい。

pub mod p2pquake;
pub mod replay;
pub mod wolfx;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

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
            tokio::join!(p2pquake::run(&url, &tsunami_url, &hub), wolfx::run(&eew_url, &hub));
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

/// session を繰り返す (戻らない)。切断・エラーのたびに指数バックオフで再接続する。
async fn reconnecting<F, Fut>(name: &str, mut session: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let mut backoff = MIN_BACKOFF;
    loop {
        tracing::info!(name, "connecting");
        match session().await {
            Ok(()) => {
                tracing::warn!(name, "upstream closed the connection");
                backoff = MIN_BACKOFF;
            }
            Err(e) => tracing::warn!(name, "upstream error: {e:#}"),
        }
        tracing::info!(name, secs = backoff.as_secs(), "reconnecting later");
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}
