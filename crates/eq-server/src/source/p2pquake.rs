use std::time::Duration;

use crate::quake::{p2pquake, Event, EventBody};
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::hub::Hub;
use crate::net;

/// これだけ無通信なら接続が死んでいるとみなす (上流はピア情報を頻繁に流すので十分長い)
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);
/// 履歴・津波予報の取得
const TIMEOUT: Duration = Duration::from_secs(15);

/// 取得元の URL
pub struct Urls<'a> {
    pub ws: &'a str,
    pub history: &'a str,
    pub history_limit: u32,
    pub tsunami: &'a str,
}

/// WebSocket に接続し続ける。
/// 接続のたびに現在の津波予報と地震情報の履歴を読み直す (切断中に出た報を取りこぼさないため。
/// 2026-10-29 00:07 千葉県北東部 (震度 5弱) では、上流の 522 と切断が続き、震度速報から各地の震度まで 5 件を落とした)。
pub async fn run(urls: Urls<'_>, hub: &Hub) {
    super::reconnecting("P2P地震情報", || session(&urls, hub)).await
}

async fn session(urls: &Urls<'_>, hub: &Hub) -> anyhow::Result<()> {
    let mut ws = crate::net::connect_ws(urls.ws).await?;
    tracing::info!("connected to upstream");
    let tsunami_url = urls.tsunami;
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
    if !urls.history.is_empty() && urls.history_limit > 0 {
        match fetch_history(urls.history, urls.history_limit).await {
            Ok(evs) => {
                let count = catch_up(evs, hub);
                if count > 0 {
                    tracing::info!(count, "caught up quake reports");
                }
            }
            Err(e) => tracing::warn!("failed to load history: {e:#}"),
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

/// 履歴のうち地震情報だけを流し直し、新しく流した件数を返す。受け取り済みは Hub が id で捨てる。
/// 緊急地震速報は古い報で画面を戻さないため流さない (Wolfx が受け持つ)
fn catch_up(events: Vec<Event>, hub: &Hub) -> usize {
    events
        .into_iter()
        .filter(|ev| matches!(ev.body, EventBody::Quake(_)))
        .filter(|ev| hub.publish(ev.clone()))
        .count()
}

/// 履歴 API から直近のイベントを古い順で取得する。
pub async fn fetch_history(base: &str, limit: u32) -> anyhow::Result<Vec<Event>> {
    let mut query: Vec<(&str, String)> = p2pquake::HANDLED_CODES
        .iter()
        .map(|c| ("codes", c.to_string()))
        .collect();
    query.push(("limit", limit.to_string()));
    let items: Vec<serde_json::Value> = net::json(net::client(TIMEOUT)?.get(base).query(&query)).await?;
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
    let items: Vec<serde_json::Value> = net::json(net::client(TIMEOUT)?.get(url).query(&[("limit", "1")])).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Event {
        p2pquake::parse(json).unwrap().unwrap()
    }

    const QUAKE: &str = r#"{"code":551,"id":"q1","issue":{"time":"2026/10/29 00:09:30","type":"ScalePrompt"},"earthquake":{"time":"2026/10/29 00:07:00","maxScale":45,"domesticTsunami":"None","hypocenter":{"name":"","latitude":-200,"longitude":-200,"depth":-1,"magnitude":-1}},"points":[]}"#;

    #[test]
    fn catch_up_publishes_only_unseen_quake_reports() {
        let hub = Hub::new(10);
        let quake = parse(QUAKE);
        let other = Event {
            id: "q2".into(),
            ..quake.clone()
        };
        let detection = parse(r#"{"code":554,"id":"d1","type":"Full"}"#);
        assert!(hub.publish(quake.clone()));
        assert_eq!(catch_up(vec![quake, other, detection], &hub), 1);
        let ids: Vec<_> = hub.recent().iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, ["q1", "q2"]);
    }
}
