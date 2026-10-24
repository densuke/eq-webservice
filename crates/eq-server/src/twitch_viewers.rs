//! Twitch ライブの同時視聴者数。`[viewers]` に twitch_client_id と twitch_login があるときだけ、一定間隔で
//! Helix API (streams) から取り、youtube_viewers の Shared に入れる (合計は `GET /api/viewers` が出す)。
//!
//! 認証はアプリのアクセストークン (client credentials)。client_secret は環境変数 TWITCH_CLIENT_SECRET で渡し、
//! トークンとあわせてヘッダか本文にだけ載せる (URL・ログ・エラー文に出さない)。

use std::time::Duration;

use anyhow::Context;
use serde_json::Value;

use crate::youtube_viewers::{set_twitch, Shared, ViewersConfig};

const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const STREAMS_URL: &str = "https://api.twitch.tv/helix/streams";
const SECRET_ENV: &str = "TWITCH_CLIENT_SECRET";

/// streams の応答から視聴者数を読む。配信していなければ data が空で None
pub fn parse_viewer_count(v: &Value) -> Option<u64> {
    v.pointer("/data/0/viewer_count")?.as_u64()
}

async fn app_token(client: &reqwest::Client, id: &str, secret: &str) -> anyhow::Result<String> {
    let res: Value = crate::net::json(client.post(TOKEN_URL).form(&[
        ("client_id", id),
        ("client_secret", secret),
        ("grant_type", "client_credentials"),
    ]))
    .await
    .context("getting the app access token")?;
    res.get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("no access_token")
}

/// 1 回取る。トークンは数十日もつので使い回し、無ければ取る
async fn tick(
    client: &reqwest::Client,
    cfg: &ViewersConfig,
    secret: &str,
    token: &mut Option<String>,
) -> anyhow::Result<Option<u64>> {
    let t = match token {
        Some(t) => t.clone(),
        None => token
            .insert(app_token(client, &cfg.twitch_client_id, secret).await?)
            .clone(),
    };
    // twitch_login は check で英数字と _ に限っている
    let url = format!("{STREAMS_URL}?user_login={}", cfg.twitch_login);
    let v: Value = crate::net::json(
        client
            .get(&url)
            .header("client-id", &cfg.twitch_client_id)
            .bearer_auth(t),
    )
    .await?;
    Ok(parse_viewer_count(&v))
}

pub fn spawn(cfg: ViewersConfig, shared: Shared) {
    if cfg.twitch_login.is_empty() {
        return;
    }
    let Ok(secret) = std::env::var(SECRET_ENV) else {
        return tracing::warn!("viewers: {SECRET_ENV} が無いので Twitch の同接は取りません");
    };
    tokio::spawn(async move {
        let client = match crate::net::client(Duration::from_secs(20)) {
            Ok(c) => c,
            Err(e) => return tracing::warn!("viewers (twitch): {e:#}"),
        };
        let mut token = None;
        loop {
            match tick(&client, &cfg, &secret, &mut token).await {
                Ok(n) => set_twitch(&shared, n),
                Err(e) => {
                    // 期限切れ・取り消しに備えて、次はトークンから取り直す。取れなかった間は合計に入れない
                    tracing::warn!("viewers (twitch): {e:#}");
                    token = None;
                    set_twitch(&shared, None);
                }
            }
            tokio::time::sleep(cfg.interval()).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_viewer_count_while_live() {
        let v = json!({"data":[{"user_login":"x","type":"live","viewer_count":42}],"pagination":{}});
        assert_eq!(parse_viewer_count(&v), Some(42));
    }

    #[test]
    fn offline_or_odd_response_is_none() {
        assert_eq!(parse_viewer_count(&json!({"data":[],"pagination":{}})), None);
        assert_eq!(parse_viewer_count(&json!({})), None);
        assert_eq!(parse_viewer_count(&json!({"data":[{"viewer_count":"3"}]})), None);
    }
}
