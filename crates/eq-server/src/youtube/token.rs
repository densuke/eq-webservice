//! クライアントの JSON (Google の "installed" 形式) と、トークンのファイル。
//! 秘密を持つ型には Debug を付けない (ログや assert に出ないように)。

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// 動画を上げる権限だけ (読み書き・削除はできない)
pub const SCOPE: &str = "https://www.googleapis.com/auth/youtube.upload";
const DEFAULT_AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
/// アクセストークンの期限の、この時間 (ミリ秒) 前から、期限切れとみなす
const EXPIRY_MARGIN_MS: u64 = 60_000;

#[derive(Clone)]
pub struct Client {
    pub client_id: String,
    pub client_secret: String,
    pub auth_uri: String,
    pub token_uri: String,
}

#[derive(Deserialize)]
struct RawClient {
    client_id: String,
    client_secret: String,
    #[serde(default)]
    auth_uri: Option<String>,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Deserialize)]
struct ClientFile {
    installed: Option<RawClient>,
}

/// クライアントの JSON を読む。{"installed": {...}} の形 (デスクトップアプリ用)
pub fn parse_client(json: &str) -> anyhow::Result<Client> {
    let file: ClientFile = serde_json::from_str(json).context("クライアントの JSON を読めません")?;
    let c = file
        .installed
        .context("クライアントの JSON に \"installed\" がありません (Google Cloud でデスクトップアプリとして作ったものを使ってください)")?;
    Ok(Client {
        client_id: c.client_id,
        client_secret: c.client_secret,
        auth_uri: c.auth_uri.unwrap_or_else(|| DEFAULT_AUTH_URI.into()),
        token_uri: c.token_uri.unwrap_or_else(|| DEFAULT_TOKEN_URI.into()),
    })
}

pub fn load_client(path: &Path) -> anyhow::Result<Client> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_client(&text).with_context(|| path.display().to_string())
}

/// トークンのファイルの中身
#[derive(Clone, Serialize, Deserialize)]
pub struct TokenFile {
    pub refresh_token: String,
    /// 前に取ったアクセストークンと期限 (epoch ミリ秒)。まだ使えれば更新を省く
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
}

impl TokenFile {
    /// まだ使えるアクセストークン
    pub fn usable_access(&self, now_ms: u64) -> Option<&str> {
        match (&self.access_token, self.expires_at_ms) {
            (Some(t), Some(exp)) if exp > now_ms + EXPIRY_MARGIN_MS => Some(t),
            _ => None,
        }
    }
}

/// トークンのエンドポイントが返したもの
pub struct Fresh {
    pub access_token: String,
    pub expires_at_ms: u64,
    /// 新しいリフレッシュトークン (更新のときは、ふつう返らない)
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// リフレッシュトークンが取り消された・期限切れ。`youtube-auth` のやり直しが要る
    InvalidGrant,
    /// 通信の失敗など。あとでやり直せばよい (応答の本文は、秘密を含みうるので入れない)
    Other(String),
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// トークンのエンドポイントの応答を読む (取得も更新も同じ形)
pub fn parse_token_response(status: u16, body: &str, now_ms: u64) -> Result<Fresh, AuthError> {
    let r: TokenResponse = serde_json::from_str(body)
        .map_err(|_| AuthError::Other(format!("トークンの応答を読めません (HTTP {status})")))?;
    match (r.error.as_deref(), r.access_token) {
        (Some("invalid_grant"), _) => Err(AuthError::InvalidGrant),
        (Some(e), _) => Err(AuthError::Other(format!("トークンの取得に失敗しました: {e}"))),
        (None, Some(access_token)) if (200..300).contains(&status) => Ok(Fresh {
            access_token,
            expires_at_ms: now_ms + r.expires_in.unwrap_or(3600) * 1000,
            refresh_token: r.refresh_token,
        }),
        _ => Err(AuthError::Other(format!(
            "トークンの取得に失敗しました (HTTP {status})"
        ))),
    }
}

pub fn load_token(path: &Path) -> anyhow::Result<TokenFile> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("トークンのファイルを読めません: {}", path.display()))
}

