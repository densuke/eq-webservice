//! 記録済みの上流メッセージ (P2P地震情報形式の JSON Lines) を再生する。
//! 地震が起きていないときの画面確認・プラグイン開発用。

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use eq_core::{p2pquake, Event};

use crate::hub::{now_ms, Hub};

pub struct Options {
    pub speed: f64,
    pub max_gap_ms: u64,
    pub rebase_time: bool,
    pub repeat: bool,
}

pub async fn run(path: &Path, opts: &Options, hub: &Hub) -> anyhow::Result<()> {
    let text = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let events = load(&text)?;
    anyhow::ensure!(!events.is_empty(), "{} has no events", path.display());
    let mut round = 0u32;
    loop {
        play(&events, opts, round, hub).await;
        if !opts.repeat {
            tracing::info!("replay finished");
            return Ok(());
        }
        round += 1;
        tokio::time::sleep(Duration::from_millis(opts.max_gap_ms)).await;
    }
}

fn load(text: &str) -> anyhow::Result<Vec<Event>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(ev) = p2pquake::parse(line).with_context(|| format!("line {}", i + 1))? {
            out.push(ev);
        }
    }
    Ok(out)
}

async fn play(events: &[Event], opts: &Options, round: u32, hub: &Hub) {
    let speed = if opts.speed > 0.0 { opts.speed } else { 1.0 };
    let mut prev: Option<i64> = None;
    // 記録上の時刻 → 再生時の時刻 へのずれ
    let mut delta = 0i64;
    // 発生時刻のずれは周回の最初の情報で決めて固定する。情報ごとに変えると、
    // 同じ地震の震度速報と各地の震度で発生時刻が変わり、別の地震に見えてしまう
    let mut origin_delta: Option<i64> = None;
    for ev in events {
        let recorded = ev.issued_at_ms();
        let wait_ms = match (prev, recorded) {
            (Some(p), Some(r)) if r > p => (((r - p) as f64 / speed) as u64).min(opts.max_gap_ms),
            (None, _) => 0,
            _ => 500,
        };
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        if let Some(r) = recorded {
            prev = Some(r);
            // 待ち時間を詰めた・倍速にした分だけずれを補正し、発表時刻がいつも「今」になるようにする
            delta = now_ms() as i64 - r;
            origin_delta.get_or_insert(delta);
        }
        let mut ev = ev.clone();
        // 同じ ID は重複として捨てられるので、周回ごとに変える
        ev.id = format!("{}#replay{round}", ev.id);
        // 画面側で「再生データ」と分かるように (訓練報でも警戒音を鳴らす)
        ev.source = "replay".into();
        if let eq_core::EventBody::Eew(e) = &mut ev.body {
            // EEW の続報は event_id でまとめられるので、これも周回ごとに変える
            e.event_id = format!("{}#replay{round}", e.event_id);
        }
        if opts.rebase_time {
            ev.shift_times(delta, origin_delta.unwrap_or(delta));
        }
        let title = ev.title();
        if hub.publish(ev) {
            tracing::info!(%title, "replayed event");
        }
    }
}
