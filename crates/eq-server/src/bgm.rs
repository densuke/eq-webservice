//! 平時の BGM。設定したディレクトリの音声ファイルをファイル名順に一覧し (`GET /api/bgm`)、`/bgm/<ファイル名>` で配信する。
//! 一覧は毎回ディレクトリを読み直すので、ファイルはサーバを止めずにいつでも差し替えられる。

use std::path::{Path, PathBuf};

use axum::routing::get;
use axum::{Json, Router};
use lofty::file::TaggedFileExt;
use lofty::tag::Accessor;
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BgmConfig {
    /// 音声ファイルを置くディレクトリ。空なら BGM を使わない
    pub dir: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Track {
    /// ディレクトリの中のファイル名 (`/bgm/<file>` で取れる)
    pub file: String,
    /// 曲の情報のタイトル。無ければファイル名 (拡張子なし)
    pub title: String,
    pub artist: Option<String>,
}

/// ブラウザで再生できる音声の拡張子
const AUDIO: [&str; 9] = ["mp3", "m4a", "aac", "ogg", "oga", "opus", "wav", "flac", "webm"];

/// ディレクトリの音声ファイルをファイル名順に。読めなければ空
pub fn list(dir: &Path) -> Vec<Track> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .filter(|p| !p.file_name().and_then(|n| n.to_str()).unwrap_or(".").starts_with('.'))
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| AUDIO.contains(&x.to_ascii_lowercase().as_str()))
        })
        .collect();
    files.sort();
    files.iter().filter_map(|p| track(p)).collect()
}

fn track(path: &Path) -> Option<Track> {
    let file = path.file_name()?.to_str()?.to_string();
    let stem = path.file_stem()?.to_string_lossy().to_string();
    // 曲の情報が読めないファイルもファイル名で流す
    let tag = lofty::read_from_path(path).ok();
    let tag = tag.as_ref().and_then(|t| t.primary_tag().or_else(|| t.first_tag()));
    let title = tag
        .and_then(|t| t.title())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let artist = tag
        .and_then(|t| t.artist())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some(Track {
        file,
        title: title.unwrap_or(stem),
        artist,
    })
}

/// `GET /api/bgm` (一覧) と `/bgm/<ファイル名>` (配信)。ディレクトリが空なら一覧は常に空
pub fn router(cfg: &BgmConfig) -> Router {
    let dir = PathBuf::from(&cfg.dir);
    let enabled = !cfg.dir.is_empty();
    let listing = dir.clone();
    let r = Router::new().route(
        "/api/bgm",
        get(move || {
            let dir = listing.clone();
            async move {
                let tracks = if enabled {
                    tokio::task::spawn_blocking(move || list(&dir))
                        .await
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                Json(tracks)
            }
        }),
    );
    // ServeDir はディレクトリの外 (../ など) を返さない
    if enabled {
        r.nest_service("/bgm", ServeDir::new(dir))
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_audio_files_by_name_and_falls_back_to_the_file_name_for_titles() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b-second.mp3", "a-first.OGG", "notes.txt", ".hidden.mp3"] {
            std::fs::write(dir.path().join(name), b"not really audio").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub.mp3")).unwrap();
        let tracks = list(dir.path());
        assert_eq!(
            tracks,
            vec![
                Track {
                    file: "a-first.OGG".into(),
                    title: "a-first".into(),
                    artist: None
                },
                Track {
                    file: "b-second.mp3".into(),
                    title: "b-second".into(),
                    artist: None
                },
            ]
        );
    }

    #[test]
    fn missing_directory_is_empty() {
        assert!(list(Path::new("/nonexistent/eq-webservice-bgm")).is_empty());
    }
}
