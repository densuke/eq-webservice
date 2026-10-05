use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub source: SourceConfig,
    /// 配信先プラグイン。`type` でプラグインを選び、残りのキーはプラグイン固有。
    #[serde(default)]
    pub sinks: Vec<toml::Table>,
    #[serde(default)]
    pub telop: TelopConfig,
    /// 気象警報・注意報 (平時の地図に出す)
    #[serde(default)]
    pub weather: crate::weather::WeatherConfig,
    /// 平時の BGM
    #[serde(default)]
    pub bgm: crate::bgm::BgmConfig,
    /// 平時のバナー (案内・お知らせ)
    #[serde(default)]
    pub banner: crate::banner::BannerConfig,
    /// 画面の並びの定義ファイル
    #[serde(default)]
    pub layout: crate::layout::LayoutConfig,
    /// 音声アナウンス (Google TTS)
    #[serde(default)]
    pub tts: crate::tts::TtsConfig,
}

/// 平常時に画面上部で切り替えて表示する文
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TelopConfig {
    /// 出典と注意書きのあとに足す文
    pub messages: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ServerConfig {
    pub listen: String,
    /// フロントエンドのビルド成果物 (web/dist)。空ならページは配信しない。
    pub static_dir: PathBuf,
    /// ブラウザ接続時にまとめて送る直近イベント数
    pub recent_capacity: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            listen: "127.0.0.1:8080".into(),
            static_dir: "web/dist".into(),
            recent_capacity: 200,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceConfig {
    /// P2P地震情報 WebSocket API
    P2pquake {
        #[serde(default = "default_p2pquake_ws")]
        url: String,
        /// 起動時に直近の履歴を取得する API。空文字で無効。
        #[serde(default = "default_p2pquake_history")]
        history_url: String,
        #[serde(default = "default_history_limit")]
        history_limit: u32,
        /// 現在の津波予報を取得する API (履歴 API には津波予報が含まれないため別に取る)。
        /// 起動時と再接続時に読む。空文字で無効。
        #[serde(default = "default_p2pquake_tsunami")]
        tsunami_url: String,
        /// 緊急地震速報 (予報を含む) の WebSocket。P2P地震情報は警報しか配信しないため別に受ける。
        /// 例: "wss://ws-api.wolfx.jp/jma_eew" (Wolfx Open API)。空文字で無効 (既定)
        #[serde(default)]
        eew_url: String,
    },
    /// 記録済みの JSON Lines を再生する (開発・デモ用)
    Replay {
        path: PathBuf,
        /// 記録時の間隔を何倍速で再生するか
        #[serde(default = "default_speed")]
        speed: f64,
        /// 記録間隔がこれより長い場合は詰める (ミリ秒)
        #[serde(default = "default_max_gap_ms")]
        max_gap_ms: u64,
        /// 時刻を「今」にずらして再生する (P波・S波の描画確認用)
        #[serde(default = "default_true")]
        rebase_time: bool,
        #[serde(default)]
        r#loop: bool,
    },
}

impl SourceConfig {
    /// `GET /api/source` で返す種類
    pub fn kind(&self) -> &'static str {
        match self {
            SourceConfig::P2pquake { .. } => "p2pquake",
            SourceConfig::Replay { .. } => "replay",
        }
    }
}

impl Default for SourceConfig {
    fn default() -> Self {
        SourceConfig::P2pquake {
            url: default_p2pquake_ws(),
            history_url: default_p2pquake_history(),
            history_limit: default_history_limit(),
            tsunami_url: default_p2pquake_tsunami(),
            eew_url: String::new(),
        }
    }
}

fn default_p2pquake_ws() -> String {
    "wss://api.p2pquake.net/v2/ws".into()
}
fn default_p2pquake_history() -> String {
    "https://api.p2pquake.net/v2/history".into()
}
fn default_p2pquake_tsunami() -> String {
    "https://api.p2pquake.net/v2/jma/tsunami".into()
}
fn default_history_limit() -> u32 {
    30
}
fn default_speed() -> f64 {
    1.0
}
fn default_max_gap_ms() -> u64 {
    10_000
}
fn default_true() -> bool {
    true
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn parse(text: &str) -> anyhow::Result<Config> {
        Ok(toml::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let cfg = Config::parse(include_str!("../../../config.example.toml")).unwrap();
        assert!(matches!(cfg.source, SourceConfig::P2pquake { .. }));
        assert!(!cfg.sinks.is_empty());
    }

    #[test]
    fn telop_messages_are_read() {
        let cfg = Config::parse("[telop]\nmessages = [\"a\"]\n[server]\nlisten = \"x\"").unwrap();
        assert_eq!(cfg.telop.messages, ["a"]);
    }

    #[test]
    fn empty_config_uses_defaults() {
        let cfg = Config::parse("").unwrap();
        assert_eq!(cfg.server.listen, "127.0.0.1:8080");
        assert!(cfg.sinks.is_empty());
    }

    #[test]
    fn tts_defaults_when_absent() {
        let cfg = Config::parse("").unwrap();
        assert_eq!(cfg.tts, crate::tts::TtsConfig::default());
    }

    #[test]
    fn tts_section_overrides_and_keeps_other_defaults() {
        let cfg = Config::parse("[tts]\nenabled = true\nvoice = \"ja-JP-Neural2-C\"").unwrap();
        assert!(cfg.tts.enabled);
        assert_eq!(cfg.tts.voice, "ja-JP-Neural2-C");
        assert_eq!(cfg.tts.monthly_char_limit, 900_000);
        assert!(cfg.tts.prewarm);
    }

    #[test]
    fn tts_unknown_key_is_error() {
        assert!(Config::parse("[tts]\nbogus = 1").is_err());
    }
}
