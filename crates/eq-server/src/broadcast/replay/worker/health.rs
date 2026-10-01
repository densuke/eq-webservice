//! YouTube のライブの受け口の健全性 (docs/replay-video.md 5.1)。作っている間だけ、1 分ごとに見る。
//! deploy/youtube_live_check.py と同じ読み取り (liveStreams.list。割り当ては 1 回 1 単位)。
//! 認証情報 (google-auth の authorized user の JSON) は読むだけで、書き戻さない。アクセストークンはメモリの中だけに置き、ログにも出さない。
//! 読めない (通信・API の不調、認証情報が無い) ときは Unknown にして、止める理由にしない。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;

const API: &str = "https://www.googleapis.com/youtube/v3/liveStreams?part=snippet,status&mine=true";
/// 期限のこの時間前になったら、トークンを取り直す
const TOKEN_MARGIN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Good,
    /// 受け口に映像が届いていない、または健全性が bad・noData
    Bad,
    /// 読めない
    Unknown,
}

/// liveStreams.list の応答から、受け口の健全性を決める (純粋な関数)。name が空なら名前は問わない。
/// 該当する受け口が無ければ Unknown (名前の設定違いで、いつまでも作れなくならないように)。
/// 受信中でなければ、また受信中でも健全性が bad・noData なら Bad
pub fn judge(response: &Value, name: &str) -> Health {
    let streams: Vec<&Value> = response["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| name.is_empty() || s["snippet"]["title"] == name)
        .collect();
    if streams.is_empty() {
        return Health::Unknown;
    }
    let Some(active) = streams.iter().find(|s| s["status"]["streamStatus"] == "active") else {
        return Health::Bad;
    };
    match active["status"]["healthStatus"]["status"].as_str() {
        Some("bad" | "noData") => Health::Bad,
        _ => Health::Good,
    }
}

#[derive(Deserialize)]
struct Credentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    token_uri: String,
}

struct Token {
    value: String,
    expires: Instant,
}

/// 健全性を見る人。結果は every の間覚えておく
pub struct Checker {
    credentials: PathBuf,
    stream_name: String,
    every: Duration,
    client: reqwest::Client,
    token: Option<Token>,
    last: Option<(Instant, Health)>,
}

impl Checker {
    pub fn new(credentials: PathBuf, stream_name: String, every: Duration) -> anyhow::Result<Self> {
        Ok(Checker {
            credentials,
            stream_name,
            every,
            client: crate::net::client(Duration::from_secs(20))?,
            token: None,
            last: None,
        })
    }

    /// 見る間隔のうちは、前の結果を返す。読めなかったときは Unknown (警告は出すが、認証情報の中身は出さない)
    pub async fn check(&mut self) -> Health {
        if let Some((at, h)) = self.last {
            if at.elapsed() < self.every {
                return h;
            }
        }
        let h = match self.fetch().await {
            Ok(v) => judge(&v, &self.stream_name),
            Err(e) => {
                tracing::warn!("replay-worker: YouTube の健全性を読めません (止める理由にはしません): {e:#}");
                Health::Unknown
            }
        };
        self.last = Some((Instant::now(), h));
        h
    }

    async fn fetch(&mut self) -> anyhow::Result<Value> {
        let token = self.access_token().await?;
        let res = crate::net::json(self.client.get(API).bearer_auth(token)).await;
        if res.is_err() {
            // 認証が切れたのかもしれないので、次は取り直す
            self.token = None;
        }
        res.context("liveStreams.list")
    }

    /// 期限が近ければ、リフレッシュトークンから取り直す
    async fn access_token(&mut self) -> anyhow::Result<String> {
        if let Some(t) = &self.token {
            if t.expires > Instant::now() + TOKEN_MARGIN {
                return Ok(t.value.clone());
            }
        }
        let cred: Credentials = serde_json::from_slice(
            &std::fs::read(&self.credentials).with_context(|| format!("reading {}", self.credentials.display()))?,
        )
        .context("the credentials are not in the authorized user form")?;
        let res: Value = crate::net::json(self.client.post(&cred.token_uri).form(&[
            ("client_id", cred.client_id.as_str()),
            ("client_secret", cred.client_secret.as_str()),
            ("refresh_token", cred.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ]))
        .await
        .context("refreshing the access token")?;
        let value = res["access_token"]
            .as_str()
            .context("no access_token in the response")?
            .to_string();
        let secs = res["expires_in"].as_u64().unwrap_or(300);
        self.token = Some(Token {
            value: value.clone(),
            expires: Instant::now() + Duration::from_secs(secs),
        });
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stream(title: &str, status: &str, health: &str) -> Value {
        json!({"snippet": {"title": title}, "status": {"streamStatus": status, "healthStatus": {"status": health}}})
    }

    #[test]
    fn an_active_stream_is_good_unless_its_health_is_bad_or_no_data() {
        let one = |h| json!({"items": [stream("地震モニター用", "active", h)]});
        assert_eq!(judge(&one("good"), "地震モニター用"), Health::Good);
        assert_eq!(judge(&one("ok"), "地震モニター用"), Health::Good);
        assert_eq!(judge(&one("bad"), "地震モニター用"), Health::Bad);
        assert_eq!(judge(&one("noData"), "地震モニター用"), Health::Bad);
    }

    #[test]
    fn a_stream_that_is_not_receiving_is_bad() {
        let r = json!({"items": [stream("地震モニター用", "inactive", "noData")]});
        assert_eq!(judge(&r, "地震モニター用"), Health::Bad);
    }

    #[test]
    fn only_the_named_stream_counts_and_a_missing_one_is_unknown() {
        let r = json!({"items": [stream("pd2", "inactive", "noData"), stream("地震モニター用", "active", "good")]});
        assert_eq!(judge(&r, "地震モニター用"), Health::Good);
        assert_eq!(judge(&r, "別の名前"), Health::Unknown);
        // 名前が空なら問わない (受信中のものがあれば Good)
        assert_eq!(judge(&r, ""), Health::Good);
        assert_eq!(judge(&json!({}), ""), Health::Unknown);
        assert_eq!(judge(&json!({"items": []}), "x"), Health::Unknown);
    }

    #[tokio::test]
    async fn unreadable_credentials_make_it_unknown_not_bad() {
        let mut c = Checker::new(
            PathBuf::from("/nonexistent/token.json"),
            String::new(),
            Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(c.check().await, Health::Unknown);
        // 見る間隔のうちは、読み直さない
        assert!(c.last.is_some());
    }
}
