//! 動画の音の規則 (純粋な関数)。web の履歴の再生と同じ (docs/quake-archive.md 2.7 章)。
//! web/src/alert.ts (alertLevel)、replay-sound.ts (pipSlot・stepStartSound・startSoundLevel)、main.ts (playAlert の 3 秒のまとめ)。
//! 訓練報は鳴らさない (web は再生の訓練報だけ鳴らすが、動画は本物の記録なので鳴らさない)。

use crate::broadcast::mixer::AlertLevel;
use crate::broadcast::native::{eew_place, event_place, latest_eews, same_quake, EEW_ACTIVE_MS};
use crate::quake::{Event, EventBody, Scale};

/// 揺れの始まりの音を鳴らす範囲: 発生からこの時間以内 (web の WAVE_MAX_SEC)
const START_WITHIN_MS: i64 = 180_000;
/// この時間内に続けて鳴らす音は、前より強いときだけ鳴らす (web の ALERT_MERGE_MS)
const MERGE_MS: u64 = 3_000;
/// 波の刻みの間隔
const PIP_MS: u64 = 2_000;

/// 鳴らす音 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    /// 動画の中の時刻 (ミリ秒)
    pub video_ms: u64,
    /// 当時の時刻 (epoch ミリ秒)
    pub now_ms: u64,
    pub level: AlertLevel,
    pub why: String,
}

/// 1 コマで音の判断に使うもの
pub struct Step<'a> {
    /// ここまでに届いた報 (このコマで届いたものを含む)
    pub events: &'a [Event],
    /// このコマで届いた報の始まりの位置
    pub due_from: usize,
    pub video_ms: u64,
    pub now_ms: u64,
    /// 揺れの始まり (のちの報で分かった発生時刻)
    pub origin_ms: Option<i64>,
    /// 時計を飛ばしたコマか
    pub jumped: bool,
    /// 「早送り」を出している間か
    pub fast_forward: bool,
    /// 地震波を描いているか
    pub waving: bool,
}

pub struct Sounds {
    /// 前のコマの当時の時刻と、揺れの始まりの音を鳴らした地震 (発生時刻)
    start: (i64, Vec<i64>),
    start_level: AlertLevel,
    last_pip: i64,
    last_alert: Option<(AlertLevel, u64)>,
}

impl Sounds {
    /// events は動画に入れる報すべて (揺れの始まりの音の強さを決めるのに、最大震度を見る)
    pub fn new(events: &[Event]) -> Self {
        let max = events
            .iter()
            .filter_map(|e| e.max_scale())
            .max()
            .unwrap_or(Scale::UNKNOWN);
        Sounds {
            start: (i64::MIN, Vec::new()),
            start_level: if max >= Scale::S3 {
                AlertLevel::Medium
            } else {
                AlertLevel::Low
            },
            last_pip: -1,
            last_alert: None,
        }
    }

    /// 1 コマ分を進めて、そのコマで鳴らす音を返す
    pub fn step(&mut self, s: &Step) -> Vec<Sound> {
        let mut out = Vec::new();
        let sound = |level, why: String| Sound {
            video_ms: s.video_ms,
            now_ms: s.now_ms,
            level,
            why,
        };
        if self.step_start(s) {
            out.extend(
                self.merged(s.now_ms, self.start_level)
                    .map(|l| sound(l, "揺れの始まり".into())),
            );
        }
        if let Some((level, why)) = due_alert(s) {
            out.extend(self.merged(s.now_ms, level).map(|l| sound(l, why)));
        }
        let slot = if s.waving && !s.fast_forward {
            (s.now_ms / PIP_MS) as i64
        } else {
            -1
        };
        if slot > self.last_pip && self.last_pip != -1 {
            out.push(sound(AlertLevel::Pip, "波の刻み".into()));
        }
        self.last_pip = slot;
        out
    }

    /// 発生時刻をまたいだコマ (または始まりで既に波が出ているコマ) で、地震ごとに 1 回だけ真。飛んだコマでは鳴らさない
    fn step_start(&mut self, s: &Step) -> bool {
        let next = s.now_ms as i64;
        let crossed = s
            .origin_ms
            .filter(|&o| self.start.0 < o && o <= next && next - o <= START_WITHIN_MS);
        let fresh = crossed.filter(|o| !self.start.1.contains(o));
        if let Some(o) = fresh {
            self.start.1.push(o);
        }
        self.start.0 = next;
        fresh.is_some() && !s.jumped
    }

    /// 3 秒以内に続けて鳴らすときは、前より強いときだけ鳴らす
    fn merged(&mut self, now_ms: u64, level: AlertLevel) -> Option<AlertLevel> {
        if let Some((last, at)) = self.last_alert {
            if now_ms.saturating_sub(at) < MERGE_MS && rank(level) <= rank(last) {
                return None;
            }
        }
        self.last_alert = Some((level, now_ms));
        Some(level)
    }
}

