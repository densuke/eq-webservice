//! OAuth の PKCE (RFC 7636) と、同意の URL。乱数は OS から取る (/dev/urandom)。

use std::io::Read;

use anyhow::Context;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

use super::token::Client;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    /// リダイレクトの取り違えを防ぐ (CSRF) ための値
    pub state: String,
}

fn random_b64(n: usize) -> anyhow::Result<String> {
    let mut buf = vec![0u8; n];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .context("reading /dev/urandom")?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// verifier から challenge (S256) を作る
pub fn challenge_of(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Pkce {
    pub fn generate() -> anyhow::Result<Pkce> {
        // 32 バイト -> 43 文字 (RFC は 43〜128 文字)
        let verifier = random_b64(32)?;
        Ok(Pkce {
            challenge: challenge_of(&verifier),
            verifier,
            state: random_b64(16)?,
        })
    }
}

/// URL に入れる文字の変換 (予約されていない文字以外を %XX にする)
pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 同意の画面の URL。リフレッシュトークンをもらうため、offline・consent を付ける
pub fn consent_url(client: &Client, redirect_uri: &str, scope: &str, p: &Pkce) -> String {
    let q = [
        ("client_id", client.client_id.as_str()),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", scope),
        ("code_challenge", p.challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", p.state.as_str()),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];
    let query: Vec<String> = q.iter().map(|(k, v)| format!("{k}={}", urlencode(v))).collect();
    format!("{}?{}", client.auth_uri, query.join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_the_rfc_7636_example() {
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_values_are_valid_and_different_each_time() {
        let a = Pkce::generate().unwrap();
        let b = Pkce::generate().unwrap();
        assert_eq!(a.verifier.len(), 43);
        assert!(a
            .verifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert_eq!(a.challenge, challenge_of(&a.verifier));
        assert_ne!((a.verifier, a.state), (b.verifier, b.state));
    }

    #[test]
    fn the_consent_url_has_the_pkce_the_scope_and_the_loopback_redirect() {
        let client = Client {
            client_id: "id.apps".into(),
            client_secret: "SECRET".into(),
            auth_uri: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_uri: "https://oauth2.googleapis.com/token".into(),
        };
        let p = Pkce {
            verifier: "v".into(),
            challenge: "C".into(),
            state: "S".into(),
        };
        let url = consent_url(&client, "http://127.0.0.1:5000", super::super::token::SCOPE, &p);
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=id.apps&"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5000&"));
        assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fyoutube.force-ssl&"));
        assert!(url.contains("code_challenge=C&code_challenge_method=S256&state=S&"));
        // 秘密は URL に入れない
        assert!(!url.contains("SECRET"));
    }
}
