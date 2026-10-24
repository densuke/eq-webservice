//! 記録から描き直すとき (再現動画) だけに渡す情報。ライブの配信は渡さない (None)。
//! - 最初の円 (発生時刻から広がる円) に合わせて、のちに届く各地の震度の観測点を灯す
//!   (観測点の値が届いた時刻に円を描き直すと、その時刻にもう一度揺れたように見えるため)
//! - 最初の報が届くまで、右の欄にあらまし (この動画で再現する地震) を出す

use crate::quake::{jst, Event, EventBody, ObservationPoint, QuakeInfoType, Scale};

use super::eew::WAVE_MAX_MS;
use super::hindsight::Hindsight;
use super::model::{event_place, same_quake, tsunami_text, Place};
use super::spread::Spread;

pub struct ReplayInfo<'a> {
    pub foresight: Option<Foresight<'a>>,
    /// 最初の報が届くまで右の欄に出す行 (1 行目は見出し)
    pub intro: &'a [String],
}

/// のちに届く各地の震度の観測点と、その地震の震央・深さ・発生時刻
#[derive(Clone, Copy)]
pub struct Foresight<'a> {
    pub lat: f64,
    pub lon: f64,
    pub depth_km: f64,
    pub origin_ms: i64,
    pub points: &'a [ObservationPoint],
}

impl Foresight<'_> {
    /// 発生時刻から広がる円。S 波が届いた点から灯す (spread::lit。円が過ぎたあとも灯ったまま)
    pub fn spread(&self) -> Spread {
        let start_ms = self.origin_ms.max(0) as u64;
        Spread {
            lat: self.lat,
            lon: self.lon,
            depth_km: self.depth_km,
            start_ms,
            end_ms: start_ms + WAVE_MAX_MS as u64,
        }
    }

    /// 最初の円が広がっている間か (案内を出す)
    pub fn waving(&self, now_ms: u64) -> bool {
        (0..=WAVE_MAX_MS).contains(&(now_ms as i64 - self.origin_ms))
    }
}

/// のちに分かった震源 h と同じ地震の、観測点 (区域ではない点) を含む最後の各地の震度。無ければ None
pub fn foresight_of<'a>(events: &'a [Event], h: &Hindsight) -> Option<Foresight<'a>> {
    let me = Place {
        origin_ms: Some(h.origin_ms),
        lat: Some(h.lat),
        lon: Some(h.lon),
    };
    let points = events
        .iter()
        .filter(|e| event_place(e).is_some_and(|p| same_quake(&p, &me)))
        .filter_map(|e| match &e.body {
            EventBody::Quake(q) if q.info_type == QuakeInfoType::DetailScale && q.points.iter().any(|p| !p.is_area) => {
                Some((e.received_at_ms, &q.points))
            }
            _ => None,
        })
        .max_by_key(|x| x.0)?
        .1;
    Some(Foresight {
        lat: h.lat,
        lon: h.lon,
        depth_km: h.depth_km,
        origin_ms: h.origin_ms,
        points,
    })
}