fn rank(l: AlertLevel) -> u8 {
    match l {
        AlertLevel::Strong => 3,
        AlertLevel::Medium => 2,
        AlertLevel::Low => 1,
        _ => 0,
    }
}

/// このコマで届いた報のうち、いちばん強い警戒音
fn due_alert(s: &Step) -> Option<(AlertLevel, String)> {
    (s.due_from..s.events.len())
        .filter_map(|i| {
            let level = alert_level(&s.events[..=i], s.now_ms)?;
            Some((level, s.events[i].title()))
        })
        .max_by_key(|(l, _)| rank(*l))
}

/// 最後の報 (events の末尾) で鳴らす警戒音。web の alertLevel と、その呼び出し (main.ts の onEvents) の判断
fn alert_level(events: &[Event], now_ms: u64) -> Option<AlertLevel> {
    let (e, prior) = events.split_last()?;
    match &e.body {
        EventBody::Eew(x) => {
            let same = |p: &&Event| matches!(&p.body, EventBody::Eew(y) if y.event_id == x.event_id);
            // 予報から警報に上がったときも最初の報とみなす
            let first = !prior.iter().any(|p| same(&p))
                || (x.warning
                    && !prior
                        .iter()
                        .filter(same)
                        .any(|p| matches!(&p.body, EventBody::Eew(y) if y.warning)));
            if x.cancelled || x.test {
                return None;
            }
            if first {
                return if x.warning {
                    Some(AlertLevel::Strong)
                } else {
                    by_scale(x.max_scale)
                };
            }
            // 最初でない報は、同じ地震のそれまでの最大震度を超えたときだけ、新しい震度で鳴らす (震度不明は最小)
            let prev_max = prior
                .iter()
                .filter(same)
                .filter_map(|p| match &p.body {
                    EventBody::Eew(y) => Some(y.max_scale),
                    _ => None,
                })
                .max()?;
            (x.max_scale > prev_max).then(|| by_scale(x.max_scale)).flatten()
        }
        EventBody::Quake(q) => {
            // 地震情報の最初の報は、その地震で最初に届いた地震情報 (連続地震では、地震ごと)
            let place = event_place(e)?;
            let earlier = |p: &&Event| {
                matches!(p.body, EventBody::Quake(_))
                    && event_place(p).is_some_and(|pp| {
                        pp.origin_ms.is_none() || place.origin_ms.is_none() || same_quake(&pp, &place)
                    })
            };
            if prior.iter().any(|p| earlier(&p)) {
                return Some(AlertLevel::Info);
            }
            // その地震の緊急地震速報で既に鳴らしていれば、地震情報の最初の報は案内音にする
            let eew_active = latest_eews(events)
                .iter()
                .any(|a| now_ms.saturating_sub(a.received_ms) < EEW_ACTIVE_MS && same_quake(&eew_place(a), &place));
            if eew_active {
                Some(AlertLevel::Info)
            } else {
                by_scale(q.max_scale)
            }
        }
        _ => None,
    }
}

