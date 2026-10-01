//! 過去の情報の返却。`GET /api/archive?from=<ms>&to=<ms>` で、jsonl の sink が書いた記録から
//! `received_at_ms` がその範囲にある情報を時刻順に返す (履歴の再生に使う)。
//! 読むのは設定された jsonl だけで、利用者の入力はファイルのパスに使わない。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::quake::Event;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Semaphore;

/// 範囲の上限 (ミリ秒)
const MAX_RANGE_MS: u64 = 3_600_000;
/// 返す件数の上限
pub const MAX_EVENTS: usize = 500;
/// 同時に読む数。毎回全部読むので、大量に開かれても e2 (メモリ 1GB) を圧迫しないように
const MAX_READERS: usize = 2;

#[derive(Clone)]
struct Archive {
    path: PathBuf,
    readers: Arc<Semaphore>,
}

#[derive(Deserialize)]
struct Range {
    from: u64,
    to: u64,
}

/// 設定の sink のうち、有効な最初の jsonl のパス。無ければ None (この API は出さない)
pub fn jsonl_path(sinks: &[toml::Table]) -> Option<PathBuf> {
    sinks
        .iter()
        .filter(|t| t.get("type").and_then(|v| v.as_str()) == Some("jsonl"))
        .filter(|t| t.get("enabled").and_then(|v| v.as_bool()) != Some(false))
        .find_map(|t| t.get("path")?.as_str().map(PathBuf::from))
}

pub fn router(path: PathBuf) -> Router {
    let state = Archive {
        path,
        readers: Arc::new(Semaphore::new(MAX_READERS)),
    };
    Router::new().route("/api/archive", get(handler).with_state(state))
}

async fn handler(State(a): State<Archive>, Query(r): Query<Range>) -> Response {
    if r.to < r.from || r.to - r.from > MAX_RANGE_MS {
        return (StatusCode::BAD_REQUEST, "range must be 0..=1 hour").into_response();
    }
    let Ok(_permit) = a.readers.try_acquire() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match read_range(&a.path, r.from, r.to).await {
        Ok(events) => Json(events).into_response(),
        Err(e) => {
            tracing::warn!("archive read failed: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// jsonl を 1 行ずつ読み、範囲に入る情報を時刻順に返す (最大 MAX_EVENTS 件)。壊れた行は飛ばす。
/// ponytail: 毎回ファイルを頭から全部読む。数十 MB を超えたら、索引や日付ごとのファイルを考える
pub async fn read_range(path: &Path, from: u64, to: u64) -> anyhow::Result<Vec<Event>> {
    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        // まだ 1 件も書かれていない
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut lines = BufReader::new(file).lines();
    let mut out = Vec::new();
    let mut broken = 0usize;
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(ev) if (from..=to).contains(&ev.received_at_ms) => out.push(ev),
            Ok(_) => {}
            Err(_) => broken += 1,
        }
    }
    if broken > 0 {
        tracing::warn!(path = %path.display(), broken, "archive: skipped broken lines");
    }
    out.sort_by_key(|e| e.received_at_ms);
    out.truncate(MAX_EVENTS);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    fn line(id: &str, recv: u64) -> String {
        format!(
            r#"{{"id":"{id}","source":"wolfx","received_at_ms":{recv},"kind":"eew_detection","detection_type":"Full"}}"#
        )
    }

    async fn get(app: Router, uri: &str) -> (StatusCode, String) {
        let res = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        (
            status,
            String::from_utf8(to_bytes(res.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap(),
        )
    }

    fn app_with(lines: &[String]) -> (tempfile::TempDir, Router) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("events.jsonl");
        std::fs::write(&p, lines.join("\n") + "\n").unwrap();
        (dir, router(p))
    }

    fn ids(body: &str) -> Vec<String> {
        let v: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
        v.iter().map(|e| e["id"].as_str().unwrap().to_string()).collect()
    }

    #[tokio::test]
    async fn filters_by_range_and_sorts() {
        let (_d, app) = app_with(&[line("c", 300), line("a", 100), line("b", 200), line("d", 900)]);
        let (status, body) = get(app, "/api/archive?from=100&to=300").await;
        assert_eq!(status, 200);
        assert_eq!(ids(&body), ["a", "b", "c"]);
    }

    #[tokio::test]
    async fn skips_broken_lines() {
        let (_d, app) = app_with(&[line("a", 100), "{broken".into(), r#"{"id":"x"}"#.into(), line("b", 200)]);
        let (status, body) = get(app, "/api/archive?from=0&to=1000").await;
        assert_eq!(status, 200);
        assert_eq!(ids(&body), ["a", "b"]);
    }

    #[tokio::test]
    async fn rejects_bad_ranges() {
        let (_d, app) = app_with(&[line("a", 100)]);
        for q in ["from=0&to=3600001", "from=5&to=1", "from=x&to=1", "from=1", ""] {
            let (status, _) = get(app.clone(), &format!("/api/archive?{q}")).await;
            assert_eq!(status, 400, "{q}");
        }
        assert_eq!(get(app, "/api/archive?from=0&to=3600000").await.0, 200);
    }

    #[tokio::test]
    async fn caps_the_number_of_events() {
        let lines: Vec<String> = (0..MAX_EVENTS as u64 + 10).map(|i| line(&format!("i{i}"), i)).collect();
        let (_d, app) = app_with(&lines);
        let (_, body) = get(app, "/api/archive?from=0&to=1000").await;
        assert_eq!(ids(&body).len(), MAX_EVENTS);
    }

    #[tokio::test]
    async fn missing_file_is_empty() {
        let app = router("/nonexistent/events.jsonl".into());
        assert_eq!(
            get(app, "/api/archive?from=0&to=1").await,
            (StatusCode::OK, "[]".into())
        );
    }

    #[test]
    fn route_exists_only_with_an_enabled_jsonl_sink() {
        let sinks = |s: &str| -> Vec<toml::Table> {
            toml::from_str::<toml::Table>(s).unwrap()["sinks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_table().unwrap().clone())
                .collect()
        };
        assert_eq!(jsonl_path(&sinks(r#"sinks = [{type = "rss", path = "x"}]"#)), None);
        assert_eq!(
            jsonl_path(&sinks(r#"sinks = [{type = "jsonl", path = "x", enabled = false}]"#)),
            None
        );
        assert_eq!(
            jsonl_path(&sinks(
                r#"sinks = [{type = "rss"}, {type = "jsonl", path = "d/e.jsonl"}]"#
            )),
            Some("d/e.jsonl".into())
        );
        assert_eq!(jsonl_path(&[]), None);
    }
}
