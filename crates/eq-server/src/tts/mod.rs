//! 音声アナウンス (Google Cloud Text-to-Speech)。仕様は docs/tts.md。
//! 文を部品に分け、部品ごとにディスクへキャッシュする (無料枠に収めるため)。

pub mod budget;
pub mod cache;
pub mod google;
pub mod http;
pub mod limits;
pub mod phrase;
pub mod prewarm;
pub mod priors;
pub mod wav;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// `config.toml` の `[tts]`
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct TtsConfig {
    pub enabled: bool,
    pub voice: String,
    pub cache_dir: PathBuf,
    pub monthly_char_limit: usize,
    pub prewarm: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        TtsConfig {
            enabled: false,
            voice: "ja-JP-Neural2-B".into(),
            cache_dir: "data/tts".into(),
            monthly_char_limit: 900_000,
            prewarm: true,
        }
    }
}
