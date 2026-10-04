//! Google Cloud Text-to-Speech の呼び出し。仕様は docs/tts.md (S3)。

use std::time::Duration;

use anyhow::Context;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

use super::wav;
use crate::net;

const ENDPOINT: &str = "https://texttospeech.googleapis.com/v1/text:synthesize";

/// 文字列を PCM (mono 44.1kHz) にする。テストでは偽物に差し替える。
pub trait Synth: Send + Sync + 'static {
    fn synth(&self, text: &str, voice: &str) -> impl std::future::Future<Output = anyhow::Result<Vec<i16>>> + Send;
}

pub struct Google {
    client: reqwest::Client,
    key: String,
}

impl Google {
    pub fn new(key: String) -> anyhow::Result<Google> {
        Ok(Google {
            client: net::client(Duration::from_secs(10))?,
            key,
        })
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(rename = "audioContent")]
    audio_content: String,
}

impl Synth for Google {
    async fn synth(&self, text: &str, voice: &str) -> anyhow::Result<Vec<i16>> {
        // キーはヘッダで渡す (URL に入れない)
        let req = self
            .client
            .post(ENDPOINT)
            .header("X-Goog-Api-Key", &self.key)
            .json(&request_body(text, voice));
        let res: Response = net::json(req).await.context("Google TTS の呼び出しに失敗")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(res.audio_content)
            .context("audioContent の base64 が不正")?;
        wav::parse(&bytes).context("Google TTS の WAV を解釈できない")
    }
}

/// text:synthesize に送る JSON 本文。
pub fn request_body(text: &str, voice: &str) -> Value {
    json!({
        "input": {"text": text},
        "voice": {"languageCode": "ja-JP", "name": voice},
        "audioConfig": {"audioEncoding": "LINEAR16", "sampleRateHertz": wav::RATE},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn リクエスト本文はlinear16の44100hz() {
        assert_eq!(
            request_body("テスト", "ja-JP-Neural2-B"),
            json!({
                "input": {"text": "テスト"},
                "voice": {"languageCode": "ja-JP", "name": "ja-JP-Neural2-B"},
                "audioConfig": {"audioEncoding": "LINEAR16", "sampleRateHertz": 44100}
            })
        );
    }
}
