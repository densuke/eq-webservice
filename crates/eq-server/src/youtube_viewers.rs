//! YouTube ライブの同時視聴者数。設定で有効なときだけ、一定間隔で YouTube Data API (videos.list) から取って保持し、
//! `GET /api/viewers` で返す (外の API を呼ぶのはここだけ)。取れない・ライブでない・非表示のときは viewers が null。
//!
//! 認証は API キー (`api_key`) か、OAuth の資格情報 JSON (`token_file`。refresh_token でアクセストークンを取り直す)。
//! 動画は `video_id` で固定するか、`channel_id` で今の live を探す (見つけたら覚え、ライブでなくなったときだけ探し直す)。
//! API キーとトークンはヘッダで送り (URL に載せない)、ログにもエラー文にも出さない。

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const API: &str = "https://www.googleapis.com/youtube/v3";
/// API キーで探す search.list は 1 回 100 単位 (1 日 10,000 単位) なので、探し直しはこの間隔より詰めない
/// (30 分ごとで 1 日 4,800 単位。videos.list の 1,440 単位と合わせて 6,240 単位)
const SEARCH_GAP_KEY: Duration = Duration::from_secs(1800);
/// 取得の間隔の範囲 (秒)。上は web が古い値を捨てる 5 分 (STALE_MS) と合わせる
const INTERVAL_MIN: u64 = 10;
const INTERVAL_MAX: u64 = 300;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ViewersConfig {
    pub enabled: bool,
    /// 取得の間隔 (秒。10 未満は 10)
    pub interval_sec: u64,
    /// API キー (視聴者数は公開の値なのでキーで取れる)
    pub api_key: String,
    /// OAuth の資格情報 JSON (client_id・client_secret・refresh_token・token_uri)
    pub token_file: String,
    /// 動画 ID を固定する
    pub video_id: String,
    /// その時の live の動画を探す
    pub channel_id: String,
}

impl Default for ViewersConfig {
    fn default() -> Self {
        ViewersConfig {
            enabled: false,
            interval_sec: 60,
            api_key: String::new(),
            token_file: String::new(),
            video_id: String::new(),
            channel_id: String::new(),
        }
    }
}

/// api_key は伏せる (設定を丸ごとログに出しても漏れない)
impl std::fmt::Debug for ViewersConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewersConfig")
            .field("enabled", &self.enabled)
            .field("interval_sec", &self.interval_sec)
            .field("api_key", &if self.api_key.is_empty() { "" } else { "<redacted>" })
            .field("token_file", &self.token_file)
            .field("video_id", &self.video_id)
            .field("channel_id", &self.channel_id)
            .finish()
    }
}

impl ViewersConfig {
    /// 取得の間隔。10 秒から 300 秒に収める
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_sec.clamp(INTERVAL_MIN, INTERVAL_MAX))
    }

    /// 有効なのに足りない・重なっている設定を、起動時に知らせる
    pub fn check(&self) -> Result<(), &'static str> {
        if self.api_key.is_empty() == self.token_file.is_empty() {
            return Err("viewers: api_key か token_file のどちらか 1 つを指定してください");
        }
        if self.video_id.is_empty() == self.channel_id.is_empty() {
            return Err("viewers: video_id か channel_id のどちらか 1 つを指定してください");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Viewers {
    pub viewers: Option<u64>,
    pub video_id: String,
    pub updated_ms: i64,
}

pub type Shared = Arc<RwLock<Viewers>>;

/// 動画の状態。通信の失敗は状態にしない (前回のまま)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// ライブ中。同時視聴者数 (非表示なら None)
    Live(Option<u64>),
    /// 動画が無い・ライブが終わった・まだ始まっていない
    Gone,
}

/// videos.list?part=liveStreamingDetails の応答から状態を読む
pub fn parse_status(v: &Value) -> Status {
    let Some(d) = v.pointer("/items/0/liveStreamingDetails") else {
        return Status::Gone;
    };
    if d.get("actualEndTime").is_some() || d.get("actualStartTime").is_none() {
        return Status::Gone;
    }
    // concurrentViewers は数字の文字列
    Status::Live(
        d.get("concurrentViewers")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok()),
    )
}

