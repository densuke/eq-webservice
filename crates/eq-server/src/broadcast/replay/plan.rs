//! 動画の筋書き (純粋な関数): 動画にする地震の選び方、始まり、時計を飛ばす区間、コマごとの当時の時刻、鳴らす音。
//! 描かずに決めるので、音を先に作り、映像はこの筋書きのとおりに描ける (docs/replay-video.md 4.1・4.3)。

use super::sound::{Sound, Sounds, Step};
use crate::broadcast::native::{
    eew_place, event_place, group_quakes, latest_eews, look, quake_place, same_quake, Hindsight,
};
use crate::quake::{Event, EventBody};

/// 始まり: 最初の揺れ (または最初の報) のこの時間前 (web の HISTORY_LEAD_MS)
const LEAD_MS: u64 = 10_000;
/// 次の報まで、これを超えて何も届かないときは、時計を飛ばす (web の HISTORY_MAX_GAP_MS)
const MAX_GAP_MS: u64 = 20_000;
/// 飛ばすとき、前の報のこの時間後から、次の報のこの時間前まで (web の HISTORY_GAP_MARGIN_MS)
const GAP_MARGIN_MS: u64 = 5_000;
/// 時計を飛ばしたあと、「早送り」を出し続ける時間 (web の FF_SHOW_MS)
const FF_SHOW_MS: u64 = 3_000;
/// 地震の画面が平時に戻ってから、動画を続ける時間
const CALM_TAIL_MS: u64 = 5_000;
/// 動画の長さの上限 (止まらなくなったときの保険)
const MAX_VIDEO_MS: u64 = 3_600_000;

/// 動画の 1 コマ
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// 当時の時刻 (epoch ミリ秒)
    pub now_ms: u64,
    /// このコマまでに届いた報の数 (報は届いた時刻の順)
    pub applied: usize,
    /// 時計を飛ばした直後 (「早送り」を出す)
    pub fast_forward: bool,
}

#[derive(Debug)]
pub struct Plan {
    pub frames: Vec<Frame>,
    pub sounds: Vec<Sound>,
}

/// 範囲の報から、最大震度が一番大きい地震の報だけを、届いた時刻の順に返す (地震情報と緊急地震速報だけ)。
/// 震度が同じなら、先に起きた地震
pub fn target_events(events: &[Event]) -> Vec<Event> {
    let quakes = group_quakes(events).into_iter().map(|q| (q.max_scale, quake_place(&q)));
    let eews = latest_eews(events).into_iter().map(|e| (e.max_scale, eew_place(&e)));
    let Some(target) = quakes
        .chain(eews)
        .filter(|(_, p)| p.origin_ms.is_some())
        .min_by_key(|(s, p)| (std::cmp::Reverse(*s), p.origin_ms))
        .map(|(_, p)| p)
    else {
        return Vec::new();
    };
    let mut out: Vec<Event> = events
        .iter()
        .filter(|e| event_place(e).is_some_and(|p| same_quake(&p, &target)))
        .cloned()
        .collect();
    out.sort_by_key(|e| e.received_at_ms);
    out
}

/// 始まりの時刻。緊急地震速報があれば、その秒単位の発生時刻の 10 秒前。無ければ最初の報が届いた時刻の 10 秒前
/// (地震情報の発生時刻は分単位で、報はその 1〜2 分後に届くため。web の historyStart)。報が無ければ None
pub fn history_start(events: &[Event]) -> Option<u64> {
    let eew_origin = events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Eew(x) => x.origin_time_ms,
            _ => None,
        })
        .min();
    let base = match eew_origin {
        Some(o) => u64::try_from(o).ok()?,
        None => events.iter().map(|e| e.received_at_ms).min()?,
    };
    Some(base.saturating_sub(LEAD_MS))
}

/// 報の届いた時刻 (時刻順) から、時計を飛ばす区間 [from, to) を出す。報の間が MAX_GAP_MS ちょうどまでは飛ばさない
/// (web の skipRanges)
pub fn skip_ranges(ats: &[u64]) -> Vec<(u64, u64)> {
    ats.windows(2)
        .filter(|w| w[1] - w[0] > MAX_GAP_MS)
        .map(|w| (w[0] + GAP_MARGIN_MS, w[1] - GAP_MARGIN_MS))
        .collect()
}

