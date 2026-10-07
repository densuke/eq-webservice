//! 動画にする報を集める。eq-server の jsonl の記録 (sink が書いたもの) を直接読むか、/api/archive から取る。
//! samples/scenarios の形 (上流の JSON のまま。received_at_ms が無い) も読め、その場合は発表時刻を届いた時刻にする
//! (web の履歴の再生と同じ)。

use std::io::BufRead;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;

use super::args::{Options, Source};
use crate::archive;
use crate::quake::Event;
use crate::source::replay;

/// /api/archive が受ける範囲の上限 (archive.rs の MAX_RANGE_MS と同じ)
const ARCHIVE_MAX_RANGE_MS: u64 = 3_600_000;

/// 続けて取るときの、範囲と範囲の間の休み (eq.fuga.jp は小さな VM で、記録を毎回頭から読むため)
pub const ARCHIVE_PAUSE: Duration = Duration::from_secs(1);

/// 記録のファイルから読む件数の上限 (/api/archive は archive::MAX_EVENTS 件で切る。数時間の連続地震を丸ごと読むため、ファイルは広げる)
pub const FILE_MAX_EVENTS: usize = 50_000;

/// 範囲の報を時刻順に返す
pub async fn load(o: &Options) -> anyhow::Result<Vec<Event>> {
    match &o.source {
        Source::Events(path) => {
            let events = from_file(path, o.from, o.to).await?;
            if events.len() >= FILE_MAX_EVENTS {
                tracing::warn!(
                    "報が {FILE_MAX_EVENTS} 件の上限に達しました。範囲の終わりの報が欠けているかもしれません"
                );
            }
            Ok(events)
        }
        Source::Archive(base) => from_archive_range(base, o.from, o.to).await,
    }
}

/// 1 回に取れる範囲 (1 時間) ずつに分けた、[from, to] を隙間も重なりもなく覆う範囲 (両端を含む)。
/// 次の範囲の始まりは、前の終わりの 1 ミリ秒あと
pub fn archive_chunks(from: u64, to: u64) -> Vec<(u64, u64)> {
    let mut chunks = Vec::new();
    let mut start = from;
    while start <= to {
        let end = to.min(start + ARCHIVE_MAX_RANGE_MS);
        chunks.push((start, end));
        if end == u64::MAX {
            break;
        }
        start = end + 1;
    }
    chunks
}

/// 範囲の報を、同じ id を 1 つにして、届いた時刻の順に並べる
pub fn merge_events(events: Vec<Event>) -> Vec<Event> {
    let mut seen = std::collections::HashSet::new();
    let mut merged: Vec<Event> = events.into_iter().filter(|e| seen.insert(e.id.clone())).collect();
    merged.sort_by_key(|e| e.received_at_ms);
    merged
}

/// 1 時間ずつの範囲を順に取って 1 つにする。範囲と範囲の間に pause を空ける (取る先の負担を減らす)。
/// 1 つでも失敗したら全体が失敗 (取れた分は捨てる。呼ぶ側は、全部取れたときだけ先へ進める)
pub async fn fetch_chunks<F, Fut>(from: u64, to: u64, pause: Duration, mut fetch: F) -> anyhow::Result<Vec<Event>>
where
    F: FnMut(u64, u64) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<Event>>>,
{
    let mut all = Vec::new();
    for (i, (a, b)) in archive_chunks(from, to).into_iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(pause).await;
        }
        all.extend(fetch(a, b).await?);
    }
    Ok(merge_events(all))
}

/// /api/archive から、1 時間を超える範囲も取る
pub async fn from_archive_range(base: &str, from: u64, to: u64) -> anyhow::Result<Vec<Event>> {
    fetch_chunks(from, to, ARCHIVE_PAUSE, |a, b| from_archive(base, a, b)).await
}

pub async fn from_archive(base: &str, from: u64, to: u64) -> anyhow::Result<Vec<Event>> {
    anyhow::ensure!(
        to - from <= ARCHIVE_MAX_RANGE_MS,
        "/api/archive は 1 時間までの範囲しか取れません"
    );
    let client = crate::net::client(Duration::from_secs(60))?;
    let events: Vec<Event> = crate::net::json(client.get(format!("{base}/api/archive?from={from}&to={to}")))
        .await
        .context("fetching /api/archive")?;
    if events.len() >= archive::MAX_EVENTS {
        tracing::warn!(
            "報が {} 件の上限に達しました。範囲の終わりの報が欠けているかもしれません",
            archive::MAX_EVENTS
        );
    }
    Ok(events)
}

pub async fn from_file(path: &Path, from: u64, to: u64) -> anyhow::Result<Vec<Event>> {
    if is_sink_format(path)? {
        return archive::read_range_upto(path, from, to, FILE_MAX_EVENTS).await;
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    from_raw(&text, from, to)
}

/// 最初の報の行に received_at_ms があれば、sink の jsonl
fn is_sink_format(path: &Path) -> anyhow::Result<bool> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    for line in std::io::BufReader::new(file).lines() {
        let line = line?;
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            return Ok(line.contains("\"received_at_ms\""));
        }
    }
    Ok(true)
}

