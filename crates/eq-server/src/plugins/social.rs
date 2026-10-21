//! 自作の投稿ツール (複数の SNS へまとめて投稿する API) へ、緊急地震速報を投稿する。
//!
//! 投稿は外に公開されて取り消せないので、地震ごとに 1 回だけにする:
//! - 警報か、予測最大震度が `min_forecast` 以上になった最初の報で 1 回投稿する
//! - 投稿した地震が取り消されたら、取り消しを 1 回投稿する
//! - テストの報・記録の再生 (replay / demo) は投稿しない

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::quake::{jst, Eew, Event, EventBody, Scale};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocialConfig {
    /// 投稿の API (例 `https://post.fuga.jp/api/post`)
    url: String,
    /// API キーを読む環境変数名 (X-Api-Key ヘッダで送る)
    api_key_env: String,
    /// 投稿先 (例 `["bluesky", "x", "mastodon-social"]`)
    targets: Vec<String>,
    /// 警報でなくても、予測最大震度がこれ以上なら投稿する ("3", "5弱" など)
    #[serde(default = "default_min_forecast", deserialize_with = "de_scale")]
    min_forecast: Scale,
    /// 本文の最後に足す行 (配信の URL など)。空なら「現在の状況は〜」の案内も付けない
    #[serde(default)]
    footer: Vec<String>,
}

fn default_min_forecast() -> Scale {
    Scale::S3
}

fn de_scale<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Scale, D::Error> {
    let s = String::deserialize(d)?;
    Scale::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown scale {s:?}")))
}

pub struct SocialSink {
    cfg: SocialConfig,
    key: String,
    client: reqwest::Client,
    /// 投稿した地震 (event_id) と、取り消しの文に使う震源の名前。取り消しを投稿したら消す
    // shortcut: 覚えるのはプロセスの中だけ。緊急地震速報の最中に eq-server を再起動すると同じ地震をもう一度投稿しうる、起きたらファイルに残す
    posted: Mutex<HashMap<String, String>>,
}

/// この報でどうするか
#[derive(Debug, PartialEq)]
enum Action {
    Post,
    /// 投稿済みの地震の取り消し (震源の名前)
    Cancel(String),
    Skip,
}

fn decide(posted: &HashMap<String, String>, e: &Eew, min_forecast: Scale) -> Action {
    if e.test {
        return Action::Skip;
    }
    if e.cancelled {
        return posted
            .get(&e.event_id)
            .map_or(Action::Skip, |place| Action::Cancel(place.clone()));
    }
    let strong = e.warning || (e.max_scale.is_known() && e.max_scale >= min_forecast);
    if strong && !posted.contains_key(&e.event_id) {
        Action::Post
    } else {
        Action::Skip
    }
}

fn place_name(e: &Eew) -> String {
    e.hypocenter
        .as_ref()
        .map(|h| h.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "調査中".into())
}

/// 最初の報の本文
fn post_text(received_ms: u64, e: &Eew, footer: &[String]) -> String {
    let h = e.hypocenter.as_ref();
    let mut source = place_name(e);
    if let Some(m) = h.and_then(|h| h.magnitude).filter(|m| *m > 0.0) {
        source += &format!(" M{m:.1}");
    }
    if let Some(d) = h.and_then(|h| h.depth_km).filter(|d| *d >= 0) {
        source += &if d == 0 {
            " ごく浅い".to_string()
        } else {
            format!(" 深さ{d}km")
        };
    }
    let mut lines = vec![
        format!("緊急地震速報を検知しました: {}", jst::format(received_ms as i64)),
        format!("発生源: {source}"),
        format!(
            "予測最大震度: {} ({})",
            e.max_scale.label(),
            if e.warning { "警報" } else { "予報" }
        ),
    ];
    if !footer.is_empty() {
        lines.push("現在の状況は以下で確認できます。".into());
        lines.push(String::new());
        lines.extend(footer.iter().cloned());
    }
    lines.join("\n")
}

fn cancel_text(place: &str) -> String {
    format!("先ほどの緊急地震速報 (発生源: {place}) は取り消されました。")
}

impl SocialSink {
    pub fn new(cfg: SocialConfig) -> anyhow::Result<Self> {
        let key = std::env::var(&cfg.api_key_env)
            .with_context(|| format!("environment variable {} is not set", cfg.api_key_env))?;
        anyhow::ensure!(!cfg.targets.is_empty(), "social: targets is empty");
        Ok(SocialSink {
            cfg,
            key,
            client: crate::net::client_no_redirect(Duration::from_secs(10))?,
            posted: Mutex::new(HashMap::new()),
        })
    }