fn by_scale(s: Scale) -> Option<AlertLevel> {
    if s >= Scale::S3 {
        Some(AlertLevel::Medium)
    } else if s.0 > 0 {
        Some(AlertLevel::Low)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;
    use crate::quake::QuakeInfoType;

    const T: u64 = T0 as u64;

    fn step(events: &[Event], due_from: usize, now_ms: u64) -> Step<'_> {
        Step {
            events,
            due_from,
            video_ms: now_ms.saturating_sub(T),
            now_ms,
            origin_ms: None,
            jumped: false,
            fast_forward: false,
            waving: false,
        }
    }

    fn levels(s: &mut Sounds, st: &Step) -> Vec<AlertLevel> {
        s.step(st).into_iter().map(|x| x.level).collect()
    }

    #[test]
    fn the_first_report_rings_by_scale_and_later_ones_ring_only_when_the_scale_rises() {
        let ev = [
            eew("E", 1, T, T0, false, Scale::S2, TOKYO),
            eew("E", 2, T + 1_000, T0, false, Scale::S4, TOKYO),
            eew("E", 3, T + 2_000, T0, false, Scale::S4, TOKYO),
            eew("E", 4, T + 3_000, T0, false, Scale::S3, TOKYO),
        ];
        assert_eq!(alert_level(&ev[..1], T), Some(AlertLevel::Low));
        // 震度が上がった報は、新しい震度で鳴らす。同じ・下がった報は鳴らさない
        assert_eq!(alert_level(&ev[..2], T + 1_000), Some(AlertLevel::Medium));
        assert_eq!(alert_level(&ev[..3], T + 2_000), None);
        assert_eq!(alert_level(&ev, T + 3_000), None);
        let big = [eew("E", 1, T, T0, false, Scale::S3, TOKYO)];
        assert_eq!(alert_level(&big, T), Some(AlertLevel::Medium));
        // 震度が分からない予報は鳴らさない
        let unknown = [eew("E", 1, T, T0, false, Scale::UNKNOWN, TOKYO)];
        assert_eq!(alert_level(&unknown, T), None);
    }

    #[test]
    fn a_forecast_that_becomes_a_warning_rings_strong_once() {
        let ev = [
            eew("E", 1, T, T0, false, Scale::S3, TOKYO),
            eew("E", 2, T + 1_000, T0, true, Scale::S5_LOWER, TOKYO),
            eew("E", 3, T + 2_000, T0, true, Scale::S5_LOWER, TOKYO),
        ];
        assert_eq!(alert_level(&ev[..2], T + 1_000), Some(AlertLevel::Strong));
        assert_eq!(alert_level(&ev, T + 2_000), None);
        // 別の地震 (event_id) の最初の報は、また鳴る
        let other = [&ev[..], &[eew("F", 1, T + 3_000, T0, true, Scale::S5_LOWER, TOKYO)]].concat();
        assert_eq!(alert_level(&other, T + 3_000), Some(AlertLevel::Strong));
    }

    #[test]
    fn test_and_cancelled_reports_are_silent() {
        let t = [as_test(eew("E", 1, T, T0, true, Scale::S5_LOWER, TOKYO))];
        assert_eq!(alert_level(&t, T), None);
        let c = [as_cancelled(eew("E", 1, T, T0, false, Scale::S4, TOKYO))];
        assert_eq!(alert_level(&c, T), None);
    }

    #[test]
    fn the_first_quake_report_rings_by_scale_and_an_active_eew_makes_it_the_info_sound() {
        let q = |recv, info| quake(recv, info, T0 - 20_000, Scale::S4, Some(TOKYO));
        // 緊急地震速報なし: 最初の地震情報は震度で鳴り、続く報は案内音
        let ev = [
            q(T, QuakeInfoType::ScalePrompt),
            q(T + 1_000, QuakeInfoType::Destination),
            q(T + 2_000, QuakeInfoType::DetailScale),
        ];
        assert_eq!(alert_level(&ev[..1], T), Some(AlertLevel::Medium));
        assert_eq!(alert_level(&ev[..2], T + 1_000), Some(AlertLevel::Info));
        assert_eq!(alert_level(&ev, T + 2_000), Some(AlertLevel::Info));
        // 3 分以内の緊急地震速報があれば、地震情報の最初の報は案内音。3 分を過ぎていれば震度で鳴らす
        let with_eew = [
            eew("E", 1, T, T0, false, Scale::S3, TOKYO),
            q(T + 60_000, QuakeInfoType::ScalePrompt),
        ];
        assert_eq!(alert_level(&with_eew, T + 60_000), Some(AlertLevel::Info));
        let late = [
            eew("E", 1, T, T0, false, Scale::S3, TOKYO),
            q(T + 200_000, QuakeInfoType::ScalePrompt),
        ];
        assert_eq!(alert_level(&late, T + 200_000), Some(AlertLevel::Medium));
    }

    #[test]
    fn every_report_that_changes_the_screen_rings_in_the_tokara_sequence() {
        // 2026-10-02 04:05 トカラ列島近海: 緊急地震速報 6 報 (震度不明 → 3)、震度速報・震源・各地の震度
        let mut ev = vec![eew("E", 1, T, T0, false, Scale::UNKNOWN, TOKYO)];
        for n in 2..=6 {
            ev.push(eew("E", n, T + n as u64 * 1_000, T0, false, Scale::S3, TOKYO));
        }
        ev.push(quake(T + 10_000, QuakeInfoType::ScalePrompt, T0, Scale::S3, None));
        ev.push(quake(
            T + 20_000,
            QuakeInfoType::Destination,
            T0,
            Scale::UNKNOWN,
            Some(TOKYO),
        ));
        ev.push(quake(
            T + 30_000,
            QuakeInfoType::DetailScale,
            T0,
            Scale::S3,
            Some(TOKYO),
        ));
        let got: Vec<_> = (0..ev.len())
            .map(|i| alert_level(&ev[..=i], ev[i].received_at_ms))
            .collect();
        let (m, i) = (Some(AlertLevel::Medium), Some(AlertLevel::Info));
        assert_eq!(got, [None, m, None, None, None, None, i, i, i]);
    }

    #[test]
    fn sounds_within_three_seconds_keep_only_a_stronger_one() {
        let ev = [
            eew("E", 1, T, T0, false, Scale::S2, TOKYO),
            eew("F", 1, T + 1_000, T0, false, Scale::S2, TOKYO),
            eew("G", 1, T + 2_000, T0, true, Scale::S5_LOWER, TOKYO),
            eew("H", 1, T + 6_000, T0, false, Scale::S2, TOKYO),
        ];
        let mut s = Sounds::new(&ev);
        assert_eq!(levels(&mut s, &step(&ev[..1], 0, T)), [AlertLevel::Low]);
        // 同じ強さは 3 秒以内なら鳴らさない
        assert!(levels(&mut s, &step(&ev[..2], 1, T + 1_000)).is_empty());
        // 強いものは鳴らす
        assert_eq!(levels(&mut s, &step(&ev[..3], 2, T + 2_000)), [AlertLevel::Strong]);
        // 3 秒を過ぎれば、弱くても鳴らす
        assert_eq!(levels(&mut s, &step(&ev, 3, T + 6_000)), [AlertLevel::Low]);
    }

    #[test]
    fn one_frame_with_several_reports_rings_the_strongest_once() {
        let ev = [
            eew("E", 1, T, T0, false, Scale::S2, TOKYO),
            eew("F", 1, T, T0, true, Scale::S5_LOWER, TOKYO),
        ];
        let mut s = Sounds::new(&ev);
        assert_eq!(levels(&mut s, &step(&ev, 0, T)), [AlertLevel::Strong]);
    }

    #[test]
    fn the_start_sound_rings_once_at_the_origin_and_not_after_a_jump() {
        let ev = [quake(T, QuakeInfoType::DetailScale, T0, Scale::S2, Some(TOKYO))];
        let at = |now, jumped| Step {
            origin_ms: Some(T0),
            jumped,
            ..step(&[], 0, now)
        };
        // 震度 2 はピンポン (low)
        let mut s = Sounds::new(&ev);
        assert!(levels(&mut s, &at(T - 200, false)).is_empty());
        assert_eq!(levels(&mut s, &at(T + 100, false)), [AlertLevel::Low]);
        assert!(levels(&mut s, &at(T + 300, false)).is_empty());
        // 飛んだコマでまたいだときは鳴らさず、そのあとも鳴らさない
        let mut s = Sounds::new(&ev);
        assert!(levels(&mut s, &at(T - 5_000, false)).is_empty());
        assert!(levels(&mut s, &at(T + 5_000, true)).is_empty());
        assert!(levels(&mut s, &at(T + 5_200, false)).is_empty());
        // 始まりで既に波が出ている (発生から 180 秒以内) ときは、最初のコマで鳴る
        let mut s = Sounds::new(&ev);
        assert_eq!(levels(&mut s, &at(T + 90_000, false)), [AlertLevel::Low]);
        // 180 秒を過ぎていれば鳴らさない
        let mut s = Sounds::new(&ev);
        assert!(levels(&mut s, &at(T + 181_000, false)).is_empty());
        // 震度 3 以上はチャイム
        let big = [quake(T, QuakeInfoType::DetailScale, T0, Scale::S3, Some(TOKYO))];
        assert_eq!(
            levels(&mut Sounds::new(&big), &at(T + 100, false)),
            [AlertLevel::Medium]
        );
    }

    #[test]
    fn ticks_ring_every_two_seconds_while_waving_and_not_while_fast_forwarding() {
        let waving = |now, fast_forward| Step {
            waving: true,
            fast_forward,
            ..step(&[], 0, now)
        };
        let mut s = Sounds::new(&[]);
        // 最初の枠は鳴らさない (枠の番号を覚えるだけ)
        assert!(levels(&mut s, &waving(T, false)).is_empty());
        assert!(levels(&mut s, &waving(T + 1_000, false)).is_empty());
        let next_slot = (T / 2000 + 1) * 2000;
        assert_eq!(levels(&mut s, &waving(next_slot, false)), [AlertLevel::Pip]);
        assert!(levels(&mut s, &waving(next_slot + 500, false)).is_empty());
        assert_eq!(levels(&mut s, &waving(next_slot + 2_000, false)), [AlertLevel::Pip]);
        // 早送りの間は鳴らさず、枠を覚え直す (終わった直後に鳴らさない)
        assert!(levels(&mut s, &waving(next_slot + 4_000, true)).is_empty());
        assert!(levels(&mut s, &waving(next_slot + 4_200, false)).is_empty());
        assert_eq!(levels(&mut s, &waving(next_slot + 6_000, false)), [AlertLevel::Pip]);
        // 波が無ければ鳴らさない
        let quiet = Step {
            waving: false,
            ..step(&[], 0, next_slot + 8_000)
        };
        assert!(levels(&mut s, &quiet).is_empty());
    }
}
