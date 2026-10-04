//! Google・YouTube との通信。呼び出す側は `Api` trait だけを見る (テストは偽物を使い、本物の Google には触れない)。
//! 本物 (`Google`) は薄い HTTP の包み。応答の判断 (classify・parse_token_response) は純粋な関数に分けてある。
//! トークン・クライアントの秘密・Authorization ヘッダーは、ログにもエラーにも出さない。

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use super::token::{parse_token_response, AuthError, Client, Fresh};

const INSERT_URL: &str = "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status";
const PLAYLIST_ITEMS_URL: &str = "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet";

/// 1 本の動画の登録内容
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insert {
    pub title: String,
    pub description: String,
    /// "private" | "unlisted" | "public"
    pub privacy: String,
    pub category: String,
}

/// videos.insert の本文
pub fn insert_body(m: &Insert) -> Value {
    json!({
        "snippet": {
            "title": m.title,
            "description": m.description,
            "categoryId": m.category,
        },
        "status": {
            "privacyStatus": m.privacy,
            // 実写の映像ではなく、記録からの再現。子ども向けではない
            "selfDeclaredMadeForKids": false,
        },
    })
}

/// playlistItems.insert の本文 (再生リストの最後に足す)
pub fn playlist_item_body(playlist_id: &str, video_id: &str) -> Value {
    json!({
        "snippet": {
            "playlistId": playlist_id,
            "resourceId": { "kind": "youtube#video", "videoId": video_id },
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadError {
    /// その日の割り当て・上げられる本数を使い切った (太平洋時間の 0 時まで上げない)
    Quota,
    /// アクセストークンが通らない (更新して 1 回だけやり直す)
    Unauthorized,
    /// 5xx・通信の失敗。間を空けて、全体をやり直す
    Transient(String),
    /// ほかの 4xx。同じ内容では通らない
    Rejected(String),
}

/// 割り当ての超過を表す YouTube の理由 (error.errors[].reason)
const QUOTA_REASONS: [&str; 3] = ["quotaExceeded", "uploadLimitExceeded", "dailyLimitExceeded"];

/// 失敗した応答 (HTTP の状態と本文) を分類する
pub fn classify(status: u16, body: &str) -> UploadError {
    let reasons: Vec<String> = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/errors").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(|e| e.get("reason").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default();
    if reasons.iter().any(|r| QUOTA_REASONS.contains(&r.as_str())) {
        return UploadError::Quota;
    }
    match status {
        401 => UploadError::Unauthorized,
        408 | 429 | 500..=599 => UploadError::Transient(format!("HTTP {status}")),
        _ => UploadError::Rejected(format!("HTTP {status} {}", reasons.join(","))),
    }
}

/// YouTube への上げ方・トークンの更新
pub trait Api {
    /// リフレッシュトークンから、アクセストークンを取る
    async fn refresh(&self, client: &Client, refresh_token: &str, now_ms: u64) -> Result<Fresh, AuthError>;
    /// 動画を上げて、動画の ID を返す
    async fn upload(&self, access_token: &str, meta: &Insert, video: &Path) -> Result<String, UploadError>;
    /// 上げた動画を再生リストに足す
    async fn add_to_playlist(&self, access_token: &str, playlist_id: &str, video_id: &str) -> Result<(), UploadError>;
}

/// 本物の Google
pub struct Google {
    http: reqwest::Client,
}

impl Google {
    pub fn new() -> anyhow::Result<Google> {
        Ok(Google {
            // 動画は大きいので、全体の上限は長め。つながらないときは早く諦める
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(20))
                .timeout(Duration::from_secs(3600))
                .build()?,
        })
    }

    /// 認可コードをトークンに換える (youtube-auth)
    pub async fn exchange(
        &self,
        client: &Client,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
        now_ms: u64,
    ) -> Result<Fresh, AuthError> {
        self.token_request(
            client,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", verifier),
                ("redirect_uri", redirect_uri),
            ],
            now_ms,
        )
        .await
    }

    async fn token_request(&self, client: &Client, extra: &[(&str, &str)], now_ms: u64) -> Result<Fresh, AuthError> {
        let mut form: Vec<(&str, &str)> = vec![
            ("client_id", &client.client_id),
            ("client_secret", &client.client_secret),
        ];
        form.extend_from_slice(extra);
        let res = self
            .http
            .post(&client.token_uri)
            .timeout(Duration::from_secs(30))
            .form(&form)
            .send()
            .await
            .map_err(|e| AuthError::Other(format!("トークンの通信に失敗しました: {}", e.without_url())))?;
        let status = res.status().as_u16();
        let body = res
            .text()
            .await
            .map_err(|e| AuthError::Other(format!("トークンの応答を読めません: {}", e.without_url())))?;
        parse_token_response(status, &body, now_ms)
    }
}

fn transient(e: reqwest::Error) -> UploadError {
    UploadError::Transient(e.without_url().to_string())
}

impl Api for Google {
    async fn refresh(&self, client: &Client, refresh_token: &str, now_ms: u64) -> Result<Fresh, AuthError> {
        self.token_request(
            client,
            &[("grant_type", "refresh_token"), ("refresh_token", refresh_token)],
            now_ms,
        )
        .await
    }

    // ponytail: 全体を 1 回の PUT で送る (途中からの再開はしない)。失敗したら最初から。
    // 数十 MB の動画なら足りる。長い動画で困るようになったら、Content-Range での再開に変える
    async fn upload(&self, access_token: &str, meta: &Insert, video: &Path) -> Result<String, UploadError> {
        let bytes = tokio::fs::read(video)
            .await
            .map_err(|e| UploadError::Rejected(format!("動画を読めません: {e}")))?;
        let init = self
            .http
            .post(INSERT_URL)
            .bearer_auth(access_token)
            .header("X-Upload-Content-Type", "video/mp4")
            .header("X-Upload-Content-Length", bytes.len())
            .json(&insert_body(meta))
            .send()
            .await
            .map_err(transient)?;
        if !init.status().is_success() {
            let status = init.status().as_u16();
            return Err(classify(status, &init.text().await.unwrap_or_default()));
        }
        let location = init
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| UploadError::Transient("上げる先の URL が返りませんでした".into()))?;
        let put = self
            .http
            .put(location)
            .header(reqwest::header::CONTENT_TYPE, "video/mp4")
            .body(bytes)
            .send()
            .await
            .map_err(transient)?;
        let status = put.status().as_u16();
        let body = put.text().await.map_err(transient)?;
        if !(200..300).contains(&status) {
            return Err(classify(status, &body));
        }
        serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
            .ok_or_else(|| UploadError::Transient("応答に動画の ID がありません".into()))
    }

    async fn add_to_playlist(&self, access_token: &str, playlist_id: &str, video_id: &str) -> Result<(), UploadError> {
        let res = self
            .http
            .post(PLAYLIST_ITEMS_URL)
            .bearer_auth(access_token)
            .timeout(Duration::from_secs(30))
            .json(&playlist_item_body(playlist_id, video_id))
            .send()
            .await
            .map_err(transient)?;
        let status = res.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(classify(status, &res.text().await.unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err_body(reason: &str) -> String {
        format!(
            r#"{{"error":{{"code":403,"message":"m","errors":[{{"reason":"{reason}","domain":"youtube.quota"}}]}}}}"#
        )
    }

    #[test]
    fn quota_reasons_stop_uploading_for_the_day() {
        for r in ["quotaExceeded", "uploadLimitExceeded", "dailyLimitExceeded"] {
            assert_eq!(classify(403, &err_body(r)), UploadError::Quota, "{r}");
        }
    }

    #[test]
    fn server_trouble_is_transient_and_other_client_errors_are_rejections() {
        assert!(matches!(classify(503, ""), UploadError::Transient(_)));
        assert!(matches!(classify(500, "<html>"), UploadError::Transient(_)));
        assert!(matches!(classify(429, ""), UploadError::Transient(_)));
        assert_eq!(classify(401, ""), UploadError::Unauthorized);
        match classify(400, &err_body("invalidDescription")) {
            UploadError::Rejected(m) => assert!(m.contains("invalidDescription")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            classify(403, &err_body("forbidden")),
            UploadError::Rejected(_)
        ));
    }

    #[test]
    fn the_insert_body_has_the_privacy_and_category() {
        let b = insert_body(&Insert {
            title: "T".into(),
            description: "D".into(),
            privacy: "private".into(),
            category: "25".into(),
        });
        assert_eq!(b["snippet"]["title"], "T");
        assert_eq!(b["snippet"]["categoryId"], "25");
        assert_eq!(b["status"]["privacyStatus"], "private");
    }

    #[test]
    fn the_playlist_item_body_points_at_the_video() {
        let b = playlist_item_body("PLx", "VID");
        assert_eq!(b["snippet"]["playlistId"], "PLx");
        assert_eq!(b["snippet"]["resourceId"]["kind"], "youtube#video");
        assert_eq!(b["snippet"]["resourceId"]["videoId"], "VID");
    }
}
