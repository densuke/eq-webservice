//! `eq-server replay-worker --scan <jsonl | URL>`: 検知とまとめだけを試して、結果を出す (キューには何も書かない)。
//! 設定の基準を決めるときと、e2 で「なぜ動画にならないか」を調べるときに使う。

use std::collections::HashSet;

use anyhow::Context;

use super::super::source;
use super::config::WorkerConfig;
use super::detect::{self, Rules};
use crate::quake::{jst, Event, Scale};

/// /api/archive が 1 回に返す範囲
const CHUNK_MS: u64 = 3_600_000;

/// 記録を読む。URL なら、今から hours 時間さかのぼって 1 時間ずつ取る。ファイルなら全部。
/// 返すもう 1 つは「今」の時刻: URL は実際の今。ファイルは、最後の報の静かな時間のあと (全部閉じたものとして見る)
async fn read(spec: &str, hours: u64, rules: &Rules) -> anyhow::Result<(Vec<Event>, u64)> {
    if spec.starts_with("http://") || spec.starts_with("https://") {
        let base = spec.trim_end_matches('/');
        let now = super::super::super::calm_state::now_ms();
        let mut seen = HashSet::new();
        let mut events: Vec<Event> = Vec::new();
        let mut from = now.saturating_sub(hours * 3_600_000);
        while from < now {
            let to = (from + CHUNK_MS).min(now);
            for e in source::from_archive(base, from, to).await? {
                if seen.insert(e.id.clone()) {
                    events.push(e);
                }
            }
            from = to + 1;
        }
        events.sort_by_key(|e| e.received_at_ms);
        return Ok((events, now));
    }
    let events = source::from_file(std::path::Path::new(spec), 0, u64::MAX)
        .await
        .with_context(|| format!("reading {spec}"))?;
    let last = events
        .iter()
        .map(|e| e.received_at_ms)
        .max()
        .context("記録に報がありません")?;
    Ok((events, last + rules.quiet_ms))
}

pub async fn run(spec: &str, hours: u64, cfg: &WorkerConfig) -> anyhow::Result<()> {
    let rules = Rules::from(cfg);
    let (events, now) = read(spec, hours, &rules).await?;
    let quakes = detect::quakes(&events, &rules);
    let groups = detect::groups(&quakes, &rules);
    println!(
        "報 {} 件、基準を満たす地震 {} 個、連続地震のまとまり {} 個 (今は {})",
        events.len(),
        quakes.len(),
        groups.len(),
        jst::format(now as i64)
    );
    for g in &groups {
        let (from, to) = g.range_ms();
        println!(
            "\n{}  {}  {}  動画の範囲 {} 〜 {}  地震 {} 個",
            g.id(),
            if g.is_closed(now, &rules) {
                "閉じた"
            } else {
                "開いている"
            },
            if g.capped {
                "(長さの上限で次へ続く)"
            } else {
                ""
            },
            jst::format(from as i64),
            jst::format(to as i64),
            g.quakes.len()
        );
        for q in &g.quakes {
            println!(
                "  {}  震度{}{}  {}  ({})",
                jst::format(q.origin_ms),
                Scale(q.max_scale).label(),
                if q.warning { " 警報" } else { "" },
                q.name,
                match (q.lat, q.lon) {
                    (Some(a), Some(o)) => format!("{a:.2}, {o:.2}"),
                    _ => "震源不明".into(),
                }
            );
        }
    }
    Ok(())
}
