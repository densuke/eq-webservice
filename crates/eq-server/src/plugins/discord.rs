//! Discord の Webhook へ埋め込みメッセージとして投稿する。

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use eq_core::{Event, EventBody, Scale, TsunamiGrade};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use super::Sink;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordConfig {
    /// Webhook URL。設定ファイルに書きたくない場合は `webhook_url_env` を使う。
    webhook_url: Option<String>,
    /// Webhook URL を読む環境変数名
    webhook_url_env: Option<String>,
    #[serde(default)]
    username: Option<String>,
}

pub struct DiscordSink {
    url: String,
    username: Option<String>,
    client: reqwest::Client,
    /// 緊急地震速報は続報が多いので、地震ごと (event_id) に通知済みの (警報か, 最大震度) を覚えておく
    eew_notified: Mutex<HashMap<String, (bool, Scale)>>,
}

impl DiscordSink {
    pub fn new(cfg: DiscordConfig) -> anyhow::Result<Self> {
        let url = match (cfg.webhook_url, cfg.webhook_url_env) {
            (Some(u), _) if !u.is_empty() => u,
            (_, Some(var)) => std::env::var(&var).with_context(|| format!("environment variable {var} is not set"))?,
            _ => anyhow::bail!("discord: webhook_url or webhook_url_env is required"),
        };
        Ok(DiscordSink {
            url,
            username: cfg.username,
            client: reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?,
            eew_notified: Mutex::new(HashMap::new()),
        })
    }

    /// EEW は「その地震で初めて」「予報から警報に上がった」「予測震度が上がった」「取消」のときだけ通知する。
    async fn should_post(&self, ev: &Event) -> bool {
        let EventBody::Eew(e) = &ev.body else { return true };
        let mut seen = self.eew_notified.lock().await;
        if seen.len() > 256 {
            seen.clear();
        }
        let now = (e.warning, e.max_scale);
        match seen.get(&e.event_id) {
            _ if e.cancelled => true,
            // (警報か, 震度) の順に比べる: 予報から警報に上がったら震度が同じでも通知する
            Some(prev) if now <= *prev => false,
            _ => {
                seen.insert(e.event_id.clone(), now);
                true
            }
        }
    }
}

#[async_trait]
impl Sink for DiscordSink {
    async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        if !self.should_post(ev).await {
            return Ok(());
        }
        let mut body = json!({
            "embeds": [{
                "title": ev.title(),
                "description": ev.summary(),
                "color": color(ev),
                "footer": { "text": format!("出典: {}", source_label(&ev.source)) },
            }],
            "allowed_mentions": { "parse": [] },
        });
        if let Some(u) = &self.username {
            body["username"] = json!(u);
        }
        for _ in 0..2 {
            // Webhook の URL は秘密情報なので、エラー (ログに出る) には含めない
            let res = self
                .client
                .post(&self.url)
                .json(&body)
                .send()
                .await
                .map_err(reqwest::Error::without_url)?;
            if res.status().as_u16() == 429 {
                // レート制限: 指定秒数待って 1 回だけ再送
                let wait: f64 = res
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|v| v["retry_after"].as_f64())
                    .unwrap_or(1.0);
                tokio::time::sleep(Duration::from_secs_f64(wait.min(10.0))).await;
                continue;
            }
            res.error_for_status().map_err(reqwest::Error::without_url)?;
            return Ok(());
        }
        anyhow::bail!("rate limited")
    }
}

fn source_label(s: &str) -> &str {
    match s {
        "p2pquake" => "P2P地震情報 (気象庁発表)",
        other => other,
    }
}

fn color(ev: &Event) -> u32 {
    if let EventBody::Tsunami(t) = &ev.body {
        return match t.areas.iter().map(|a| a.grade).max() {
            Some(TsunamiGrade::MajorWarning) => 0x9b00ff,
            Some(TsunamiGrade::Warning) => 0xff2800,
            Some(TsunamiGrade::Watch) => 0xfaf500,
            _ => 0x888888,
        };
    }
    match ev.max_scale().map(|s| s.0).unwrap_or(-1) {
        70 => 0xb40068,
        60 => 0xa50021,
        55 => 0xff2800,
        50 => 0xff9900,
        45 | 46 => 0xffe600,
        40 => 0xfaf500,
        30 => 0x0041ff,
        20 => 0x00aaff,
        10 => 0xf2f2ff,
        _ => 0x888888,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_core::Eew;

    fn sink() -> DiscordSink {
        DiscordSink::new(DiscordConfig {
            webhook_url: Some("https://discord.example/api/webhooks/x".into()),
            webhook_url_env: None,
            username: None,
        })
        .unwrap()
    }

    fn eew(event_id: &str, warning: bool, scale: Scale, cancelled: bool) -> Event {
        Event {
            id: format!("{event_id}-{warning}-{}-{cancelled}", scale.0),
            source: "test".into(),
            received_at_ms: 0,
            body: EventBody::Eew(Eew {
                event_id: event_id.into(),
                serial: "1".into(),
                cancelled,
                test: false,
                warning,
                issued_at: String::new(),
                origin_time: None,
                origin_time_ms: None,
                hypocenter: None,
                areas: vec![],
                pref_max: vec![],
                max_scale: scale,
            }),
        }
    }

    #[tokio::test]
    async fn eew_is_posted_once_and_again_only_when_it_gets_worse() {
        let s = sink();
        assert!(s.should_post(&eew("A", false, Scale::S4, false)).await);
        // 続報で変わらなければ送らない
        assert!(!s.should_post(&eew("A", false, Scale::S4, false)).await);
        assert!(!s.should_post(&eew("A", false, Scale::S3, false)).await);
        // 予報から警報に上がったら、震度が同じでも送る
        assert!(s.should_post(&eew("A", true, Scale::S4, false)).await);
        assert!(!s.should_post(&eew("A", true, Scale::S4, false)).await);
        // 予測震度が上がったら送る
        assert!(s.should_post(&eew("A", true, Scale::S5_LOWER, false)).await);
        // 取消はいつでも送る
        assert!(s.should_post(&eew("A", true, Scale::S5_LOWER, true)).await);
        // 別の地震は別に数える
        assert!(s.should_post(&eew("B", false, Scale::S3, false)).await);
    }
}
