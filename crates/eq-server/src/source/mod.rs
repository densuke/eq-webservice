//! 上流 (情報の取得元)。取得元を増やす場合はここにモジュールを追加し、
//! `eq_core::Event` に変換して `Hub::publish` すればよい。

pub mod p2pquake;
pub mod replay;

use std::sync::Arc;

use crate::config::SourceConfig;
use crate::hub::Hub;

/// 取得元を起動する (戻らない)。起動時の履歴取り込みもここで行う。
pub async fn run(cfg: SourceConfig, hub: Arc<Hub>, on_seed: impl FnOnce(&[Arc<eq_core::Event>])) {
    match cfg {
        SourceConfig::P2pquake {
            url,
            history_url,
            history_limit,
            tsunami_url,
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
            p2pquake::run(&url, &tsunami_url, &hub).await
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
