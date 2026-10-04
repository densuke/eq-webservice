//! sound_for (mixer への知らせ) の確認。読み上げは voice が true のときだけ知らせる。

use tokio::sync::mpsc::unbounded_channel;

use super::*;

const T: u64 = 1_790_000_000_000;

fn tsunami_msg(id: &str, grade: &str) -> ServerMessage {
    let event = serde_json::json!({
        "id": id, "source": "t", "received_at_ms": T, "kind": "tsunami",
        "cancelled": false, "issued_at": "2026-01-01T00:00:00+09:00",
        "areas": [{"name": "宮城県", "grade": grade, "immediate": false}],
    });
    ServerMessage::Event {
        server_time_ms: T,
        event,
    }
}

fn sent(voice: bool) -> Vec<String> {
    let (tx, mut rx) = unbounded_channel();
    sound_for(
        &mut Vec::new(),
        &mut Gate::default(),
        &tsunami_msg("t1", "watch"),
        "http://s",
        voice,
        &tx,
    );
    let mut out = Vec::new();
    while let Ok(n) = rx.try_recv() {
        out.push(n);
    }
    out
}

#[test]
fn a_new_tsunami_is_read_when_voice_is_on() {
    assert_eq!(sent(true), vec![model::voice_notice("http://s/api/tts/event/t1")]);
}

#[test]
fn nothing_is_sent_when_voice_is_off() {
    assert!(sent(false).is_empty());
}

// ---- 地震感知情報 (docs/tts.md S12) ----

// 2026/09/29 10:00:00 JST
const U0: u64 = 1_790_643_600_000;

fn uq_msg(id: &str, started: &str, updated: &str, confidence: f64, at: u64) -> ServerMessage {
    let event = serde_json::json!({
        "id": id, "source": "p2pquake", "received_at_ms": at, "kind": "userquake",
        "started_at": started, "updated_at": updated, "count": 5, "confidence": confidence,
        "areas": [{"code": 205, "count": 3, "confidence": 0.9}],
    });
    ServerMessage::Event {
        server_time_ms: at,
        event,
    }
}

fn quake_msg(id: &str, at: u64) -> ServerMessage {
    let event = serde_json::json!({
        "id": id, "source": "p2pquake", "received_at_ms": at, "kind": "quake",
        "info_type": "scale_prompt", "origin_time": "2026/09/29 10:00:00", "origin_time_ms": U0,
        "issued_at": "2026/09/29 10:01:00", "hypocenter": null, "max_scale": 30,
        "domestic_tsunami": "None", "points": [], "pref_max": [], "comment": "",
    });
    ServerMessage::Event {
        server_time_ms: at,
        event,
    }
}

/// 順に流して、そのつど mixer に送った知らせを集める
fn run(msgs: &[ServerMessage], voice: bool) -> Vec<Vec<String>> {
    let (tx, mut rx) = unbounded_channel();
    let (mut live, mut gate) = (Vec::new(), Gate::default());
    msgs.iter()
        .map(|m| {
            sound_for(&mut live, &mut gate, m, "http://s", voice, &tx);
            std::iter::from_fn(|| rx.try_recv().ok()).collect()
        })
        .collect()
}

fn chime_then_voice(id: &str) -> Vec<String> {
    vec![
        model::alert_notice(AlertLevel::Info),
        model::voice_notice(&format!("http://s/api/tts/event/{id}")),
    ]
}

const S: &str = "2026/09/29 10:00:00.000";

#[test]
fn a_credible_userquake_is_announced_once_with_an_info_chime() {
    let out = run(
        &[
            uq_msg("u1", S, "2026/09/29 10:00:10.000", 0.97, U0 + 10_000),
            uq_msg("u2", S, "2026/09/29 10:00:20.000", 0.98, U0 + 20_000),
        ],
        true,
    );
    assert_eq!(out[0], chime_then_voice("u1"));
    // 同じ揺れの更新では読み直さない
    assert!(out[1].is_empty());
}

#[test]
fn a_weak_userquake_is_not_announced() {
    let out = run(&[uq_msg("u1", S, "2026/09/29 10:00:10.000", 0.5, U0 + 10_000)], true);
    assert!(out[0].is_empty());
}

#[test]
fn a_userquake_after_an_official_report_is_not_announced() {
    let out = run(
        &[
            quake_msg("q1", U0 + 5_000),
            uq_msg("u1", S, "2026/09/29 10:00:10.000", 0.97, U0 + 10_000),
        ],
        true,
    );
    assert!(out[1].is_empty());
}

#[test]
fn userquakes_closer_than_ten_minutes_are_announced_once() {
    let out = run(
        &[
            uq_msg("u1", S, "2026/09/29 10:00:10.000", 0.97, U0 + 10_000),
            uq_msg(
                "u2",
                "2026/09/29 10:05:00.000",
                "2026/09/29 10:05:10.000",
                0.97,
                U0 + 310_000,
            ),
            uq_msg(
                "u3",
                "2026/09/29 10:11:00.000",
                "2026/09/29 10:11:10.000",
                0.97,
                U0 + 670_000,
            ),
        ],
        true,
    );
    assert_eq!(out[0], chime_then_voice("u1"));
    assert!(out[1].is_empty());
    assert_eq!(out[2], chime_then_voice("u3"));
}

#[test]
fn a_userquake_is_silent_when_voice_is_off() {
    let out = run(&[uq_msg("u1", S, "2026/09/29 10:00:10.000", 0.97, U0 + 10_000)], false);
    assert!(out[0].is_empty());
}