/// 動画の筋書きを作る。events は target_events の結果。コマは固定の fps で埋める
pub fn build(events: &[Event], hindsight: Option<&Hindsight>, fps: u32) -> Option<Plan> {
    let start = history_start(events)?;
    let ats: Vec<u64> = events.iter().map(|e| e.received_at_ms).collect();
    let last_at = *ats.last()?;
    let skips = skip_ranges(&ats);
    let mut sounds = Sounds::new(events);
    let (mut frames, mut out) = (Vec::new(), Vec::new());
    let (mut skipped, mut ff_until, mut applied, mut calm_since) = (0u64, 0u64, 0usize, None);
    for k in 0u64.. {
        let video_ms = k * 1000 / u64::from(fps);
        if video_ms > MAX_VIDEO_MS {
            break;
        }
        let mut now = start + video_ms + skipped;
        let jumped = match skips.iter().find(|(from, to)| (*from..*to).contains(&now)) {
            Some(&(_, to)) => {
                skipped += to - now;
                now = to;
                ff_until = video_ms + FF_SHOW_MS;
                true
            }
            None => false,
        };
        let fast_forward = video_ms < ff_until;
        let due_from = applied;
        applied = ats.partition_point(|&a| a <= now);
        let seen = &events[..applied];
        let look = look(seen, now, hindsight);
        out.extend(sounds.step(&Step {
            events: seen,
            due_from,
            video_ms,
            now_ms: now,
            origin_ms: hindsight.map(|h| h.origin_ms),
            jumped,
            fast_forward,
            waving: look.waving,
        }));
        frames.push(Frame {
            now_ms: now,
            applied,
            fast_forward,
        });
        // 最後の報のあと、地震の画面も波も無くなってから、数秒で終える
        if now >= last_at && look.calm && !look.waving {
            let since = *calm_since.get_or_insert(video_ms);
            if video_ms - since >= CALM_TAIL_MS {
                break;
            }
        } else {
            calm_since = None;
        }
    }
    Some(Plan { frames, sounds: out })
}

/// 時計を飛ばしたところ: (動画の中の時刻, 飛ぶ前の当時の時刻, 飛んだ先の当時の時刻)
pub fn jumps(frames: &[Frame], fps: u32) -> Vec<(u64, u64, u64)> {
    let step = 1000 / u64::from(fps);
    frames
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[1].now_ms - w[0].now_ms > step + 1)
        .map(|(i, w)| (duration_ms(i + 1, fps), w[0].now_ms, w[1].now_ms))
        .collect()
}