/// トークンのファイルを、モード 0600 で書く (別名で書いて置き換える。書いている間も他人に読ませない)
pub fn save_token(path: &Path, token: &TokenFile) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("writing {}", tmp.display()))?;
    f.write_all(&serde_json::to_vec_pretty(token)?)?;
    f.sync_all()?;
    // umask に左右されないよう、権限を明示する
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    #[test]
    fn the_installed_client_format_is_read_with_defaults() {
        let c = parse_client(
            r#"{"installed":{"client_id":"id","client_secret":"s","redirect_uris":["http://localhost"]}}"#,
        )
        .unwrap();
        assert_eq!((c.client_id.as_str(), c.client_secret.as_str()), ("id", "s"));
        assert_eq!(c.token_uri, "https://oauth2.googleapis.com/token");
        let c = parse_client(
            r#"{"installed":{"client_id":"i","client_secret":"s","auth_uri":"https://a","token_uri":"https://t"}}"#,
        )
        .unwrap();
        assert_eq!((c.auth_uri.as_str(), c.token_uri.as_str()), ("https://a", "https://t"));
        assert!(parse_client(r#"{"web":{"client_id":"i","client_secret":"s"}}"#).is_err());
        assert!(parse_client("nope").is_err());
    }

    #[test]
    fn a_good_response_gives_an_access_token_with_its_expiry() {
        let f = parse_token_response(
            200,
            r#"{"access_token":"AT","expires_in":3599,"token_type":"Bearer"}"#,
            NOW,
        )
        .ok()
        .unwrap();
        assert_eq!((f.access_token.as_str(), f.expires_at_ms), ("AT", NOW + 3_599_000));
        assert!(f.refresh_token.is_none());
        let f = parse_token_response(
            200,
            r#"{"access_token":"AT","expires_in":10,"refresh_token":"RT"}"#,
            NOW,
        )
        .ok()
        .unwrap();
        assert_eq!(f.refresh_token.as_deref(), Some("RT"));
    }

    #[test]
    fn invalid_grant_is_told_apart_from_other_failures() {
        let e = parse_token_response(
            400,
            r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
            NOW,
        );
        assert_eq!(e.err(), Some(AuthError::InvalidGrant));
        let e = parse_token_response(400, r#"{"error":"invalid_client"}"#, NOW)
            .err()
            .unwrap();
        assert!(matches!(e, AuthError::Other(m) if m.contains("invalid_client")));
        assert!(matches!(
            parse_token_response(503, "<html>", NOW).err(),
            Some(AuthError::Other(_))
        ));
        assert!(matches!(
            parse_token_response(200, "{}", NOW).err(),
            Some(AuthError::Other(_))
        ));
    }

    #[test]
    fn an_access_token_is_usable_until_a_minute_before_its_expiry() {
        let t = TokenFile {
            refresh_token: "RT".into(),
            access_token: Some("AT".into()),
            expires_at_ms: Some(NOW + 61_000),
        };
        assert_eq!(t.usable_access(NOW), Some("AT"));
        assert_eq!(t.usable_access(NOW + 2_000), None);
        let none = TokenFile {
            access_token: None,
            ..t.clone()
        };
        assert_eq!(none.usable_access(NOW), None);
    }

    #[test]
    fn the_token_file_is_written_with_mode_0600_even_over_an_old_loose_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let t = TokenFile {
            refresh_token: "RT".into(),
            access_token: Some("AT".into()),
            expires_at_ms: Some(5),
        };
        save_token(&path, &t).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let back = load_token(&path).unwrap();
        assert_eq!((back.refresh_token.as_str(), back.expires_at_ms), ("RT", Some(5)));
    }

    #[test]
    fn a_token_file_with_only_a_refresh_token_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.json");
        std::fs::write(&path, r#"{"refresh_token":"RT"}"#).unwrap();
        assert!(load_token(&path).unwrap().usable_access(NOW).is_none());
        assert!(load_token(&dir.path().join("missing.json")).is_err());
    }
}