/// 上流の JSON のままの記録を、発表時刻を届いた時刻にして、範囲で絞って時刻順に返す
fn from_raw(text: &str, from: u64, to: u64) -> anyhow::Result<Vec<Event>> {
    let mut events: Vec<Event> = replay::load(text)?
        .into_iter()
        .filter_map(|mut e| {
            e.received_at_ms = u64::try_from(e.issued_at_ms()?).ok()?;
            Some(e)
        })
        .filter(|e| (from..=to).contains(&e.received_at_ms))
        .collect();
    events.sort_by_key(|e| e.received_at_ms);
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = concat!(
        "# name: x\n",
        r#"{"type":"jma_eew","Title":"緊急地震速報（予報）","EventID":"E1","Serial":1,"AnnouncedTime":"2024/01/01 16:06:12","OriginTime":"2024/01/01 16:06:06","Hypocenter":"石川県能登地方","Latitude":37.4,"Longitude":137.4,"Magnitude":1.0,"Depth":10,"MaxIntensity":"不明","WarnArea":[],"isSea":false,"isTraining":false,"isAssumption":false,"isWarn":false,"isFinal":false,"isCancel":false}"#,
        "\n"
    );

    #[test]
    fn a_raw_record_gets_its_announced_time_as_the_received_time() {
        let t = crate::quake::jst::parse_ms("2024/01/01 16:06:12").unwrap() as u64;
        let events = from_raw(RAW, 0, u64::MAX).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].received_at_ms, t);
        assert!(from_raw(RAW, t + 1, u64::MAX).unwrap().is_empty());
        assert!(from_raw(RAW, 0, t - 1).unwrap().is_empty());
    }

    fn ev(id: &str, at: u64) -> Event {
        let mut e = from_raw(RAW, 0, u64::MAX).unwrap().remove(0);
        e.id = id.to_string();
        e.received_at_ms = at;
        e
    }
    #[tokio::test]
    async fn from_file_reads_to_the_end_even_when_the_file_is_not_in_time_order() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("events.jsonl");
        // 範囲 (0..=200) をずっと過ぎた行のあとに、範囲内の古い時刻の行が来る
        let lines: Vec<String> = [("a", 100), ("far", 10_000_000), ("late", 150)]
            .iter()
            .map(|(id, at)| serde_json::to_string(&ev(id, *at)).unwrap())
            .collect();
        std::fs::write(&p, lines.join("\n") + "\n").unwrap();
        let ids: Vec<String> = from_file(&p, 0, 200).await.unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["a", "late"]);
    }

    #[test]
    fn chunks_cover_the_range_hour_by_hour_without_gaps_or_overlaps() {
        const H: u64 = 3_600_000;
        assert_eq!(archive_chunks(10, 5), vec![]);
        assert_eq!(archive_chunks(5, 5), vec![(5, 5)]);
        assert_eq!(archive_chunks(0, H), vec![(0, H)]);
        assert_eq!(archive_chunks(0, H + 1), vec![(0, H), (H + 1, H + 1)]);
        for (from, to) in [(1_000, 3 * H + 17), (7, 168 * H + 3), (0, 2 * H + 1)] {
            let c = archive_chunks(from, to);
            assert_eq!((c[0].0, c[c.len() - 1].1), (from, to));
            assert!(c.iter().all(|&(a, b)| a <= b && b - a <= H), "{c:?}");
            assert!(c.windows(2).all(|w| w[1].0 == w[0].1 + 1), "{c:?}");
        }
    }

    #[test]
    fn merging_drops_repeated_ids_and_sorts_by_time() {
        let m = merge_events(vec![ev("b", 20), ev("a", 10), ev("b", 20), ev("c", 15)]);
        let ids: Vec<_> = m.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["a", "c", "b"]);
    }

    #[tokio::test]
    async fn chunks_are_fetched_in_order_and_one_failure_fails_the_whole() {
        const H: u64 = 3_600_000;
        let calls = std::sync::Mutex::new(Vec::new());
        let ok = fetch_chunks(0, 2 * H + 5, Duration::ZERO, |a, b| {
            calls.lock().unwrap().push((a, b));
            // 隣の範囲に同じ報が重なって返っても、1 つになる
            async move { Ok(vec![ev("shared", 1), ev(&format!("e{a}"), a)]) }
        })
        .await
        .unwrap();
        assert_eq!(calls.lock().unwrap().len(), 3);
        assert_eq!(ok.len(), 4);
        let n = std::sync::atomic::AtomicUsize::new(0);
        let failed = fetch_chunks(0, 5 * H, Duration::ZERO, |_, _| {
            let i = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if i == 2 {
                    anyhow::bail!("offline")
                } else {
                    Ok(vec![])
                }
            }
        })
        .await;
        assert!(failed.is_err());
        // 失敗した範囲で打ち切る (残りは取りにいかない)
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn the_sink_format_is_told_by_received_at_ms_in_the_first_report() {
        let dir = tempfile::tempdir().unwrap();
        let sink = dir.path().join("sink.jsonl");
        std::fs::write(&sink, "\n{\"id\":\"a\",\"received_at_ms\":5}\n").unwrap();
        let raw = dir.path().join("raw.jsonl");
        std::fs::write(&raw, RAW).unwrap();
        assert!(is_sink_format(&sink).unwrap());
        assert!(!is_sink_format(&raw).unwrap());
    }
}
