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

/// 記録のファイルから読む件数の上限 (/api/archive は archive::MAX_EVENTS 件で切る。数時間の連続地震を丸ごと読むため、ファイルは広げる)
pub const FILE_MAX_EVENTS: usize = 50_000;

/// 範囲の報を時刻順に返す
pub async fn load(o: &Options) -> anyhow::Result<Vec<Event>> {
    let (events, max) = match &o.source {
        Source::Events(path) => (from_file(path, o.from, o.to).await?, FILE_MAX_EVENTS),
        Source::Archive(base) => (from_archive(base, o.from, o.to).await?, archive::MAX_EVENTS),
    };
    if events.len() >= max {
        tracing::warn!("報が {max} 件の上限に達しました。範囲の終わりの報が欠けているかもしれません");
    }
    Ok(events)
}

pub async fn from_archive(base: &str, from: u64, to: u64) -> anyhow::Result<Vec<Event>> {
    anyhow::ensure!(
        to - from <= ARCHIVE_MAX_RANGE_MS,
        "--archive は 1 時間までの範囲しか取れません"
    );
    let client = crate::net::client(Duration::from_secs(60))?;
    crate::net::json(client.get(format!("{base}/api/archive?from={from}&to={to}")))
        .await
        .context("fetching /api/archive")
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
