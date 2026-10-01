//! 動画にする地震の検知と、連続地震のまとめ (純粋な関数。docs/replay-video.md 5.2 の 2)。
//! 記録の報から地震を数え、基準 (震度・警報) を満たすものを、近い震源で続いたものどうしで 1 つのまとまりにする。
//! まとまりは、最後の報から一定時間静かになったら閉じる。時刻は引数で、時計には触れない。

use serde::{Deserialize, Serialize};

use super::super::super::native::{distance_km, eew_place, group_quakes, latest_eews, quake_place, same_quake, Place};
use super::config::WorkerConfig;
use crate::quake::jst;
use crate::quake::Event;

/// 動画の範囲の始まり: まとまりの最初の地震の発生のこの時間前 (地震情報の発生時刻は分単位なので余裕を見る)
const FROM_LEAD_MS: i64 = 120_000;
/// 動画の範囲の終わり: まとまりの最後の報のこの時間後 (そのあとに届く地震情報を取りこぼさない)
const TO_TAIL_MS: u64 = 600_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    pub min_scale: i32,
    pub warning: bool,
    pub join_km: f64,
    pub join_ms: i64,
    pub quiet_ms: u64,
    pub max_len_ms: i64,
}

impl From<&WorkerConfig> for Rules {
    fn from(c: &WorkerConfig) -> Self {
        Rules {
            min_scale: c.min_scale,
            warning: c.warning,
            join_km: c.join_km,
            join_ms: (c.join_min * 60_000) as i64,
            quiet_ms: c.quiet_min * 60_000,
            max_len_ms: (c.max_group_hours * 3_600_000) as i64,
        }
    }
}

/// 動画にする地震 1 つ
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quake {
    /// 発生時刻 (epoch ミリ秒)
    pub origin_ms: i64,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// 最大震度 (10 = 震度 1 ... 70 = 震度 7)。地震情報があればその観測、無ければ緊急地震速報の予測
    pub max_scale: i32,
    /// 緊急地震速報の警報が出たか
    pub warning: bool,
    pub name: String,
    /// 最後の報が届いた時刻
    pub last_recv_ms: u64,
}

impl Quake {
    pub fn place(&self) -> Place {
        Place {
            origin_ms: Some(self.origin_ms),
            lat: self.lat,
            lon: self.lon,
        }
    }
}

/// 記録の報から、基準を満たす地震を、発生の早い順に返す。
/// 地震情報と緊急地震速報は同じ地震 (same_quake) にまとめる。訓練報と、取り消された緊急地震速報は数えない
pub fn quakes(events: &[Event], rules: &Rules) -> Vec<Quake> {
    let mut found: Vec<Quake> = group_quakes(events)
        .iter()
        .filter_map(|q| {
            let p = quake_place(q);
            Some(Quake {
                origin_ms: p.origin_ms?,
                lat: p.lat,
                lon: p.lon,
                max_scale: q.max_scale.0,
                warning: false,
                name: q.hypocenter.as_ref().map(|h| h.name.clone()).unwrap_or_default(),
                last_recv_ms: q.updated_ms,
            })
        })
        .collect();
    for e in latest_eews(events).iter().filter(|e| !e.test) {
        let p = eew_place(e);
        let Some(origin_ms) = p.origin_ms else { continue };
        let name = e.hypocenter.as_ref().map(|h| h.name.clone()).unwrap_or_default();
        match found.iter_mut().find(|f| same_quake(&f.place(), &p)) {
            Some(f) => {
                f.warning |= e.warning;
                f.last_recv_ms = f.last_recv_ms.max(e.received_ms);
                // 地震情報に震度が無い (震源だけの報) ときは、予測の震度で補う
                if f.max_scale <= 0 {
                    f.max_scale = e.max_scale.0;
                }
                if f.name.is_empty() {
                    f.name = name;
                }
            }
            None => found.push(Quake {
                origin_ms,
                lat: p.lat,
                lon: p.lon,
                max_scale: e.max_scale.0,
                warning: e.warning,
                name,
                last_recv_ms: e.received_ms,
            }),
        }
    }
    found.retain(|q| q.max_scale >= rules.min_scale || (rules.warning && q.warning));
    found.sort_by_key(|q| q.origin_ms);
    found
}

/// 連続地震のまとまり
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// 発生の早い順
    pub quakes: Vec<Quake>,
    /// 長さの上限で、続くはずの地震を次のまとまりに回した (もう伸びないので、静かになるのを待たずに閉じる)
    pub capped: bool,
}

impl Group {
    fn new(q: Quake) -> Self {
        Group {
            quakes: vec![q],
            capped: false,
        }
    }

