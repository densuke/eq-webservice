//! docs/tts.md を参照

use crate::quake::model::{Event, EventBody, Hypocenter, QuakeInfoType, Tsunami, TsunamiGrade};
use crate::quake::scale::Scale;

// 固定句。segments と fixed_segments で共有する。
const EEW_CANCELLED: &str = "先ほどの緊急地震速報は取り消されました。";
const EEW_WARNING: &str = "緊急地震速報。";
const EEW_FORECAST: &str = "緊急地震速報、予報。";
const EEW_STRONG: &str = "強い揺れに警戒してください。";
const PLACE_UNKNOWN: &str = "震源は調査中です。";
const SCALE_PROMPT: &str = "震度速報。";
const DESTINATION: &str = "震源に関する情報。";
const SCALE_AND_DEST: &str = "地震情報。";
const FOREIGN: &str = "遠地地震に関する情報。";
const TSUNAMI_CANCELLED: &str = "津波予報は解除されました。";
const TSUNAMI_MORE: &str = "ほかの地域。";
const EVACUATE: &str = "海岸から離れ、高台に避難してください。";
const WATCH_AREA_LIMIT: usize = 10;

/// 震度の種類 (不明を除く)。46 は 5弱以上と推定。
const SCALES: [Scale; 10] = [
    Scale::S1,
    Scale::S2,
    Scale::S3,
    Scale::S4,
    Scale::S5_LOWER,
    Scale(46),
    Scale::S5_UPPER,
    Scale::S6_LOWER,
    Scale::S6_UPPER,
    Scale::S7,
];

/// 津波の有無ごとの文 (地名を含まない)。
const DOMESTIC_TSUNAMI: [(&[&str], &str); 4] = [
    (&["None"], "この地震による津波の心配はありません。"),
    (&["Checking"], "津波の有無は現在調査中です。"),
    (
        &["NonEffective"],
        "若干の海面変動があるかもしれませんが、被害の心配はありません。",
    ),
    (&["Watch", "Warning"], "津波警報などが発表されています。"),
];

/// 津波の grade を重い順に。
const GRADES: [TsunamiGrade; 3] = [TsunamiGrade::MajorWarning, TsunamiGrade::Warning, TsunamiGrade::Watch];

/// 読み上げ用の震度。括弧は読まれないので 46 だけ言い換える。
fn scale_text(s: Scale) -> &'static str {
    if s == Scale(46) {
        "5弱以上と推定"
    } else {
        s.label()
    }
}

fn max_sentence(s: Scale) -> String {
    format!("最大震度{}。", scale_text(s))
}

fn expected_sentence(s: Scale) -> String {
    format!("予想される最大震度は{}。", scale_text(s))
}

fn place(h: Option<&Hypocenter>) -> String {
    match h {
        Some(h) if !h.name.is_empty() => format!("震源は{}。", h.name),
        _ => PLACE_UNKNOWN.to_string(),
    }
}

fn mag(h: Option<&Hypocenter>) -> Option<String> {
    h.and_then(|h| h.magnitude)
        .filter(|m| *m > 0.0)
        .map(|m| format!("マグニチュード{m:.1}。"))
}

fn tsunami_note(kind: &str) -> Option<String> {
    DOMESTIC_TSUNAMI
        .iter()
        .find(|(keys, _)| keys.contains(&kind))
        .map(|(_, s)| s.to_string())
}

fn tsunami_segments(t: &Tsunami) -> Vec<String> {
    if t.cancelled {
        return vec![TSUNAMI_CANCELLED.to_string()];
    }
    let mut out = Vec::new();
    for g in GRADES {
        let names: Vec<&str> = t
            .areas
            .iter()
            .filter(|a| a.grade == g)
            .map(|a| a.name.as_str())
            .collect();
        if names.is_empty() {
            continue;
        }
        out.push(format!("{}を発表しました。", g.label()));
        let shown = if g == TsunamiGrade::Watch {
            WATCH_AREA_LIMIT
        } else {
            names.len()
        };
        out.extend(names.iter().take(shown).map(|n| format!("{n}。")));
        if names.len() > shown {
            out.push(TSUNAMI_MORE.to_string());
        }
    }
    if t.areas
        .iter()
        .any(|a| matches!(a.grade, TsunamiGrade::MajorWarning | TsunamiGrade::Warning))
    {
        out.push(EVACUATE.to_string());
    }
    out
}