/// 動画の長さ (ミリ秒)
pub fn duration_ms(frames: usize, fps: u32) -> u64 {
    frames as u64 * 1000 / u64::from(fps)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;
    use crate::quake::{QuakeInfoType, Scale};

    const T: u64 = T0 as u64;

    /// 与那国のように、緊急地震速報の予報が続いたあと、地震情報が 2 分ほどあとに届く地震
    fn forecast_then_quake() -> Vec<Event> {
        vec![
            eew("E", 1, T + 5_000, T0, false, Scale::S2, TOKYO),
            eew("E", 2, T + 6_000, T0, false, Scale::S3, TOKYO),
            quake(
                T + 120_000,
                QuakeInfoType::DetailScale,
                T0 - 30_000,
                Scale::S3,
                Some(TOKYO),
            ),
        ]
    }

    #[test]
    fn the_target_is_the_quake_with_the_largest_scale_and_only_its_reports() {
        let hour = 3_600_000;
        let mut events = forecast_then_quake();
        events.push(quake(
            T + hour,
            QuakeInfoType::DetailScale,
            T0 + hour as i64,
            Scale::S5_UPPER,
            Some(OSAKA),
        ));
        events.push(quake(
            T + hour + 1,
            QuakeInfoType::ScalePrompt,
            T0 + hour as i64,
            Scale::S5_UPPER,
            None,
        ));
        assert_eq!(target_events(&events[..3]).len(), 3);
        // 震度が大きい方 (震度速報も、震源が無くても同じ地震に入る)
        let got = target_events(&events);
        assert_eq!(
            got.iter().map(|e| e.received_at_ms).collect::<Vec<_>>(),
            [T + hour, T + hour + 1]
        );
        assert!(target_events(&[]).is_empty());
    }

    #[test]
    fn the_start_is_ten_seconds_before_the_eew_origin_or_the_first_report() {
        let events = forecast_then_quake();
        assert_eq!(history_start(&events), Some(T - 10_000));
        // 緊急地震速報が無ければ、最初の報が届いた時刻の 10 秒前 (発生時刻の分単位ではなく)
        assert_eq!(history_start(&events[2..]), Some(T + 110_000));
        assert_eq!(history_start(&[]), None);
    }

    #[test]
    fn a_gap_of_exactly_twenty_seconds_is_kept_and_a_longer_one_is_skipped() {
        assert_eq!(skip_ranges(&[0, 20_000]), vec![]);
        assert_eq!(skip_ranges(&[0, 20_001, 21_000]), vec![(5_000, 15_001)]);
        assert_eq!(skip_ranges(&[7]), vec![]);
    }

    #[test]
    fn the_plan_steps_in_fixed_frames_skips_the_quiet_gap_and_ends_after_calm() {
        let events = forecast_then_quake();
        let h = crate::broadcast::native::hindsight_of(&events);
        let plan = build(&events, h.as_ref(), 5).unwrap();
        let f = &plan.frames;
        // 始まりは、発生の 10 秒前。コマは 200ms ずつ進み、飛ばしたところだけ大きく進む
        assert_eq!(f[0].now_ms, T - 10_000);
        assert_eq!(f[0].applied, 0);
        let jumps: Vec<_> = f.windows(2).filter(|w| w[1].now_ms - w[0].now_ms > 200).collect();
        assert_eq!(jumps.len(), 1);
        // 6 秒の報から 120 秒の報までは、前の報の 5 秒後から、次の報の 5 秒前まで飛ぶ
        assert_eq!(jumps[0][1].now_ms, T + 115_000);
        assert!(f.iter().all(|x| !(T + 11_001..T + 115_000).contains(&x.now_ms)));
        // 「早送り」は飛んだコマから 3 秒 (15 コマ)
        assert_eq!(f.iter().filter(|x| x.fast_forward).count(), 15);
        assert!(jumps[0][1].fast_forward && !jumps[0][0].fast_forward);
        // 報は届いた時刻の順に増える。最後は、全部届いたあと
        assert!(f.windows(2).all(|w| w[0].applied <= w[1].applied));
        assert_eq!(f.last().unwrap().applied, 3);
        // 最後の報 (120 秒) のあと、地震情報の画面 (震度 3 は 3 分) が平時に戻ってから、数秒で終わる
        let calm = T + 120_000 + 180_000;
        assert!(f.last().unwrap().now_ms >= calm + CALM_TAIL_MS - 200);
        assert!(f.last().unwrap().now_ms < calm + CALM_TAIL_MS + 1_500);
    }

    #[test]
    fn the_plan_plays_the_start_sound_once_and_no_ticks_while_fast_forwarding() {
        let events = forecast_then_quake();
        let h = crate::broadcast::native::hindsight_of(&events);
        let plan = build(&events, h.as_ref(), 5).unwrap();
        let starts: Vec<_> = plan.sounds.iter().filter(|s| s.why == "揺れの始まり").collect();
        assert_eq!(starts.len(), 1);
        // 発生 (T0) をまたいだコマ。震度 3 なのでチャイム
        assert_eq!(starts[0].now_ms, T);
        assert_eq!(starts[0].level, crate::broadcast::mixer::AlertLevel::Medium);
        let ff: Vec<u64> = plan
            .frames
            .iter()
            .enumerate()
            .filter(|(_, x)| x.fast_forward)
            .map(|(i, _)| i as u64 * 200)
            .collect();
        assert!(plan
            .sounds
            .iter()
            .all(|s| !(ff[0]..=*ff.last().unwrap()).contains(&s.video_ms)));
        // 波の間 (発生から 180 秒) は、2 秒ごとに刻む
        let pips = plan.sounds.iter().filter(|s| s.why == "波の刻み").count();
        assert!(pips > 20, "pips = {pips}");
        assert!(plan.sounds.windows(2).all(|w| w[0].video_ms <= w[1].video_ms));
    }

    #[test]
    fn no_reports_make_no_plan() {
        assert!(build(&[], None, 5).is_none());
    }
}