    pub fn first_origin_ms(&self) -> i64 {
        self.quakes[0].origin_ms
    }

    fn last(&self) -> &Quake {
        &self.quakes[self.quakes.len() - 1]
    }

    /// 最後の報が届いた時刻
    pub fn last_recv_ms(&self) -> u64 {
        self.quakes.iter().map(|q| q.last_recv_ms).max().unwrap_or(0)
    }

    /// 動画の範囲 (報の received_at_ms)
    pub fn range_ms(&self) -> (u64, u64) {
        (
            (self.first_origin_ms() - FROM_LEAD_MS).max(0) as u64,
            self.last_recv_ms() + TO_TAIL_MS,
        )
    }

    /// まとまりの名前 (最初の地震の発生時刻。JST の `YYYYMMDD-HHMMSS`)
    pub fn id(&self) -> String {
        let t = jst::format(self.first_origin_ms());
        t.chars()
            .filter(|c| c.is_ascii_digit() || *c == ' ')
            .map(|c| if c == ' ' { '-' } else { c })
            .collect()
    }

    /// 閉じているか (動画にしてよいか)
    pub fn is_closed(&self, now_ms: u64, rules: &Rules) -> bool {
        self.capped || now_ms >= self.last_recv_ms() + rules.quiet_ms
    }
}

/// 前の地震と近く (距離・時間) 続いていれば、同じまとまりにする。震源が分からないときは時間だけで決める
fn joins(g: &Group, q: &Quake, rules: &Rules) -> bool {
    let last = g.last();
    let near = match (last.lat, last.lon, q.lat, q.lon) {
        (Some(la), Some(lo), Some(lb), Some(ob)) => distance_km(la, lo, lb, ob) <= rules.join_km,
        _ => true,
    };
    near && q.origin_ms - last.origin_ms <= rules.join_ms
}