/// 動画 ID を探した応答 (liveBroadcasts は items[0].id、search は items[0].id.videoId) から ID を取り出す
pub fn parse_found_id(v: &Value) -> Option<String> {
    let id = v.pointer("/items/0/id")?;
    let s = id.as_str().or_else(|| id.get("videoId")?.as_str())?;
    (!s.is_empty()).then(|| s.to_string())
}

/// 動画 ID を探す必要があるか。覚えた ID が無いか、ライブが終わっていて、前回の探索から間隔が空いているとき
pub fn should_search(
    cached: Option<&str>,
    last: Option<Status>,
    since_search: Option<Duration>,
    gap: Duration,
) -> bool {
    let lost = cached.is_none() || last == Some(Status::Gone);
    lost && since_search.is_none_or(|d| d >= gap)
}

#[derive(Deserialize)]
struct Cred {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    token_uri: String,
}

/// 認証のヘッダ (名前, 値) と、OAuth かどうか
async fn auth_header(client: &reqwest::Client, cfg: &ViewersConfig) -> anyhow::Result<(&'static str, String, bool)> {
    if !cfg.api_key.is_empty() {
        return Ok(("x-goog-api-key", cfg.api_key.clone(), false));
    }
    let raw = tokio::fs::read(&cfg.token_file).await.context("reading token_file")?;
    let c: Cred = serde_json::from_slice(&raw).context("parsing token_file")?;
    let res: Value = crate::net::json(client.post(&c.token_uri).form(&[
        ("client_id", c.client_id.as_str()),
        ("client_secret", c.client_secret.as_str()),
        ("refresh_token", c.refresh_token.as_str()),
        ("grant_type", "refresh_token"),
    ]))
    .await
    .context("refreshing the access token")?;
    let t = res
        .get("access_token")
        .and_then(Value::as_str)
        .context("no access_token")?;
    Ok(("authorization", format!("Bearer {t}"), true))
}

