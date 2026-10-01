//! 作り始めてよいか・作っている間に止める (kill) か凍結する (freeze) かの判断 (純粋な関数。docs/replay-video.md 5.1)。
//! ライブの配信と本番が最優先で、動画作りはいつでも止めてよい後回しの仕事。
//! 配信の状態・e2 の詰まり・YouTube の健全性・凍結の長さを受けて、何をするかだけを返す。

use std::time::Duration;

use super::super::super::calm_state::Screen;
use super::health::Health;

/// 今の様子
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seen {
    pub screen: Screen,
    /// e2 が詰まっているか (PSI)
    pub congested: bool,
    pub health: Health,
}

/// 作り始めてよいか: 配信が平時で、最後の地震の画面から calm_ms たち、詰まっていなくて、健全性が落ちていないとき。
/// 配信の状態が読めない・古いとき (Unknown) は、地震の画面と同じに扱って始めない
pub fn may_start(o: &Seen, now_ms: u64, calm_ms: u64) -> bool {
    let calm_long_enough = matches!(o.screen, Screen::Calm { since_ms } if now_ms.saturating_sub(since_ms) >= calm_ms);
    calm_long_enough && !o.congested && o.health != Health::Bad
}

/// 止めた理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// 地震の画面になった (配信の状態がわからなくなったのを含む)
    Quake,
    /// 凍結が長く続いた
    FrozenTooLong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Keep,
    /// 凍結する (作りかけを抱えたまま止める。解凍すれば続きから)
    Freeze,
    Thaw,
    /// 止めて、作りかけを消す
    Kill(Why),
}

/// 作っている間の見張り。frozen_for は凍結している時間 (凍結していなければ None)。
/// 地震の画面なら、凍結中でもすぐ止める (作りかけの約 180MB を抱えたままにせず、メモリを配信と本番に回す)
pub fn watch(o: &Seen, frozen_for: Option<Duration>, give_up: Duration) -> Action {
    if !matches!(o.screen, Screen::Calm { .. }) {
        return Action::Kill(Why::Quake);
    }
    if frozen_for.is_some_and(|d| d >= give_up) {
        return Action::Kill(Why::FrozenTooLong);
    }
    let heavy = o.congested || o.health == Health::Bad;
    match (heavy, frozen_for.is_some()) {
        (true, false) => Action::Freeze,
        (false, true) => Action::Thaw,
        _ => Action::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 10_000_000;
    const CALM_MS: u64 = 30 * 60_000;
    const GIVE_UP: Duration = Duration::from_secs(600);

    fn seen(screen: Screen, congested: bool, health: Health) -> Seen {
        Seen {
            screen,
            congested,
            health,
        }
    }

    fn calm(since_ms: u64) -> Screen {
        Screen::Calm { since_ms }
    }

    #[test]
    fn it_starts_only_when_calm_for_thirty_minutes_and_nothing_is_heavy() {
        let ok = |s: Seen| may_start(&s, NOW, CALM_MS);
        assert!(ok(seen(calm(NOW - CALM_MS), false, Health::Good)));
        // 健全性が読めない (Unknown) ときは、止める理由にしない
        assert!(ok(seen(calm(0), false, Health::Unknown)));
        // 最後の地震の画面から 30 分たっていない
        assert!(!ok(seen(calm(NOW - CALM_MS + 1), false, Health::Good)));
        assert!(!ok(seen(Screen::Quake, false, Health::Good)));
        // 配信の状態がわからないときは、地震の画面と同じ扱い
        assert!(!ok(seen(Screen::Unknown, false, Health::Good)));
        assert!(!ok(seen(calm(0), true, Health::Good)));
        assert!(!ok(seen(calm(0), false, Health::Bad)));
    }

    #[test]
    fn a_quake_screen_or_an_unknown_state_kills_even_a_frozen_job_at_once() {
        for screen in [Screen::Quake, Screen::Unknown] {
            for frozen in [None, Some(Duration::from_secs(1))] {
                assert_eq!(
                    watch(&seen(screen, false, Health::Good), frozen, GIVE_UP),
                    Action::Kill(Why::Quake)
                );
            }
        }
    }

    #[test]
    fn a_heavy_machine_or_a_bad_stream_freezes_and_a_calm_one_thaws() {
        let calm = calm(0);
        assert_eq!(watch(&seen(calm, true, Health::Good), None, GIVE_UP), Action::Freeze);
        assert_eq!(watch(&seen(calm, false, Health::Bad), None, GIVE_UP), Action::Freeze);
        // 凍結中に、まだ重ければそのまま。落ち着けば解凍
        let f = Some(Duration::from_secs(60));
        assert_eq!(watch(&seen(calm, true, Health::Good), f, GIVE_UP), Action::Keep);
        assert_eq!(watch(&seen(calm, false, Health::Bad), f, GIVE_UP), Action::Keep);
        assert_eq!(watch(&seen(calm, false, Health::Good), f, GIVE_UP), Action::Thaw);
        assert_eq!(watch(&seen(calm, false, Health::Unknown), f, GIVE_UP), Action::Thaw);
        // 何も無ければ続ける
        assert_eq!(watch(&seen(calm, false, Health::Good), None, GIVE_UP), Action::Keep);
    }

    #[test]
    fn ten_minutes_frozen_kills_and_returns_to_the_queue() {
        let s = seen(calm(0), true, Health::Good);
        assert_eq!(watch(&s, Some(Duration::from_secs(599)), GIVE_UP), Action::Keep);
        assert_eq!(watch(&s, Some(GIVE_UP), GIVE_UP), Action::Kill(Why::FrozenTooLong));
        // 落ち着いていても、10 分を超えて凍結していたら止める (続きから作るより、作り直す)
        let quiet = seen(calm(0), false, Health::Good);
        assert_eq!(watch(&quiet, Some(GIVE_UP), GIVE_UP), Action::Kill(Why::FrozenTooLong));
    }
}
