use std::time::Duration;

use anyhow::Context;
use eq_core::{p2pquake, Event};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::hub::Hub;

/// これだけ無通信なら接続が死んでいるとみなす (上流はピア情報を頻繁に流すので十分長い)
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);

/// WebSocket に接続し続ける。
/// 接続のたびに現在の津波予報を読み直す (切断中に出た予報・解除を取りこぼさないため)。
pub async fn run(url: &str, tsunami_url: &str, hub: &Hub) {
    super::reconnecting("P2P地震情報", || session(url, tsunami_url, hub)).await
}

async fn session(url: &str, tsunami_url: &str, hub: &Hub) -> anyhow::Result<()> {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.context("connect")?;
    tracing::info!("connected to upstream");
    if !tsunami_url.is_empty() {
        // 既に受け取っている予報なら重複として捨てられる
        match fetch_latest_tsunami(tsunami_url).await {
            Ok(Some(ev)) => {
                if hub.publish(ev) {
                    tracing::info!("caught up tsunami forecast");
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("failed to load tsunami: {e:#}"),
        }
    }
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
    events.iter_mut().for_each(stamp_issued);
    Ok(events)
}

/// 現在の津波予報 (最新の 1 件。解除済みならその解除の情報)。
pub async fn fetch_latest_tsunami(url: &str) -> anyhow::Result<Option<Event>> {
    let items: Vec<serde_json::Value> = reqwest::Client::new()
        .get(url)
        .query(&[("limit", "1")])
        .timeout(Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let Some(item) = items.into_iter().next() else {
        return Ok(None);
    };
    let mut ev = p2pquake::parse_value(item)?;
    ev.iter_mut().for_each(stamp_issued);
    Ok(ev)
}

/// 受信時刻を発表時刻にしておく。起動時にまとめて配信すると全件がほぼ同じ時刻になり、
/// ブラウザ側で新旧の順が崩れるため
fn stamp_issued(ev: &mut Event) {
    if let Some(t) = ev.issued_at_ms() {
        ev.received_at_ms = t.max(0) as u64;
    }
}