/// 発生の早い順の地震を、まとまりに分ける。まとまりの長さ (最初の地震から) が上限を超える地震は、次のまとまりにする
pub fn groups(quakes: &[Quake], rules: &Rules) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for q in quakes {
        // 新しいまとまりから探す
        let target = out.iter_mut().rev().find(|g| joins(g, q, rules));
        match target {
            Some(g) if q.origin_ms - g.first_origin_ms() <= rules.max_len_ms => g.quakes.push(q.clone()),
            Some(g) => {
                g.capped = true;
                out.push(Group::new(q.clone()));
            }
            None => out.push(Group::new(q.clone())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::super::testkit::*;
    use super::*;
    use crate::quake::{QuakeInfoType, Scale};

    const T: u64 = T0 as u64;
    const MIN: u64 = 60_000;

    fn rules() -> Rules {
        Rules::from(&WorkerConfig::default())
    }

    fn q(origin_ms: i64, at: Option<(f64, f64)>, scale: i32, last_recv_ms: u64) -> Quake {
        Quake {
            origin_ms,
            lat: at.map(|a| a.0),
            lon: at.map(|a| a.1),
            max_scale: scale,
            warning: false,
            name: "x".into(),
            last_recv_ms,
        }
    }

    #[test]
    fn a_quake_info_and_its_eew_are_one_quake_counted_by_the_observed_scale() {
        let events = [
            eew("E", 1, T + 5_000, T0, false, Scale::S4, TOKYO),
            quake(
                T + 90_000,
                QuakeInfoType::DetailScale,
                T0 - 20_000,
                Scale::S3,
                Some(TOKYO),
            ),
        ];
        let got = quakes(&events, &rules());
        assert_eq!(got.len(), 1);
        // 予測 4 ではなく、観測の 3
        assert_eq!((got[0].max_scale, got[0].last_recv_ms), (30, T + 90_000));
    }

    #[test]
    fn the_criteria_are_the_scale_or_a_warning() {
        let small = eew("E", 1, T, T0, false, Scale::S2, TOKYO);
        assert!(quakes(std::slice::from_ref(&small), &rules()).is_empty());
        // 警報は震度が小さくても入る。warning = false にすれば入らない
        let warn = eew("E", 1, T, T0, true, Scale::S2, TOKYO);
        assert_eq!(quakes(std::slice::from_ref(&warn), &rules()).len(), 1);
        assert!(quakes(
            &[warn],
            &Rules {
                warning: false,
                ..rules()
            }
        )
        .is_empty());
        // 震度 3 ちょうどは入る
        let three = quake(T, QuakeInfoType::DetailScale, T0, Scale::S3, Some(TOKYO));
        assert_eq!(quakes(&[three], &rules()).len(), 1);
    }

    #[test]
    fn training_and_cancelled_reports_are_not_quakes() {
        let t = as_test(eew("E", 1, T, T0, true, Scale::S5_LOWER, TOKYO));
        assert!(quakes(&[t], &rules()).is_empty());
        let c = as_cancelled(eew("E", 1, T, T0, true, Scale::S5_LOWER, TOKYO));
        assert!(quakes(&[c], &rules()).is_empty());
    }

    #[test]
    fn a_quake_whose_eew_came_alone_is_still_counted_and_sorted_by_origin() {
        let events = [
            quake(
                T + 10 * MIN,
                QuakeInfoType::DetailScale,
                T0 + 600_000,
                Scale::S4,
                Some(OSAKA),
            ),
            eew("E", 1, T, T0, false, Scale::S3, TOKYO),
        ];
        let got = quakes(&events, &rules());
        assert_eq!(got.iter().map(|x| x.origin_ms).collect::<Vec<_>>(), [T0, T0 + 600_000]);
    }

    #[test]
    fn near_quakes_within_thirty_minutes_chain_and_far_or_late_ones_do_not() {
        let near = (TOKYO.0 + 0.2, TOKYO.1);
        let list = [
            q(T0, Some(TOKYO), 30, 0),
            // 25 分後・約 22km: 前の地震に続く
            q(T0 + 25 * MIN as i64, Some(near), 30, 0),
            // その 25 分後 (最初からは 50 分) でも、前の地震から 25 分なので続く (最初からではなく前から数える)
            q(T0 + 50 * MIN as i64, Some(TOKYO), 30, 0),
            // 大阪は遠い
            q(T0 + 51 * MIN as i64, Some(OSAKA), 30, 0),
            // 31 分あいた
            q(T0 + 82 * MIN as i64, Some(TOKYO), 30, 0),
        ];
        let g = groups(&list, &rules());
        assert_eq!(g.iter().map(|x| x.quakes.len()).collect::<Vec<_>>(), [3, 1, 1]);
        assert!(g.iter().all(|x| !x.capped));
    }

    #[test]
    fn a_quake_with_no_epicenter_chains_by_time_only() {
        let list = [q(T0, Some(TOKYO), 30, 0), q(T0 + 10 * MIN as i64, None, 30, 0)];
        assert_eq!(groups(&list, &rules()).len(), 1);
        let list = [q(T0, None, 30, 0), q(T0 + 31 * MIN as i64, None, 30, 0)];
        assert_eq!(groups(&list, &rules()).len(), 2);
    }

    #[test]
    fn a_group_that_would_pass_three_hours_is_split_and_closes_at_once() {
        // 20 分おきに 3 時間 20 分続く地震 (どれも同じ場所)
        let list: Vec<Quake> = (0..=10)
            .map(|i| q(T0 + i * 20 * MIN as i64, Some(TOKYO), 30, T + i as u64 * 20 * MIN))
            .collect();
        let g = groups(&list, &rules());
        assert_eq!(g.iter().map(|x| x.quakes.len()).collect::<Vec<_>>(), [10, 1]);
        // 最初から 3 時間ちょうど (9 個目) までは入る。超える 10 個目から次
        assert!(g[0].capped && !g[1].capped);
        // 上限で切れたまとまりは、静かになるのを待たずに閉じる。次のは待つ
        let last = g[1].last_recv_ms();
        assert!(g[0].is_closed(T, &rules()));
        assert!(!g[1].is_closed(last + 59 * MIN, &rules()));
        assert!(g[1].is_closed(last + 60 * MIN, &rules()));
    }

    #[test]
    fn a_group_closes_after_sixty_quiet_minutes_from_the_last_report() {
        let g = groups(
            &[
                q(T0, Some(TOKYO), 30, T + 5 * MIN),
                q(T0 + 10 * MIN as i64, Some(TOKYO), 30, T + 12 * MIN),
            ],
            &rules(),
        );
        let g = &g[0];
        assert_eq!(g.last_recv_ms(), T + 12 * MIN);
        assert!(!g.is_closed(T + 72 * MIN - 1, &rules()));
        assert!(g.is_closed(T + 72 * MIN, &rules()));
    }

    #[test]
    fn the_id_and_range_come_from_the_first_quake_and_the_last_report() {
        let g = &groups(&[q(T0, Some(TOKYO), 30, T + 5 * MIN)], &rules())[0];
        // 2026/09/21 23:13:20 JST (T0 = 1_790_000_000_000)
        assert_eq!(g.id(), "20260921-231320");
        let (from, to) = g.range_ms();
        assert_eq!((from, to), (T - 120_000, T + 5 * MIN + 600_000));
    }
}
