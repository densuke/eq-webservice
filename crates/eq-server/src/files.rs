//! 設定したディレクトリのファイルを、決めた種類 (拡張子) だけ配信する (BGM の音声、バナーの画像)。
//! ディレクトリの外 (../ など) は ServeDir が返さない。種類の違うファイル (SVG・HTML など) を置いても配信しない。

use std::path::PathBuf;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::Router;
use tower_http::services::ServeDir;

pub fn serve(prefix: &'static str, dir: PathBuf, exts: &'static [&'static str]) -> Router {
    Router::new()
        .nest_service(prefix, ServeDir::new(dir))
        .layer(middleware::from_fn(move |req: Request, next: Next| async move {
            let allowed = std::path::Path::new(req.uri().path())
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| exts.contains(&x.to_ascii_lowercase().as_str()));
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
    use axum::body::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn serves_only_the_listed_kinds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), b"png").unwrap();
        std::fs::write(dir.path().join("b.svg"), b"<svg/>").unwrap();
        let app = serve("/banner", dir.path().to_path_buf(), &["png"]);
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
}