    pub async fn handle(&self, ev: &Event) -> anyhow::Result<()> {
        let EventBody::Eew(e) = &ev.body else { return Ok(()) };
        if ev.source == "replay" || ev.source == "demo" {
            return Ok(());
        }
        let text = {
            let mut posted = self.posted.lock().await;
            match decide(&posted, e, self.cfg.min_forecast) {
                Action::Skip => return Ok(()),
                Action::Post => {
                    if posted.len() > 256 {
                        posted.clear();
                    }
                    // 送る前に覚える (失敗しても同じ地震の続報で何度も投稿し直さない)
                    posted.insert(e.event_id.clone(), place_name(e));
                    post_text(ev.received_at_ms, e, &self.cfg.footer)
                }
                Action::Cancel(place) => {
                    posted.remove(&e.event_id);
                    cancel_text(&place)
                }
            }
        };
        self.send(&text).await
    }

    async fn send(&self, text: &str) -> anyhow::Result<()> {
        let res = self
            .client
            .post(&self.cfg.url)
            .header("X-Api-Key", &self.key)
            .json(&json!({ "text": text, "targets": self.cfg.targets }))
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        // キー (ヘッダ) を別の送り先へ転送しないよう、リダイレクトは追わずに失敗として扱う
        anyhow::ensure!(res.status().is_success(), "social: HTTP {}", res.status());
        // 一部の投稿先だけ失敗しても HTTP 200 で返る (X の月の上限など)。中身の results を見る
        let body: serde_json::Value = res.json().await.unwrap_or_default();
        let (total, failed) = failures(&body);
        if !failed.is_empty() {
            tracing::warn!(
                failed = failed.len(),
                total,
                "social: some targets failed: {}",
                failed.join(" / ")
            );
        }
        anyhow::ensure!(total == 0 || failed.len() < total, "social: all targets failed");
        tracing::info!(targets = ?self.cfg.targets, ok = total - failed.len(), "social: posted");
        Ok(())
    }
}

