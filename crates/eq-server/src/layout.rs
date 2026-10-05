//! 画面の並びの定義 (`GET /api/layout`)。設定したファイル (JSON) を毎回読み直して返すので、
//! ファイルを書き換えてブラウザを再読み込みすれば並びが変わる (ビルドもサーバの再起動も要らない)。
//! 設定が無い・読めない・JSON でないときは、組み込みの定義 (web/src/layout.json) を返す。
//! 書式 (部品の名前など) は web が確かめ、正しくなければ web の組み込みの定義を使う。

use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

/// 組み込みの定義 (web の組み込みと同じファイル)
pub const BUILTIN: &str = include_str!("../../../web/src/layout.json");

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LayoutConfig {
    /// 定義ファイル (JSON)。空なら組み込みの定義
    pub file: String,
}

/// 返す定義。ファイルが読めない・JSON でなければ、警告を出して組み込みの定義
pub fn read(file: &str) -> String {
    if file.is_empty() {
        return BUILTIN.to_string();
    }
    match std::fs::read_to_string(file) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(_) => text,
            Err(e) => {
                tracing::warn!(file, "layout file is not valid JSON, using the built-in layout: {e}");
                BUILTIN.to_string()
            }
        },
        Err(e) => {
            tracing::warn!(file, "cannot read the layout file, using the built-in layout: {e}");
            BUILTIN.to_string()
        }
    }
}

pub fn router(cfg: &LayoutConfig) -> Router {
    let file = cfg.file.clone();
    Router::new().route(
        "/api/layout",
        get(move || {
            let file = file.clone();
            async move {
                let body = tokio::task::spawn_blocking(move || read(&file))
                    .await
                    .unwrap_or_else(|_| BUILTIN.to_string());
                (
                    [
                        (header::CONTENT_TYPE, "application/json"),
                        (header::CACHE_CONTROL, "no-store"),
                    ],
                    body,
                )
                    .into_response()
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_layout_is_json() {
        let v: serde_json::Value = serde_json::from_str(BUILTIN).unwrap();
        assert_eq!(v["version"], 1);
        assert!(v["layouts"].as_array().is_some_and(|l| !l.is_empty()));
    }

    #[test]
    fn reads_the_file_each_time_and_falls_back_to_the_built_in() {
        assert_eq!(read(""), BUILTIN);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        let file = path.to_str().unwrap();
        // 無い・JSON でない → 組み込み
        assert_eq!(read(file), BUILTIN);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(read(file), BUILTIN);
        // 書き換えると次から変わる
        std::fs::write(&path, r#"{"version":1,"layouts":[]}"#).unwrap();
        assert_eq!(read(file), r#"{"version":1,"layouts":[]}"#);
        std::fs::write(&path, r#"{"version":1,"layouts":[{"name":"x"}]}"#).unwrap();
        assert_eq!(read(file), r#"{"version":1,"layouts":[{"name":"x"}]}"#);
    }
}
