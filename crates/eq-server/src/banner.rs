//! 平時のバナー (案内・お知らせ)。設定したディレクトリの画像とテキストを、ファイル名 (拡張子を除く) ごとに 1 枚にまとめて
//! 名前順に一覧し (`GET /api/banners`)、画像は `/banner/<ファイル名>` で配信する。一覧は毎回読み直すので、いつでも差し替えられる。
//!
//! - 画像: png / jpg / jpeg / webp / gif (SVG はスクリプトを含められるので扱わない)
//! - テキスト (.txt): 本文。`http://` か `https://` で始まる行はリンク先 (最初の 1 つ) として扱い、本文には含めない
//! - 同じ名前の画像とテキストは、画像に文字を添えた 1 枚になる

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BannerConfig {
    /// 画像とテキストを置くディレクトリ。空ならバナーを出さない
    pub dir: String,
    /// 切り替える間隔 (秒)
    pub interval_sec: u64,
}

impl Default for BannerConfig {
    fn default() -> Self {
        BannerConfig {
            dir: String::new(),
            interval_sec: 20,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Banner {
    /// 画像の URL (`banner/<ファイル名>`)
    pub image: Option<String>,
    pub text: Option<String>,
    /// 押したときに開く先 (http / https だけ)
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Banners {
    pub interval_sec: u64,
    pub items: Vec<Banner>,
}

const IMAGES: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];
/// テキストの上限 (バナーに出せる程度)
const MAX_TEXT: usize = 4 * 1024;

/// ディレクトリの画像とテキストを、名前 (拡張子を除く) ごとに 1 枚にして名前順に
pub fn list(dir: &Path) -> Vec<Banner> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut by_stem: BTreeMap<String, Banner> = BTreeMap::new();
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    for path in files {
        let (Some(name), Some(stem), Some(ext)) = (
            path.file_name().and_then(|n| n.to_str()),
            path.file_stem().and_then(|n| n.to_str()),
            path.extension()
                .and_then(|n| n.to_str())
                .map(|x| x.to_ascii_lowercase()),
        ) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if IMAGES.contains(&ext.as_str()) {
            let b = by_stem.entry(stem.to_string()).or_default();
            b.image.get_or_insert_with(|| format!("banner/{}", encode(name)));
        } else if ext == "txt" {
            // 上限を超える部分は読まない (本文は MAX_TEXT までしか使わない)
            let mut raw = Vec::new();
            let read = std::fs::File::open(&path).and_then(|f| f.take(MAX_TEXT as u64 * 4).read_to_end(&mut raw));
            if read.is_err() {
                continue;
            }
            let (text, link) = parse_text(&String::from_utf8_lossy(&raw));
            let b = by_stem.entry(stem.to_string()).or_default();
            b.text = text;
            b.link = link;
        }
    }
    by_stem
        .into_values()
        .filter(|b| b.image.is_some() || b.text.is_some())
        .collect()
}

/// 本文とリンク先に分ける。http(s) で始まる行はリンク先 (最初の 1 つ)
fn parse_text(raw: &str) -> (Option<String>, Option<String>) {
    let mut link = None;
    let mut lines = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with("https://") || t.starts_with("http://") {
            if link.is_none() && !t.contains(char::is_whitespace) {
                link = Some(t.to_string());
            }
        } else {
            lines.push(line.trim_end());
        }
    }
    let text: String = lines.join("\n").trim().chars().take(MAX_TEXT).collect();
    ((!text.is_empty()).then_some(text), link)
}

/// URL のパスやクエリに使えるようにする (空白や日本語があっても取れるように)
pub fn encode(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `GET /api/banners` (一覧) と `/banner/<ファイル名>` (画像)。ディレクトリが空なら一覧は常に空
pub fn router(cfg: &BannerConfig) -> Router {
    let dir = PathBuf::from(&cfg.dir);
    let enabled = !cfg.dir.is_empty();
    let interval_sec = cfg.interval_sec.max(5);
    let listing = dir.clone();
    let r = Router::new().route(
        "/api/banners",
        get(move || {
            let dir = listing.clone();
            async move {
                let items = if enabled {
                    tokio::task::spawn_blocking(move || list(&dir))
                        .await
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                Json(Banners { interval_sec, items })
            }
        }),
    );
    // 決めた種類のファイルだけを配信する (ディレクトリの外も返さない)
    if enabled {
        r.merge(serve_images(dir))
    } else {
        r
    }
}

/// `/banner/<ファイル名>` で決めた種類 (IMAGES) の画像だけを配信する。
/// ディレクトリの外 (../ など) は ServeDir が返さない。SVG・HTML などを置いても配信しない
fn serve_images(dir: PathBuf) -> Router {
    Router::new()
        .nest_service("/banner", ServeDir::new(dir))
        .layer(middleware::from_fn(|req: Request, next: Next| async move {
            let allowed = Path::new(req.uri().path())
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| IMAGES.contains(&x.to_ascii_lowercase().as_str()));
            if allowed {
                next.run(req).await
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_images_and_texts_by_name_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let w = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
        w(
            "20-notice.txt",
            "保守のお知らせ\n10/1 3:00〜3:10 に停止します\nhttps://example.com/info\n",
        );
        w("10-ad.png", "png");
        w("10-ad.txt", "スポンサー\n");
        w("30 画像.webp", "webp");
        w("40-script.svg", "<svg/>");
        w(".hidden.txt", "x");
        w("50-empty.txt", "  \n");
        let got = list(dir.path());
        assert_eq!(
            got,
            vec![
                Banner {
                    image: Some("banner/10-ad.png".into()),
                    text: Some("スポンサー".into()),
                    link: None
                },
                Banner {
                    image: None,
                    text: Some("保守のお知らせ\n10/1 3:00〜3:10 に停止します".into()),
                    link: Some("https://example.com/info".into())
                },
                Banner {
                    image: Some("banner/30%20%E7%94%BB%E5%83%8F.webp".into()),
                    text: None,
                    link: None
                },
            ]
        );
    }

    #[tokio::test]
    async fn serves_only_image_files_inside_the_directory() {
        use axum::body::Body;
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), b"png").unwrap();
        std::fs::write(dir.path().join("b.svg"), b"<svg/>").unwrap();
        let app = serve_images(dir.path().to_path_buf());
        let status = |path: &'static str| {
            let app = app.clone();
            async move {
                app.oneshot(Request::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status()
            }
        };
        assert_eq!(status("/banner/a.png").await, StatusCode::OK);
        assert_eq!(status("/banner/b.svg").await, StatusCode::NOT_FOUND);
        assert_eq!(status("/banner/../Cargo.toml").await, StatusCode::NOT_FOUND);
    }

    #[test]
    fn only_http_links_are_taken() {
        assert_eq!(
            parse_text("見出し\njavascript:alert(1)\n"),
            (Some("見出し\njavascript:alert(1)".into()), None)
        );
        assert_eq!(
            parse_text("https://a.example/x\nhttps://b.example/y"),
            (None, Some("https://a.example/x".into()))
        );
    }
}