/// 応答の results の数と、失敗したものの理由 (長いものは切る)。results の並びは頼んだ投稿先の順と限らない
fn failures(body: &serde_json::Value) -> (usize, Vec<String>) {
    let results = body["results"].as_array().map(Vec::as_slice).unwrap_or_default();
    let failed = results
        .iter()
        .filter(|r| r["success"] != true)
        .map(|r| {
            r["error_message"]
                .as_str()
                .unwrap_or("理由なし")
                .chars()
                .take(200)
                .collect()
        })
        .collect();
    (results.len(), failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::Hypocenter;

    fn eew(id: &str, warning: bool, scale: Scale) -> Eew {
        Eew {
            event_id: id.into(),
            serial: "1".into(),
            cancelled: false,
            test: false,
            warning,
            issued_at: String::new(),
            origin_time: None,
            origin_time_ms: None,
            hypocenter: Some(Hypocenter {
                name: "千葉県北西部".into(),
                latitude: None,
                longitude: None,
                depth_km: Some(80),
                magnitude: Some(4.3),
            }),
            areas: vec![],
            pref_max: vec![],
            max_scale: scale,
        }
    }

    #[test]
    fn posts_a_warning_or_a_strong_forecast_once_per_quake() {
        let none = HashMap::new();
        assert_eq!(decide(&none, &eew("a", false, Scale::S3), Scale::S3), Action::Post);
        assert_eq!(decide(&none, &eew("a", false, Scale::S2), Scale::S3), Action::Skip);
        // 警報は予測震度によらず投稿する
        assert_eq!(decide(&none, &eew("a", true, Scale::UNKNOWN), Scale::S4), Action::Post);
        // 震度不明の予報は投稿しない
        assert_eq!(decide(&none, &eew("a", false, Scale::UNKNOWN), Scale::S3), Action::Skip);
        let posted = HashMap::from([("a".to_string(), "千葉県北西部".to_string())]);
        assert_eq!(
            decide(&posted, &eew("a", true, Scale::S5_LOWER), Scale::S3),
            Action::Skip
        );
        let test = Eew {
            test: true,
            ..eew("b", true, Scale::S7)
        };
        assert_eq!(decide(&none, &test, Scale::S3), Action::Skip);
    }

    #[test]
    fn a_cancellation_is_posted_only_for_a_posted_quake() {
        let cancel = Eew {
            cancelled: true,
            ..eew("a", false, Scale::UNKNOWN)
        };
        assert_eq!(decide(&HashMap::new(), &cancel, Scale::S3), Action::Skip);
        let posted = HashMap::from([("a".to_string(), "千葉県北西部".to_string())]);
        assert_eq!(
            decide(&posted, &cancel, Scale::S3),
            Action::Cancel("千葉県北西部".into())
        );
        assert_eq!(
            cancel_text("千葉県北西部"),
            "先ほどの緊急地震速報 (発生源: 千葉県北西部) は取り消されました。"
        );
    }

    #[test]
    fn the_text_has_the_time_source_forecast_and_footer() {
        // 2026-10-21 10:04:44 JST に受けた、千葉県北西部の予報 (予測震度 3)
        let footer = vec![
            "YouTube: https://youtube.com/@fugahoge873/live".to_string(),
            "Twitch: https://www.twitch.tv/fugahogeds".to_string(),
        ];
        assert_eq!(
            post_text(1_792_544_684_399, &eew("a", false, Scale::S3), &footer),
            "緊急地震速報を検知しました: 2026/10/21 10:04:44\n\
             発生源: 千葉県北西部 M4.3 深さ80km\n\
             予測最大震度: 3 (予報)\n\
             現在の状況は以下で確認できます。\n\
             \n\
             YouTube: https://youtube.com/@fugahoge873/live\n\
             Twitch: https://www.twitch.tv/fugahogeds"
        );
        // 震源が分からない報・足す行が無いとき
        let unknown = Eew {
            hypocenter: None,
            ..eew("a", true, Scale::S5_LOWER)
        };
        assert_eq!(
            post_text(1_792_544_684_399, &unknown, &[]),
            "緊急地震速報を検知しました: 2026/10/21 10:04:44\n発生源: 調査中\n予測最大震度: 5弱 (警報)"
        );
    }

    #[test]
    fn reads_which_targets_failed_from_a_200_response() {
        // 2026-10-21 に bluesky と x へ送ったときの実際の応答 (x は月の上限で失敗、並びは頼んだ順の逆)
        let body = json!({"success": false, "results": [
            {"success": false, "post_id": null, "error_message": "HTTP 403 Forbidden: {\"detail\":\"Your monthly spend cap has been reached.\"}"},
            {"success": true, "post_id": "at://did:plc:x/app.bsky.feed.post/y", "error_message": null}
        ]});
        let (total, failed) = failures(&body);
        assert_eq!(total, 2);
        assert_eq!(failed.len(), 1);
        assert!(failed[0].contains("spend cap"));
        assert_eq!(failures(&json!({"success": true, "results": []})), (0, vec![]));
        assert_eq!(failures(&serde_json::Value::Null), (0, vec![]));
    }

    #[tokio::test]
    async fn sends_the_key_and_targets_once_per_quake_and_the_cancellation() {
        use axum::http::HeaderMap;
        use axum::routing::post;
        use axum::{Json, Router};
        use std::sync::Arc;
        type Seen = Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>;
        let seen: Seen = Arc::default();
        let s = seen.clone();
        let app = Router::new().route(
            "/api/post",
            post(move |h: HeaderMap, Json(v): Json<serde_json::Value>| async move {
                let key = h
                    .get("x-api-key")
                    .and_then(|k| k.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                s.lock().unwrap().push((key, v));
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await });
        let sink = SocialSink {
            cfg: SocialConfig {
                url: format!("http://{addr}/api/post"),
                api_key_env: String::new(),
                targets: vec!["bluesky".into()],
                min_forecast: Scale::S3,
                footer: vec![],
            },
            key: "test-key".into(),
            client: crate::net::client_no_redirect(Duration::from_secs(5)).unwrap(),
            posted: Mutex::new(HashMap::new()),
        };
        let ev = |body: Eew, source: &str| Event {
            id: "x".into(),
            source: source.into(),
            received_at_ms: 1_792_544_684_399,
            body: EventBody::Eew(body),
        };
        // 第 1 報 (震度 2: 投稿しない) → 第 4 報 (震度 3: 投稿) → 第 7 報 (投稿しない) → 取り消し (投稿)
        sink.handle(&ev(eew("a", false, Scale::S2), "wolfx")).await.unwrap();
        sink.handle(&ev(eew("a", false, Scale::S3), "wolfx")).await.unwrap();
        sink.handle(&ev(eew("a", false, Scale::S3), "wolfx")).await.unwrap();
        // 記録の再生は投稿しない
        sink.handle(&ev(eew("b", true, Scale::S6_LOWER), "replay"))
            .await
            .unwrap();
        let cancel = Eew {
            cancelled: true,
            ..eew("a", false, Scale::UNKNOWN)
        };
        sink.handle(&ev(cancel.clone(), "wolfx")).await.unwrap();
        // 取り消しのあとに同じ取り消しがもう一度届いても投稿しない
        sink.handle(&ev(cancel, "p2pquake")).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen
            .iter()
            .all(|(k, v)| k == "test-key" && v["targets"] == serde_json::json!(["bluesky"])));
        assert!(seen[0].1["text"]
            .as_str()
            .unwrap()
            .starts_with("緊急地震速報を検知しました: 2026/10/21 10:04:44"));
        assert_eq!(
            seen[1].1["text"],
            "先ほどの緊急地震速報 (発生源: 千葉県北西部) は取り消されました。"
        );
    }
}
