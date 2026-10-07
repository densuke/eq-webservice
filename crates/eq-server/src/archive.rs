//! 過去の情報の返却。`GET /api/archive?from=<ms>&to=<ms>` で、jsonl の sink が書いた記録から
//! `received_at_ms` がその範囲にある情報を時刻順に返す (履歴の再生に使う)。
//! 読むのは設定された jsonl だけで、利用者の入力はファイルのパスに使わない。

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::archive_store;
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
/// 1 回の走査の上限。超えたら打ち切る (HTTP は 503。web は null として扱い、ブラウザが持つ分で代用する)
#[derive(Clone, Copy)]
struct Limits {
    bytes: u64,
    time: Duration,
}

const HTTP_LIMITS: Limits = Limits {
    bytes: 64 << 20,
    time: Duration::from_secs(5),
};

/// 走査量か時間の上限を超えた
#[derive(Debug)]
struct OverBudget;

impl std::fmt::Display for OverBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("archive scan exceeded its budget")
    }
}
impl std::error::Error for OverBudget {}

#[derive(Clone)]
struct Archive {
    path: PathBuf,
    readers: Arc<Semaphore>,
    limits: Limits,
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
    router_with(path, HTTP_LIMITS)
}

fn router_with(path: PathBuf, limits: Limits) -> Router {
    let state = Archive {
        path,
        readers: Arc::new(Semaphore::new(MAX_READERS)),
        limits,
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
    match scan(&a.path, r.from, r.to, MAX_EVENTS, Some(a.limits)).await {
        Ok(events) => Json(events).into_response(),
        Err(e) if e.is::<OverBudget>() => {
            tracing::warn!("archive: {e}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
        Err(e) => {
            tracing::warn!("archive read failed: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// 時刻 (同じなら読んだ順) で比べる。BinaryHeap に入れて、残す中でいちばん新しいものをすぐ捨てるため
struct Held {
    at: u64,
    seq: usize,
    ev: Event,
}
impl PartialEq for Held {
    fn eq(&self, o: &Self) -> bool {
        (self.at, self.seq) == (o.at, o.seq)
    }
}
impl Eq for Held {}
impl PartialOrd for Held {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Held {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.at, self.seq).cmp(&(o.at, o.seq))
    }
}

/// jsonl (旧ファイル + 日付ファイル。path は設定の jsonl のもの) を 1 行ずつ読み、範囲に入る情報を時刻順に返す (最大 max 件)。壊れた行は飛ばす。
/// HTTP の応答は MAX_EVENTS で絞るが、動画を作る係は 1 つの範囲を丸ごと読む。走査の上限は付けない
pub async fn read_range_upto(path: &Path, from: u64, to: u64, max: usize) -> anyhow::Result<Vec<Event>> {
    scan(path, from, to, max, None).await
}

async fn scan(path: &Path, from: u64, to: u64, max: usize, limits: Option<Limits>) -> anyhow::Result<Vec<Event>> {
    let run = scan_inner(path, from, to, max, limits.map(|l| l.bytes)); // 上限がある = HTTP
    match limits {
        None => run.await,
        Some(l) => tokio::time::timeout(l.time, run).await.map_err(|_| OverBudget)?,
    }
}

/// 残すのは範囲内でいちばん古い max 件 (並べ替えて切り詰めるのと同じ結果)。保持は max 件までで済む。
/// 読むのは archive_store が選んだファイルだけ。走査量の上限は、その合計 (旧ファイル込み) に掛ける。
/// 時刻が戻る行も範囲内なら返す (早期打ち切りはしない)
async fn scan_inner(
    base: &Path,
    from: u64,
    to: u64,
    max: usize,
    byte_limit: Option<u64>,
) -> anyhow::Result<Vec<Event>> {
    let mut heap: BinaryHeap<Held> = BinaryHeap::new();
    let (mut seq, mut read) = (0usize, 0u64);
    for path in archive_store::files_in_range(base, from, to)? {
        let file = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                // 1 つ読めなくても、他の日の問い合わせまで巻き込まない
                tracing::warn!(path = %path.display(), "archive: cannot open, skipped: {e}");
                continue;
            }
        };
        let mut broken = 0usize;
        let mut lines = BufReader::new(file).lines();
        while let Some(line) = lines.next_line().await? {
            read += line.len() as u64 + 1;
            if byte_limit.is_some_and(|b| read > b) {
                return Err(OverBudget.into());
            }
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Event>(&line) {
                Ok(ev) if (from..=to).contains(&ev.received_at_ms) => {
                    seq += 1;
                    heap.push(Held {
                        at: ev.received_at_ms,
                        seq,
                        ev,
                    });
                    if heap.len() > max {
                        heap.pop();
                    }
                }
                Ok(_) => {}
                Err(_) => broken += 1,
            }
        }
        if broken > 0 {
            tracing::warn!(path = %path.display(), broken, "archive: skipped broken lines");
        }
    }
    Ok(heap.into_sorted_vec().into_iter().map(|h| h.ev).collect())
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
    async fn keeps_the_oldest_events_even_when_the_file_is_newest_first() {
        let lines: Vec<String> = (0..MAX_EVENTS as u64 + 50)
            .rev()
            .map(|i| line(&format!("i{i}"), i))
            .collect();
        let (_d, app) = app_with(&lines);
        let got = ids(&get(app, "/api/archive?from=0&to=1000").await.1);
        assert_eq!(got.len(), MAX_EVENTS);
        assert_eq!(got.first().unwrap(), "i0");
        assert_eq!(got.last().unwrap(), &format!("i{}", MAX_EVENTS - 1));
    }

    #[tokio::test]
    async fn returns_in_range_lines_even_when_time_goes_backwards() {
        // 受信時刻が戻る行 (p2pquake 再接続時の津波予報は発表時刻が received_at_ms) も、範囲内なら返す
        let (_d, app) = app_with(&[line("a", 100), line("far", 60_201), line("late", 150)]);
        assert_eq!(ids(&get(app, "/api/archive?from=0&to=200").await.1), ["a", "late"]);
    }

    #[tokio::test]
    async fn gives_503_when_the_scan_budget_is_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("events.jsonl");
        std::fs::write(&p, [line("a", 1), line("b", 2)].join("\n") + "\n").unwrap();
        let app = router_with(
            p,
            Limits {
                bytes: 10,
                time: Duration::from_secs(5),
            },
        );
        assert_eq!(
            get(app, "/api/archive?from=0&to=10").await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn missing_file_is_empty() {
        let app = router("/nonexistent/events.jsonl".into());
        assert_eq!(
            get(app, "/api/archive?from=0&to=1").await,
            (StatusCode::OK, "[]".into())
        );
    }

    const DAY: u64 = 86_400_000;
    /// 2024-01-01T00:00:00Z
    const D0: u64 = 19_723 * DAY;

    fn dir_with(files: &[(&str, Vec<String>)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, lines) in files {
            std::fs::write(dir.path().join(name), lines.join("\n") + "\n").unwrap();
        }
        dir
    }

    fn bulk(prefix: &str, n: u64, at: u64) -> Vec<String> {
        (0..n).map(|i| line(&format!("{prefix}{i}"), at + i)).collect()
    }

    fn app_in(dir: &tempfile::TempDir, bytes: u64) -> Router {
        router_with(
            dir.path().join("events.jsonl"),
            Limits {
                bytes,
                time: Duration::from_secs(5),
            },
        )
    }

    #[tokio::test]
    async fn reads_only_the_files_of_the_range_so_a_big_total_does_not_matter() {
        // 古い日付側 (day1、day4) が大きく、合計は上限を超える。最近 (day9) も古い (day1) も、その日のファイルだけで足りる
        let dir = dir_with(&[
            ("events-2024-01-01.jsonl", bulk("o", 20, D0 + 1000)),
            ("events-2024-01-04.jsonl", bulk("m", 20, D0 + 3 * DAY)),
            ("events-2024-01-09.jsonl", vec![line("new", D0 + 8 * DAY + 5)]),
        ]);
        let app = app_in(&dir, 3000);
        let q = |from: u64| format!("/api/archive?from={from}&to={}", from + 60_000);
        assert_eq!(ids(&get(app.clone(), &q(D0 + 8 * DAY)).await.1), ["new"]);
        assert_eq!(get(app, &q(D0 + 1000)).await.0, 200);
    }

    #[tokio::test]
    async fn gives_503_when_the_files_of_one_range_exceed_the_budget_together() {
        // 旧ファイルも数える。どれか 1 つなら上限未満でも、合計が超えたら不完全な 200 にしない
        let dir = dir_with(&[
            ("events.jsonl", bulk("l", 20, D0)),
            ("events-2024-01-01.jsonl", bulk("d", 20, D0 + 100)),
        ]);
        let app = app_in(&dir, 3000);
        assert_eq!(
            get(app, &format!("/api/archive?from={D0}&to={}", D0 + 1000)).await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn returns_a_late_written_line_from_the_next_days_file() {
        // 前日の時刻の行 (津波の取り込み) が、翌日のファイルに書かれた。時刻も戻っている
        let dir = dir_with(&[
            (
                "events-2024-01-01.jsonl",
                vec![line("a", D0 + 100), line("far", D0 + 60_201)],
            ),
            ("events-2024-01-02.jsonl", vec![line("late", D0 + 150)]),
        ]);
        let app = app_in(&dir, 1 << 20);
        let (st, body) = get(app, &format!("/api/archive?from={D0}&to={}", D0 + 200)).await;
        assert_eq!((st, ids(&body)), (StatusCode::OK, vec!["a".to_string(), "late".into()]));
    }

    #[tokio::test]
    async fn range_across_utc_midnight_reads_both_days_and_the_legacy_file() {
        let dir = dir_with(&[
            ("events.jsonl", vec![line("old", D0 + DAY - 3)]),
            ("events-2024-01-01.jsonl", vec![line("before", D0 + DAY - 1)]),
            ("events-2024-01-02.jsonl", vec![line("after", D0 + DAY)]),
        ]);
        let app = app_in(&dir, 1 << 20);
        let (_, body) = get(app, &format!("/api/archive?from={}&to={}", D0 + DAY - 5, D0 + DAY + 5)).await;
        assert_eq!(ids(&body), ["old", "before", "after"]);
    }

    #[tokio::test]
    async fn read_range_upto_reads_the_daily_files_too() {
        let dir = dir_with(&[
            ("events.jsonl", vec![line("l", 10)]),
            ("events-1970-01-01.jsonl", vec![line("d", 20)]),
        ]);
        let got = read_range_upto(&dir.path().join("events.jsonl"), 0, 100, 10)
            .await
            .unwrap();
        assert_eq!(got.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["l", "d"]);
    }

    #[tokio::test]
    async fn a_directory_named_like_a_daily_file_does_not_break_the_scan() {
        let dir = dir_with(&[("events-2024-01-01.jsonl", vec![line("a", D0 + 1)])]);
        std::fs::create_dir(dir.path().join("events-2024-01-02.jsonl")).unwrap();
        let app = app_in(&dir, 1 << 20);
        let (st, body) = get(app, &format!("/api/archive?from={D0}&to={}", D0 + 10)).await;
        assert_eq!((st, ids(&body)), (StatusCode::OK, vec!["a".to_string()]));
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
