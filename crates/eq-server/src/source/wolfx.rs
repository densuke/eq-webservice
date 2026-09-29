//! Wolfx Open API の JMA 緊急地震速報 (予報・警報) を WebSocket で受ける。

use std::time::Duration;

use crate::quake::wolfx;
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::hub::{now_ms, Hub};

/// サーバは毎分ハートビートを送るので、これだけ無通信なら接続が死んでいる
const IDLE_TIMEOUT: Duration = Duration::from_secs(150);
/// 推奨されている ping の間隔
const PING_INTERVAL: Duration = Duration::from_secs(60);
/// 発表からこれ以上たった速報は流さない (再接続直後に古い速報が届いても画面に出さないため)
const MAX_AGE_MS: i64 = 3 * 60_000;

/// url が空なら何もしない。
pub async fn run(url: &str, hub: &Hub) {
    if url.is_empty() {
        return;
    }
    super::reconnecting("Wolfx", || session(url, hub)).await
}

async fn session(url: &str, hub: &Hub) -> anyhow::Result<()> {
    let mut ws = crate::net::connect_ws(url).await?;
    tracing::info!("connected to Wolfx");
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await;
    loop {
        let msg = tokio::select! {
            _ = ping.tick() => {
                ws.send(Message::text("ping")).await.context("ping")?;
                continue;
            }
            r = tokio::time::timeout(IDLE_TIMEOUT, ws.next()) => match r {
                Err(_) => anyhow::bail!("no data for {}s", IDLE_TIMEOUT.as_secs()),
                Ok(None) => return Ok(()),
                Ok(Some(msg)) => msg.context("receive")?,
            },
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
    match wolfx::parse(text) {
        Ok(Some(ev)) => {
            if is_stale(&ev, now_ms() as i64) {
                tracing::info!(id = %ev.id, "skipped stale EEW");
                return;
            }
            let title = ev.title();
            if hub.publish(ev) {
                tracing::info!(%title, "event");
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("unparsable Wolfx message: {e}"),
    }
}

/// 発表から MAX_AGE_MS を過ぎた速報か (再接続直後に届く古い速報を流さない)
fn is_stale(ev: &crate::quake::Event, now_ms: i64) -> bool {
    ev.issued_at_ms().is_some_and(|t| now_ms - t > MAX_AGE_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_reports_are_dropped() {
        let ev = wolfx::parse(
            r#"{"type":"jma_eew","EventID":"1","Serial":1,"AnnouncedTime":"2026/09/29 04:45:58","OriginTime":"2026/09/29 04:45:05","Hypocenter":"茨城県南部","MaxIntensity":"4","WarnArea":[]}"#,
        )
        .unwrap()
        .unwrap();
        let issued = ev.issued_at_ms().unwrap();
        assert!(!is_stale(&ev, issued + 1_000));
        assert!(!is_stale(&ev, issued + MAX_AGE_MS));
        assert!(is_stale(&ev, issued + MAX_AGE_MS + 1));
    }
}
