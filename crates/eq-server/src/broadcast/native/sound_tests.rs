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
    sound_for(&mut Vec::new(), &tsunami_msg("t1", "watch"), "http://s", voice, &tx);
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