/// あらまし: 見出し・再現であることの断り・最終的な震源と震度・これから届く報の予定。
/// 予定は、緊急地震速報を 1 行にまとめ、地震情報は報ごとに 1 行 (届いた時刻)
pub fn intro_lines(events: &[Event], h: &Hindsight) -> Vec<String> {
    let me = Place {
        origin_ms: Some(h.origin_ms),
        lat: Some(h.lat),
        lon: Some(h.lon),
    };
    let mine: Vec<&Event> = events
        .iter()
        .filter(|e| event_place(e).is_some_and(|p| same_quake(&p, &me)))
        .collect();
    let hms = |ms: u64| jst::format(ms as i64).get(11..).unwrap_or("").to_string();
    let quakes: Vec<_> = mine
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Quake(q) => Some((e.received_at_ms, q)),
            _ => None,
        })
        .collect();
    let located = quakes
        .iter()
        .rev()
        .find_map(|(_, q)| q.hypocenter.as_ref().filter(|h| !h.name.is_empty()));
    let mut source = located.map_or("調査中".to_string(), |h| h.name.clone());
    if let Some(m) = located.and_then(|h| h.magnitude).filter(|m| *m > 0.0) {
        source += &format!(" / M{m:.1}");
    }
    if let Some(d) = located.and_then(|h| h.depth_km).filter(|d| *d >= 0) {
        source += &if d == 0 {
            " / ごく浅い".to_string()
        } else {
            format!(" / 深さ{d}km")
        };
    }
    let max = quakes.iter().map(|(_, q)| q.max_scale).max().unwrap_or(Scale::UNKNOWN);
    let tsunami = quakes
        .iter()
        .rev()
        .map(|(_, q)| tsunami_text(&q.domestic_tsunami))
        .find(|t| *t != "—");
    let mut lines = vec![
        "この動画で再現する地震".to_string(),
        "気象庁の発表と観測データに基づく再現です。".to_string(),
        format!("発生 {}", jst::format(h.origin_ms)),
        format!("震源 {source}"),
    ];
    if max.is_known() {
        lines.push(format!("最大震度 {}", max.label()));
    }
    if let Some(t) = tsunami {
        lines.push(format!("津波 {t}"));
    }
    lines.push("これから届く報".to_string());
    let eews: Vec<_> = mine
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Eew(x) if !x.test => Some((e.received_at_ms, x)),
            _ => None,
        })
        .collect();
    let mut schedule: Vec<(u64, String)> = Vec::new();
    if let (Some(first), Some(last)) = (eews.first(), eews.last()) {
        let kind = if eews.iter().any(|(_, x)| x.warning) {
            "警報"
        } else {
            "予報"
        };
        let mut serials: Vec<&str> = eews.iter().map(|(_, x)| x.serial.as_str()).collect();
        serials.sort_unstable();
        serials.dedup();
        let range = if first.0 == last.0 {
            hms(first.0)
        } else {
            format!("{}〜{}", hms(first.0), hms(last.0))
        };
        let cancelled = if eews.iter().any(|(_, x)| x.cancelled) {
            " (取消あり)"
        } else {
            ""
        };
        schedule.push((
            first.0,
            format!("緊急地震速報 ({kind}) {} 報 {range}{cancelled}", serials.len()),
        ));
    }
    schedule.extend(
        quakes
            .iter()
            .map(|(at, q)| (*at, format!("{} {}", q.info_type.label(), hms(*at)))),
    );
    schedule.sort_by_key(|x| x.0);
    lines.extend(schedule.into_iter().map(|x| x.1));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::{Hypocenter, Quake, Scale};

    const T0: i64 = 1_792_771_668_000; // 2026-10-24 01:07:48 JST

    fn quake(t: QuakeInfoType, at: u64, points: Vec<ObservationPoint>) -> Event {
        Event {
            id: format!("q{at}"),
            source: "p2pquake".into(),
            received_at_ms: at,
            body: EventBody::Quake(Quake {
                info_type: t,
                origin_time: String::new(),
                origin_time_ms: Some(T0 - 48_000),
                issued_at: String::new(),
                hypocenter: Some(Hypocenter {
                    name: "岩手県沖".into(),
                    latitude: Some(39.8),
                    longitude: Some(142.3),
                    depth_km: Some(20),
                    magnitude: Some(5.0),
                }),
                max_scale: Scale::S3,
                domestic_tsunami: "None".into(),
                points,
                pref_max: vec![],
                comment: String::new(),
            }),
        }
    }

    fn point(addr: &str, is_area: bool) -> ObservationPoint {
        ObservationPoint {
            pref: "岩手県".into(),
            addr: addr.into(),
            is_area,
            scale: Scale::S3,
            station: None,
        }
    }

    #[test]
    fn the_foresight_is_the_last_detail_report_with_stations_of_the_same_quake() {
        let h = Hindsight {
            lat: 39.8,
            lon: 142.3,
            depth_km: 20.0,
            origin_ms: T0,
        };
        let at = T0 as u64;
        let events = vec![
            quake(
                QuakeInfoType::ScalePrompt,
                at + 115_000,
                vec![point("岩手県沿岸南部", true)],
            ),
            quake(
                QuakeInfoType::DetailScale,
                at + 270_000,
                vec![point("宮古市鍬ヶ崎", false)],
            ),
        ];
        let f = foresight_of(&events, &h).unwrap();
        assert_eq!(f.points[0].addr, "宮古市鍬ヶ崎");
        // 震度速報 (区域だけ) しか無ければ無い
        assert!(foresight_of(&events[..1], &h).is_none());
        // 円は発生時刻から。広がっている間だけ案内を出す
        assert_eq!(f.spread().start_ms, at);
        assert!(f.waving(at + 1_000) && !f.waving(at - 1) && !f.waving(at + WAVE_MAX_MS as u64 + 1));
    }

    fn eew(serial: &str, at: u64) -> Event {
        Event {
            id: format!("e{serial}"),
            source: "wolfx".into(),
            received_at_ms: at,
            body: EventBody::Eew(crate::quake::Eew {
                event_id: "20261024010748".into(),
                serial: serial.into(),
                cancelled: false,
                test: false,
                warning: false,
                issued_at: String::new(),
                origin_time: None,
                origin_time_ms: Some(T0),
                hypocenter: Some(Hypocenter {
                    name: "岩手県沖".into(),
                    latitude: Some(39.8),
                    longitude: Some(142.3),
                    depth_km: Some(30),
                    magnitude: Some(4.9),
                }),
                areas: vec![],
                pref_max: vec![],
                max_scale: Scale::S3,
            }),
        }
    }

    #[test]
    fn the_intro_says_it_is_a_replay_and_lists_the_reports_to_come() {
        // 2026-10-24 01:07 岩手県沖: 緊急地震速報 (予報) 11 報 → 震度速報 → 震源に関する情報 → 各地の震度
        let h = Hindsight {
            lat: 39.8,
            lon: 142.3,
            depth_km: 20.0,
            origin_ms: T0,
        };
        let at = T0 as u64;
        let mut events: Vec<Event> = (1..=11).map(|n| eew(&n.to_string(), at + 13_000 + n * 5_000)).collect();
        events.push(quake(
            QuakeInfoType::ScalePrompt,
            at + 115_000,
            vec![point("岩手県沿岸南部", true)],
        ));
        events.push(quake(QuakeInfoType::Destination, at + 204_000, vec![]));
        events.push(quake(
            QuakeInfoType::DetailScale,
            at + 270_000,
            vec![point("宮古市鍬ヶ崎", false)],
        ));
        assert_eq!(
            intro_lines(&events, &h),
            vec![
                "この動画で再現する地震",
                "気象庁の発表と観測データに基づく再現です。",
                "発生 2026/10/24 01:07:48",
                "震源 岩手県沖 / M5.0 / 深さ20km",
                "最大震度 3",
                "津波 この地震による津波の心配はありません",
                "これから届く報",
                "緊急地震速報 (予報) 11 報 01:08:06〜01:08:56",
                "震度速報 01:09:43",
                "震源に関する情報 01:11:12",
                "各地の震度に関する情報 01:12:18",
            ]
        );
    }
}