/// イベントを読み上げ部品 (1 部品 = 1 回の合成) に分ける。空は「読まない」。
pub fn segments(ev: &Event) -> Vec<String> {
    match &ev.body {
        EventBody::Eew(e) => {
            if e.test && ev.source != "replay" && ev.source != "demo" {
                return vec![];
            }
            if e.cancelled {
                return vec![EEW_CANCELLED.to_string()];
            }
            let h = e.hypocenter.as_ref();
            let mut out = vec![
                (if e.warning { EEW_WARNING } else { EEW_FORECAST }).to_string(),
                place(h),
            ];
            if e.max_scale.is_known() {
                out.push(expected_sentence(e.max_scale));
            }
            if e.warning {
                out.push(EEW_STRONG.to_string());
            }
            out
        }
        EventBody::Quake(q) => {
            let h = q.hypocenter.as_ref();
            let max = q.max_scale.is_known().then(|| max_sentence(q.max_scale));
            let tsunami = tsunami_note(&q.domestic_tsunami);
            let (head, parts): (&str, Vec<Option<String>>) = match q.info_type {
                QuakeInfoType::ScalePrompt => {
                    let pref = q
                        .pref_max
                        .first()
                        .map(|p| format!("{}などで揺れを観測しました。", p.pref));
                    (SCALE_PROMPT, vec![max, pref])
                }
                QuakeInfoType::Destination => (DESTINATION, vec![Some(place(h)), mag(h), tsunami]),
                QuakeInfoType::ScaleAndDestination | QuakeInfoType::DetailScale => {
                    (SCALE_AND_DEST, vec![Some(place(h)), max, mag(h), tsunami])
                }
                QuakeInfoType::Foreign => (FOREIGN, vec![Some(place(h)), mag(h), tsunami]),
                QuakeInfoType::Other => return vec![],
            };
            std::iter::once(head.to_string())
                .chain(parts.into_iter().flatten())
                .collect()
        }
        EventBody::Tsunami(t) => tsunami_segments(t),
        EventBody::EewDetection(_) | EventBody::Userquake(_) => vec![],
    }
}

