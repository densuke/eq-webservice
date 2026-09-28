use std::time::Duration;

use anyhow::Context;
use eq_core::{p2pquake, Event};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::hub::Hub;

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// これだけ無通信なら接続が死んでいるとみなす (上流はピア情報を頻繁に流すので十分長い)
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);

/// WebSocket に接続し続ける。切断時は指数バックオフで再接続する。
pub async fn run(url: &str, hub: &Hub) {
    let mut backoff = MIN_BACKOFF;
    loop {
        tracing::info!(url, "connecting to P2P地震情報");
        match session(url, hub).await {
            Ok(()) => {
                tracing::warn!("upstream closed the connection");
                backoff = MIN_BACKOFF;
            }
            Err(e) => tracing::warn!("upstream error: {e:#}"),
        }
        tracing::info!(secs = backoff.as_secs(), "reconnecting later");
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn session(url: &str, hub: &Hub) -> anyhow::Result<()> {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.context("connect")?;
    tracing::info!("connected to upstream");
    loop {
        let msg = match tokio::time::timeout(IDLE_TIMEOUT, ws.next()).await {
            Err(_) => anyhow::bail!("no data for {}s", IDLE_TIMEOUT.as_secs()),
            Ok(None) => return Ok(()),
            Ok(Some(msg)) => msg.context("receive")?,
        };
        match msg {
            Message::Text(text) => handle_text(text.as_str(), hub),
            Message::Ping(p) => ws.send(Message::Pong(p)).await.context("pong")?,
            Message::Close(_) => return Ok(()),
            _ => {}
        }
    }
}

fn handle_text(text: &str, hub: &Hub) {
    match p2pquake::parse(text) {
        Ok(Some(ev)) => {
            let title = ev.title();
            if hub.publish(ev) {
                tracing::info!(%title, "event");
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("unparsable message: {e}"),
    }
}

/// 履歴 API から直近のイベントを古い順で取得する。
pub async fn fetch_history(base: &str, limit: u32) -> anyhow::Result<Vec<Event>> {
    let mut query: Vec<(&str, String)> = p2pquake::HANDLED_CODES
        .iter()
        .map(|c| ("codes", c.to_string()))
        .collect();
    query.push(("limit", limit.to_string()));
    let items: Vec<serde_json::Value> = reqwest::Client::new()
        .get(base)
        .query(&query)
        .timeout(Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut events: Vec<Event> = items
        .into_iter()
        .filter_map(|v| match p2pquake::parse_value(v) {
            Ok(ev) => ev,
            Err(e) => {
                tracing::warn!("unparsable history item: {e}");
                None
            }
        })
        .collect();
    // 履歴 API は新しい順
    events.reverse();
    Ok(events)
}