async fn get_json(client: &reqwest::Client, h: (&str, &str), url: &str) -> anyhow::Result<Value> {
    crate::net::json(client.get(url).header(h.0, h.1)).await
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn set(shared: &Shared, viewers: Option<u64>, video: &str) {
    *shared.write().unwrap() = Viewers {
        viewers,
        video_id: video.to_string(),
        updated_ms: now_ms(),
    };
}

/// 取得の状態 (覚えた動画 ID・前回の状態・最後に探した時刻)
struct State {
    video: String,
    last: Option<Status>,
    searched: Option<Instant>,
}

async fn tick(client: &reqwest::Client, cfg: &ViewersConfig, shared: &Shared, st: &mut State) -> anyhow::Result<()> {
    let (name, value, oauth) = auth_header(client, cfg).await?;
    let h = (name, value.as_str());
    let gap = if oauth { Duration::ZERO } else { SEARCH_GAP_KEY };
    let cached = (!st.video.is_empty()).then_some(st.video.as_str());
    if cfg.video_id.is_empty() && should_search(cached, st.last, st.searched.map(|t| t.elapsed()), gap) {
        st.searched = Some(Instant::now());
        let url = if oauth {
            format!("{API}/liveBroadcasts?part=id&broadcastStatus=active&broadcastType=all&maxResults=1")
        } else {
            format!(
                "{API}/search?part=id&channelId={}&eventType=live&type=video&maxResults=1",
                cfg.channel_id
            )
        };
        st.video = parse_found_id(&get_json(client, h, &url).await?).unwrap_or_default();
        st.last = None;
    }
    if st.video.is_empty() {
        set(shared, None, "");
        return Ok(());
    }
    let v = get_json(
        client,
        h,
        &format!("{API}/videos?part=liveStreamingDetails&id={}", st.video),
    )
    .await?;
    let status = parse_status(&v);
    st.last = Some(status);
    set(shared, if let Status::Live(n) = status { n } else { None }, &st.video);
    Ok(())
}

pub fn spawn(cfg: ViewersConfig, shared: Shared) {
    tokio::spawn(async move {
        let client = match crate::net::client(Duration::from_secs(20)) {
            Ok(c) => c,
            Err(e) => return tracing::warn!("viewers: {e:#}"),
        };
        // 有効なら updated_ms は 0 でなくなる (web は 0 を「無効」と見て取りに行かなくなる)
        set(&shared, None, &cfg.video_id);
        let mut st = State {
            video: cfg.video_id.clone(),
            last: None,
            searched: None,
        };
        loop {
            if let Err(e) = tick(&client, &cfg, &shared, &mut st).await {
                // 秘密はヘッダにしか載せていないので、エラー文には出ない。取れなかった間は表示を消す
                tracing::warn!("viewers: {e:#}");
                set(&shared, None, &st.video);
            }
            tokio::time::sleep(Duration::from_secs(cfg.interval().as_secs())).await;
        }
    });
}

pub fn router(shared: Shared) -> Router {
    Router::new().route(
        "/api/viewers",
        get(move || async move { Json(shared.read().unwrap().clone()) }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_concurrent_viewers_while_live() {
        let v = json!({"items":[{"liveStreamingDetails":{"actualStartTime":"2026-10-07T00:00:00Z","concurrentViewers":"12"}}]});
        assert_eq!(parse_status(&v), Status::Live(Some(12)));
    }

    #[test]
    fn hidden_count_is_live_without_a_number() {
        let v = json!({"items":[{"liveStreamingDetails":{"actualStartTime":"2026-10-07T00:00:00Z"}}]});
        assert_eq!(parse_status(&v), Status::Live(None));
    }

    #[test]
    fn ended_missing_or_not_started_is_gone() {
        let ended = json!({"items":[{"liveStreamingDetails":{"actualStartTime":"a","actualEndTime":"b"}}]});
        assert_eq!(parse_status(&ended), Status::Gone);
        assert_eq!(parse_status(&json!({"items":[]})), Status::Gone);
        let upcoming = json!({"items":[{"liveStreamingDetails":{"scheduledStartTime":"a"}}]});
        assert_eq!(parse_status(&upcoming), Status::Gone);
        assert_eq!(parse_status(&json!({"items":[{}]})), Status::Gone);
    }

    #[test]
    fn finds_the_video_id_in_both_response_shapes() {
        assert_eq!(parse_found_id(&json!({"items":[{"id":"abc"}]})).as_deref(), Some("abc"));
        assert_eq!(
            parse_found_id(&json!({"items":[{"id":{"kind":"k","videoId":"xyz"}}]})).as_deref(),
            Some("xyz")
        );
        assert_eq!(parse_found_id(&json!({"items":[]})), None);
    }

    #[test]
    fn searches_only_when_the_id_is_lost_and_the_gap_has_passed() {
        let gap = Duration::from_secs(600);
        assert!(should_search(None, None, None, gap));
        assert!(!should_search(Some("a"), Some(Status::Live(Some(1))), None, gap));
        // 非表示 (Live(None)) だけでは探し直さない
        assert!(!should_search(Some("a"), Some(Status::Live(None)), None, gap));
        assert!(should_search(Some("a"), Some(Status::Gone), None, gap));
        assert!(!should_search(
            Some("a"),
            Some(Status::Gone),
            Some(Duration::from_secs(60)),
            gap
        ));
        assert!(should_search(None, None, Some(Duration::from_secs(601)), gap));
    }

    #[test]
    fn config_needs_one_auth_and_one_target() {
        let ok = ViewersConfig {
            api_key: "k".into(),
            video_id: "v".into(),
            ..Default::default()
        };
        assert!(ok.check().is_ok());
        assert!(ViewersConfig {
            video_id: "v".into(),
            ..Default::default()
        }
        .check()
        .is_err());
        assert!(ViewersConfig {
            api_key: "k".into(),
            ..Default::default()
        }
        .check()
        .is_err());
        let both = ViewersConfig {
            api_key: "k".into(),
            token_file: "t".into(),
            video_id: "v".into(),
            ..Default::default()
        };
        assert!(both.check().is_err());
    }

    #[test]
    fn interval_is_kept_between_10_and_300_seconds() {
        let at = |s| {
            ViewersConfig {
                interval_sec: s,
                ..Default::default()
            }
            .interval()
            .as_secs()
        };
        assert_eq!((at(1), at(60), at(300), at(9999)), (10, 60, 300, 300));
    }

    #[test]
    fn debug_hides_the_api_key() {
        let c = ViewersConfig {
            api_key: "SECRET".into(),
            ..Default::default()
        };
        assert!(!format!("{c:?}").contains("SECRET"));
    }

    #[test]
    fn config_parses_from_toml() {
        let c: ViewersConfig = toml::from_str("enabled = true\ntoken_file = \"t.json\"\nchannel_id = \"UC1\"").unwrap();
        assert!(c.enabled && c.interval_sec == 60 && c.check().is_ok());
    }
}