/// prewarm 用に、地名を含まない全部品を返す。
pub fn fixed_segments() -> Vec<String> {
    let fixed = [
        EEW_CANCELLED,
        EEW_WARNING,
        EEW_FORECAST,
        EEW_STRONG,
        PLACE_UNKNOWN,
        SCALE_PROMPT,
        DESTINATION,
        SCALE_AND_DEST,
        FOREIGN,
        TSUNAMI_CANCELLED,
        TSUNAMI_MORE,
        EVACUATE,
    ]
    .map(String::from);
    let scales = SCALES.iter().flat_map(|s| [max_sentence(*s), expected_sentence(*s)]);
    // 整数から作って浮動小数のずれを避ける
    let mags = (1..=99).map(|i| format!("マグニチュード{}.{}。", i / 10, i % 10));
    let tsunami = DOMESTIC_TSUNAMI.iter().map(|(_, s)| s.to_string());
    let grades = GRADES.iter().map(|g| format!("{}を発表しました。", g.label()));
    let mut out: Vec<String> = Vec::new();
    for s in fixed.into_iter().chain(scales).chain(mags).chain(tsunami).chain(grades) {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::model::{
        Eew, EewDetection, EventBody, Hypocenter, PrefScale, Quake, QuakeInfoType, Tsunami, TsunamiArea, TsunamiGrade,
        Userquake,
    };
    use crate::quake::scale::Scale;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn ev(source: &str, body: EventBody) -> Event {
        Event {
            id: "x".into(),
            source: source.into(),
            received_at_ms: 0,
            body,
        }
    }

    fn hypo(name: &str, mag: Option<f64>) -> Option<Hypocenter> {
        Some(Hypocenter {
            name: name.into(),
            latitude: None,
            longitude: None,
            depth_km: None,
            magnitude: mag,
        })
    }

    fn eew(warning: bool, cancelled: bool, test: bool, scale: Scale) -> Eew {
        Eew {
            event_id: "e".into(),
            serial: "1".into(),
            cancelled,
            test,
            warning,
            issued_at: String::new(),
            origin_time: None,
            origin_time_ms: None,
            hypocenter: hypo("能登半島沖", None),
            areas: vec![],
            pref_max: vec![],
            max_scale: scale,
        }
    }

    fn quake(t: QuakeInfoType, name: &str, mag: Option<f64>, scale: Scale, tsu: &str) -> Quake {
        Quake {
            info_type: t,
            origin_time: String::new(),
            origin_time_ms: None,
            issued_at: String::new(),
            hypocenter: hypo(name, mag),
            max_scale: scale,
            domestic_tsunami: tsu.into(),
            points: vec![],
            pref_max: vec![],
            comment: String::new(),
        }
    }

    fn quake_ev(q: Quake) -> Event {
        ev("p2pquake", EventBody::Quake(q))
    }

    fn area(name: &str, grade: TsunamiGrade) -> TsunamiArea {
        TsunamiArea {
            name: name.into(),
            grade,
            immediate: false,
            first_height: None,
            max_height: None,
        }
    }

    fn tsunami_ev(cancelled: bool, areas: Vec<TsunamiArea>) -> Event {
        ev(
            "p2pquake",
            EventBody::Tsunami(Tsunami {
                cancelled,
                issued_at: String::new(),
                areas,
            }),
        )
    }

    #[test]
    fn eew_warning() {
        let e = ev("p2pquake", EventBody::Eew(eew(true, false, false, Scale(55))));
        assert_eq!(
            segments(&e),
            s(&[
                "緊急地震速報。",
                "震源は能登半島沖。",
                "予想される最大震度は6弱。",
                "強い揺れに警戒してください。"
            ])
        );
    }

    #[test]
    fn eew_forecast_unknown_scale_has_no_scale_segment() {
        let e = ev("p2pquake", EventBody::Eew(eew(false, false, false, Scale::UNKNOWN)));
        assert_eq!(segments(&e), s(&["緊急地震速報、予報。", "震源は能登半島沖。"]));
    }

    #[test]
    fn eew_cancelled() {
        let e = ev("p2pquake", EventBody::Eew(eew(true, true, false, Scale(55))));
        assert_eq!(segments(&e), s(&["先ほどの緊急地震速報は取り消されました。"]));
    }

    #[test]
    fn eew_test_is_silent_except_replay() {
        let body = EventBody::Eew(eew(true, false, true, Scale(55)));
        assert!(segments(&ev("p2pquake", body.clone())).is_empty());
        assert!(!segments(&ev("replay", body.clone())).is_empty());
        assert!(!segments(&ev("demo", body)).is_empty());
    }

    #[test]
    fn scale_prompt_with_and_without_pref() {
        let mut q = quake(QuakeInfoType::ScalePrompt, "", None, Scale(50), "Unknown");
        assert_eq!(segments(&quake_ev(q.clone())), s(&["震度速報。", "最大震度5強。"]));
        q.pref_max = vec![PrefScale {
            pref: "石川県".into(),
            scale: Scale(50),
        }];
        assert_eq!(
            segments(&quake_ev(q)),
            s(&["震度速報。", "最大震度5強。", "石川県などで揺れを観測しました。"])
        );
    }

    #[test]
    fn destination_without_magnitude_has_tsunami() {
        let q = quake(QuakeInfoType::Destination, "能登半島沖", None, Scale::UNKNOWN, "None");
        assert_eq!(
            segments(&quake_ev(q)),
            s(&[
                "震源に関する情報。",
                "震源は能登半島沖。",
                "この地震による津波の心配はありません。"
            ])
        );
    }

    #[test]
    fn unknown_hypocenter_reads_investigating() {
        let q = quake(QuakeInfoType::Destination, "", None, Scale::UNKNOWN, "Unknown");
        assert!(segments(&quake_ev(q.clone())).contains(&"震源は調査中です。".to_string()));
        let mut q = q;
        q.hypocenter = None;
        assert!(segments(&quake_ev(q)).contains(&"震源は調査中です。".to_string()));
    }

    #[test]
    fn estimated_scale_has_no_parenthesis() {
        let q = quake(
            QuakeInfoType::ScaleAndDestination,
            "能登半島沖",
            None,
            Scale(46),
            "Unknown",
        );
        let seg = segments(&quake_ev(q));
        assert!(seg.contains(&"最大震度5弱以上と推定。".to_string()));
        assert!(seg.iter().all(|x| !x.contains('(')));
    }

    #[test]
    fn magnitude_formatting() {
        let mk = |m| {
            quake_ev(quake(
                QuakeInfoType::Destination,
                "能登半島沖",
                Some(m),
                Scale::UNKNOWN,
                "Unknown",
            ))
        };
        assert!(segments(&mk(6.5)).contains(&"マグニチュード6.5。".to_string()));
        assert!(segments(&mk(7.0)).contains(&"マグニチュード7.0。".to_string()));
    }

    #[test]
    fn domestic_tsunami_table() {
        let cases = [
            ("None", Some("この地震による津波の心配はありません。")),
            ("Checking", Some("津波の有無は現在調査中です。")),
            (
                "NonEffective",
                Some("若干の海面変動があるかもしれませんが、被害の心配はありません。"),
            ),
            ("Watch", Some("津波警報などが発表されています。")),
            ("Warning", Some("津波警報などが発表されています。")),
            ("Unknown", None),
        ];
        for (key, want) in cases {
            let q = quake(QuakeInfoType::Destination, "能登半島沖", None, Scale::UNKNOWN, key);
            let want: Vec<String> = ["震源に関する情報。", "震源は能登半島沖。"]
                .iter()
                .chain(want.iter())
                .map(|x| x.to_string())
                .collect();
            assert_eq!(segments(&quake_ev(q)), want, "domestic_tsunami={key}");
        }
    }

    #[test]
    fn tsunami_watch_truncates_to_ten() {
        let areas: Vec<_> = (0..12)
            .map(|i| area(&format!("区域{i}"), TsunamiGrade::Watch))
            .collect();
        let mut want = vec!["津波注意報を発表しました。".to_string()];
        want.extend((0..10).map(|i| format!("区域{i}。")));
        want.push("ほかの地域。".into());
        assert_eq!(segments(&tsunami_ev(false, areas)), want);
    }

    #[test]
    fn tsunami_orders_by_grade_and_ends_with_evacuation() {
        let areas = vec![
            area("注意区域", TsunamiGrade::Watch),
            area("大津波区域", TsunamiGrade::MajorWarning),
        ];
        assert_eq!(
            segments(&tsunami_ev(false, areas)),
            s(&[
                "大津波警報を発表しました。",
                "大津波区域。",
                "津波注意報を発表しました。",
                "注意区域。",
                "海岸から離れ、高台に避難してください。"
            ])
        );
    }

    #[test]
    fn tsunami_cancelled_and_empty() {
        assert_eq!(segments(&tsunami_ev(true, vec![])), s(&["津波予報は解除されました。"]));
        assert!(segments(&tsunami_ev(false, vec![])).is_empty());
    }

    #[test]
    fn silent_events() {
        let q = quake(QuakeInfoType::Other, "能登半島沖", Some(6.0), Scale(55), "None");
        assert!(segments(&quake_ev(q)).is_empty());
        let u = Userquake {
            started_at: String::new(),
            updated_at: String::new(),
            count: 1,
            confidence: 0.5,
            areas: vec![],
        };
        assert!(segments(&ev("p2pquake", EventBody::Userquake(u))).is_empty());
        let d = EewDetection {
            detection_type: "Full".into(),
        };
        assert!(segments(&ev("p2pquake", EventBody::EewDetection(d))).is_empty());
    }

    #[test]
    fn fixed_segments_catalog() {
        let f = fixed_segments();
        let mut uniq = f.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), f.len(), "重複がある");
        assert_eq!(f.iter().filter(|x| x.starts_with("マグニチュード")).count(), 99);
        for want in [
            "マグニチュード0.1。",
            "マグニチュード9.9。",
            "緊急地震速報。",
            "最大震度5弱以上と推定。",
        ] {
            assert!(f.contains(&want.to_string()), "{want}");
        }
    }
}
