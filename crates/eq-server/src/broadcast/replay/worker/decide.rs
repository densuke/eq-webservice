//! 作り始めてよいか・作っている間に止める (kill) か凍結する (freeze) かの判断 (純粋な関数。docs/replay-video.md 5.1・5.5)。
//! ライブの配信と本番が最優先で、動画作りはいつでも止めてよい後回しの仕事。
//! 配信の状態・e2 の詰まり・YouTube の健全性・凍結の長さを受けて、何をするかだけを返す。

use std::time::Duration;

use super::super::super::calm_state::Screen;
use super::config::OnBusy;
use super::health::Health;
use super::hours::Hours;

/// 今の様子
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seen {
    pub screen: Screen,
    /// e2 が詰まっているか (PSI)
    pub congested: bool,
    pub health: Health,
    /// e2 のメモリの空き (MB)。読めない (Mac) ときは None
    pub mem_available_mb: Option<u64>,
}

/// 作り始める条件の設定
#[derive(Debug, Clone, Copy)]
pub struct StartRules {
    pub calm_ms: u64,
    pub hours: Hours,
    pub min_mem_mb: u64,
}

/// 作り始めてよいか: 作る時間帯で、配信が平時で、最後の地震の画面から calm_ms たち、詰まっていなくて、健全性が落ちていなくて、
/// メモリの空きが足りているとき。配信の状態が読めない・古いとき (Unknown) は、地震の画面と同じに扱って始めない
pub fn may_start(o: &Seen, now_ms: u64, r: &StartRules) -> bool {
    let calm_long_enough =
        matches!(o.screen, Screen::Calm { since_ms } if now_ms.saturating_sub(since_ms) >= r.calm_ms);
    let enough_memory = o.mem_available_mb.is_none_or(|m| m >= r.min_mem_mb);
    r.hours.contains(now_ms) && calm_long_enough && !o.congested && o.health != Health::Bad && enough_memory
}

/// 止めた理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// 地震の画面になった (配信の状態がわからなくなったのを含む)
    Quake,
    /// e2 が詰まった、または YouTube の健全性が落ちた (on_busy = kill)。やり直しに数える
    Busy,
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
/// 地震の画面なら、凍結中でもすぐ止める (作りかけを抱えたままにせず、メモリを配信と本番に回す)。
/// 重いとき (詰まり・健全性) は、on_busy が kill ならすぐ止める。凍結は CPU とディスクを手放すがメモリは抱えたままなので、freeze は選んだときだけ
pub fn watch(o: &Seen, frozen_for: Option<Duration>, give_up: Duration, on_busy: OnBusy) -> Action {
    if !matches!(o.screen, Screen::Calm { .. }) {
        return Action::Kill(Why::Quake);
    }
    if frozen_for.is_some_and(|d| d >= give_up) {
        return Action::Kill(Why::FrozenTooLong);
    }
    let heavy = o.congested || o.health == Health::Bad;
    match (heavy, on_busy, frozen_for.is_some()) {
        (true, OnBusy::Kill, _) => Action::Kill(Why::Busy),
        (true, OnBusy::Freeze, false) => Action::Freeze,
        (false, _, true) => Action::Thaw,
        _ => Action::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-01 02:00 JST
    const NOW: u64 = 1_790_780_400_000 + 2 * 3_600_000;
    const CALM_MS: u64 = 30 * 60_000;
    const GIVE_UP: Duration = Duration::from_secs(600);

    fn rules(hours: &str) -> StartRules {
        StartRules {
            calm_ms: CALM_MS,
            hours: Hours::parse(hours).unwrap(),
            min_mem_mb: 250,
        }
    }

    fn seen(screen: Screen, congested: bool, health: Health) -> Seen {
        Seen {
            screen,
            congested,
            health,
            mem_available_mb: Some(400),
        }
    }

    fn calm(since_ms: u64) -> Screen {
        Screen::Calm { since_ms }
    }

    #[test]
    fn it_starts_only_when_calm_for_thirty_minutes_and_nothing_is_heavy() {
        let ok = |s: Seen| may_start(&s, NOW, &rules("1-5"));
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
    fn it_starts_only_inside_the_hours() {
        let s = seen(calm(0), false, Health::Good);
        assert!(may_start(&s, NOW, &rules("1-5")));
        assert!(!may_start(&s, NOW, &rules("3-5")));
        // 日をまたぐ時間帯
        assert!(may_start(&s, NOW, &rules("22-3")));
        // 5 時ちょうどは時間帯の外
        assert!(!may_start(&s, NOW + 3 * 3_600_000, &rules("1-5")));
    }

    #[test]
    fn it_needs_enough_free_memory_but_an_unreadable_one_does_not_block() {
        let with = |m: Option<u64>| Seen {
            mem_available_mb: m,
            ..seen(calm(0), false, Health::Good)
        };
        assert!(may_start(&with(Some(250)), NOW, &rules("1-5")));
        assert!(!may_start(&with(Some(249)), NOW, &rules("1-5")));
        // Mac など、読めないとき
        assert!(may_start(&with(None), NOW, &rules("1-5")));
    }

    #[test]
    fn a_quake_screen_or_an_unknown_state_kills_even_a_frozen_job_at_once() {
        for screen in [Screen::Quake, Screen::Unknown] {
            for frozen in [None, Some(Duration::from_secs(1))] {
                for on_busy in [OnBusy::Kill, OnBusy::Freeze] {
                    assert_eq!(
                        watch(&seen(screen, false, Health::Good), frozen, GIVE_UP, on_busy),
                        Action::Kill(Why::Quake)
                    );
                }
            }
        }
    }

    #[test]
    fn a_heavy_machine_or_a_bad_stream_kills_by_default() {
        let calm = calm(0);
        let w = |s: Seen| watch(&s, None, GIVE_UP, OnBusy::Kill);
        assert_eq!(w(seen(calm, true, Health::Good)), Action::Kill(Why::Busy));
        assert_eq!(w(seen(calm, false, Health::Bad)), Action::Kill(Why::Busy));
        // 何も無ければ続ける。健全性がわからないだけでは止めない
        assert_eq!(w(seen(calm, false, Health::Good)), Action::Keep);
        assert_eq!(w(seen(calm, false, Health::Unknown)), Action::Keep);
    }

    #[test]
    fn with_freeze_a_heavy_machine_freezes_and_a_calm_one_thaws() {
        let calm = calm(0);
        let w = |s: Seen, f: Option<Duration>| watch(&s, f, GIVE_UP, OnBusy::Freeze);
        assert_eq!(w(seen(calm, true, Health::Good), None), Action::Freeze);
        assert_eq!(w(seen(calm, false, Health::Bad), None), Action::Freeze);
        // 凍結中に、まだ重ければそのまま。落ち着けば解凍
        let f = Some(Duration::from_secs(60));
        assert_eq!(w(seen(calm, true, Health::Good), f), Action::Keep);
        assert_eq!(w(seen(calm, false, Health::Bad), f), Action::Keep);
        assert_eq!(w(seen(calm, false, Health::Good), f), Action::Thaw);
        assert_eq!(w(seen(calm, false, Health::Unknown), f), Action::Thaw);
        assert_eq!(w(seen(calm, false, Health::Good), None), Action::Keep);
    }

    #[test]
    fn ten_minutes_frozen_kills_and_returns_to_the_queue() {
        let s = seen(calm(0), true, Health::Good);
        let w = |s: &Seen, d: Duration| watch(s, Some(d), GIVE_UP, OnBusy::Freeze);
        assert_eq!(w(&s, Duration::from_secs(599)), Action::Keep);
        assert_eq!(w(&s, GIVE_UP), Action::Kill(Why::FrozenTooLong));
        // 落ち着いていても、10 分を超えて凍結していたら止める (続きから作るより、作り直す)
        let quiet = seen(calm(0), false, Health::Good);
        assert_eq!(w(&quiet, GIVE_UP), Action::Kill(Why::FrozenTooLong));
    }
}
