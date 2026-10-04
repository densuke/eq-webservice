//! docs/tts.md を参照

use crate::quake::area::PREFS;
use crate::quake::model::{Event, EventBody, Hypocenter, QuakeInfoType, Tsunami, TsunamiGrade, Userquake};
use crate::quake::scale::Scale;
use crate::quake::userquake;

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
const USERQUAKE_REPORTED: &str = "揺れを感じたという報告が集まっています。";

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

/// 読み上げる震度か。表に無い値 (旧い震度階級など) は「震度不明」と読んでしまうので読まない
fn readable(s: Scale) -> bool {
    SCALES.contains(&s)
}

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

/// 地震感知情報の文。信頼できる評価でなければ空。県名の部品は userquake_pref_segments と同じ形。
/// 「揺れを感じたという報告」とだけ言う (機器の検知ではない)。docs/tts.md S12
fn userquake_segments(u: &Userquake) -> Vec<String> {
    if !userquake::credible(u) {
        return vec![];
    }
    let prefs = userquake::credible_prefs(u);
    let shown = &prefs[..prefs.len().min(userquake::MAX_PREFS)];
    let more = prefs.len() > shown.len();
    let mut out: Vec<String> = shown
        .iter()
        .enumerate()
        .map(|(i, p)| match (i + 1 == shown.len(), more) {
            (false, _) => format!("{p}、"),
            (true, false) => format!("{p}で、"),
            (true, true) => format!("{p}などで、"),
        })
        .collect();
    out.push(USERQUAKE_REPORTED.to_string());
    out
}

/// prewarm 用に、地震感知情報で読む県名の部品 (「{p}、」「{p}で、」「{p}などで、」) を返す。
pub fn userquake_pref_segments() -> Vec<String> {
    PREFS
        .iter()
        .flat_map(|p| [format!("{p}、"), format!("{p}で、"), format!("{p}などで、")])
        .collect()
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
            if readable(e.max_scale) {
                out.push(expected_sentence(e.max_scale));
            }
            if e.warning {
                out.push(EEW_STRONG.to_string());
            }
            out
        }
        EventBody::Quake(q) => {
            let h = q.hypocenter.as_ref();
            let max = readable(q.max_scale).then(|| max_sentence(q.max_scale));
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
        EventBody::Userquake(u) => userquake_segments(u),
        EventBody::EewDetection(_) => vec![],
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
        USERQUAKE_REPORTED,
    ]
    .map(String::from);
    let scales = SCALES.iter().flat_map(|s| [max_sentence(*s), expected_sentence(*s)]);
    // 整数から作って浮動小数のずれを避ける
    let mags = (1..=99).map(|i| format!("マグニチュード{}.{}。", i / 10, i % 10));
    let tsunami = DOMESTIC_TSUNAMI.iter().map(|(_, s)| s.to_string());
    let grades = GRADES.iter().map(|g| issued_sentence(*g));
    let follow = [FOLLOW_UP.to_string()].into_iter();
    let follow_scales = SCALES.iter().flat_map(|s| [raised_sentence(*s), observed_sentence(*s)]);
    let mag_updates = (1..=99).map(|i| mag_update_sentence(&format!("{}.{}", i / 10, i % 10)));
    let switches = DOWNGRADES.iter().map(|(o, n)| switch_sentence(*o, *n));
    let released = GRADES.iter().map(|g| released_sentence(*g));
    let prefs = crate::quake::area::PREFS
        .iter()
        .flat_map(|p| SCALES.iter().filter(|s| **s >= PREF_MIN).map(|s| pref_sentence(p, *s)));
    let follows = follow
        .chain(follow_scales)
        .chain(mag_updates)
        .chain(switches)
        .chain(released)
        .chain(prefs);
    let mut out: Vec<String> = Vec::new();
    for s in fixed
        .into_iter()
        .chain(scales)
        .chain(mags)
        .chain(tsunami)
        .chain(grades)
        .chain(follows)
    {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

const FOLLOW_UP: &str = "続報。";
const PREF_LIST_LIMIT: usize = 3;
/// 都道府県の個別読み上げは 5弱 から。
const PREF_MIN: Scale = Scale(45);
/// 格下げの組 (旧, 新)。
const DOWNGRADES: [(TsunamiGrade, TsunamiGrade); 3] = [
    (TsunamiGrade::MajorWarning, TsunamiGrade::Warning),
    (TsunamiGrade::MajorWarning, TsunamiGrade::Watch),
    (TsunamiGrade::Warning, TsunamiGrade::Watch),
];

fn raised_sentence(s: Scale) -> String {
    format!("予想される最大震度は{}に引き上げられました。", scale_text(s))
}

fn observed_sentence(s: Scale) -> String {
    format!("最大震度{}を観測しました。", scale_text(s))
}

fn mag_update_sentence(m: &str) -> String {
    format!("マグニチュードは{m}に更新されました。")
}

fn pref_sentence(pref: &str, s: Scale) -> String {
    format!("{pref}で震度{}を観測しました。", scale_text(s))
}

fn switch_sentence(old: TsunamiGrade, new: TsunamiGrade) -> String {
    format!("{}は{}に切り替えられました。", old.label(), new.label())
}

fn released_sentence(old: TsunamiGrade) -> String {
    format!("{}は解除されました。", old.label())
}

fn issued_sentence(g: TsunamiGrade) -> String {
    format!("{}を発表しました。", g.label())
}

fn hypo_name(h: &Option<Hypocenter>) -> Option<&str> {
    h.as_ref().map(|h| h.name.as_str()).filter(|n| !n.is_empty())
}

fn hypo_mag(h: &Option<Hypocenter>) -> Option<f64> {
    h.as_ref().and_then(|h| h.magnitude).filter(|m| *m > 0.0)
}

fn announce_eew(ev: &Event, priors: &[&Event]) -> Vec<String> {
    let EventBody::Eew(e) = &ev.body else { return vec![] };
    if e.cancelled {
        return vec![EEW_CANCELLED.to_string()];
    }
    if e.test && ev.source != "replay" && ev.source != "demo" {
        return vec![];
    }
    let prior: Vec<_> = priors
        .iter()
        .filter_map(|p| match &p.body {
            EventBody::Eew(pe) => Some(pe),
            _ => None,
        })
        .collect();
    if !prior.iter().any(|p| p.warning) && e.warning {
        return segments(ev);
    }
    let prev_max = prior.iter().map(|p| p.max_scale).max().unwrap_or(Scale::UNKNOWN);
    if readable(e.max_scale) && e.max_scale > prev_max {
        return vec![FOLLOW_UP.to_string(), raised_sentence(e.max_scale)];
    }
    vec![]
}

fn announce_quake(ev: &Event, priors: &[&Event]) -> Vec<String> {
    let EventBody::Quake(q) = &ev.body else { return vec![] };
    if q.info_type == QuakeInfoType::Other {
        return vec![];
    }
    let prior: Vec<_> = priors
        .iter()
        .filter_map(|p| match &p.body {
            EventBody::Quake(pq) => Some(pq),
            _ => None,
        })
        .collect();
    if prior.is_empty() {
        return segments(ev);
    }
    let mut out = Vec::new();
    // 震源名とマグニチュード
    let last_name = prior.iter().rev().find_map(|p| hypo_name(&p.hypocenter));
    let ev_mag = hypo_mag(&q.hypocenter);
    let prev_mag = prior.iter().rev().find_map(|p| hypo_mag(&p.hypocenter));
    match hypo_name(&q.hypocenter) {
        Some(name) if last_name != Some(name) => {
            out.push(format!("震源は{name}。"));
            out.extend(mag(q.hypocenter.as_ref()));
        }
        _ => match (ev_mag, prev_mag) {
            (Some(m), Some(pm)) if (m - pm).abs() >= 0.05 => out.push(mag_update_sentence(&format!("{m:.1}"))),
            (Some(_), None) => out.extend(mag(q.hypocenter.as_ref())),
            _ => {}
        },
    }
    // 5弱以上を新たに観測した都道府県
    let mut prefs: Vec<_> = q
        .pref_max
        .iter()
        .filter(|p| p.scale >= PREF_MIN && readable(p.scale))
        .filter(|p| {
            let prev = prior
                .iter()
                .flat_map(|pq| pq.pref_max.iter())
                .filter(|pp| pp.pref == p.pref)
                .map(|pp| pp.scale)
                .max()
                .unwrap_or(Scale::UNKNOWN);
            p.scale > prev
        })
        .collect();
    prefs.sort_by_key(|p| std::cmp::Reverse(p.scale));
    let before = out.len();
    out.extend(
        prefs
            .iter()
            .take(PREF_LIST_LIMIT)
            .map(|p| pref_sentence(&p.pref, p.scale)),
    );
    if prefs.len() > PREF_LIST_LIMIT {
        out.push(TSUNAMI_MORE.to_string());
    }
    if out.len() == before {
        let prev_max = prior.iter().map(|p| p.max_scale).max().unwrap_or(Scale::UNKNOWN);
        if readable(q.max_scale) && q.max_scale > prev_max {
            out.push(observed_sentence(q.max_scale));
        }
    }
    // 震度速報はいつも「調査中」なので比べない。比べる相手も震度速報以外の直前の報にする
    if q.info_type != QuakeInfoType::ScalePrompt {
        let prev = prior
            .iter()
            .rev()
            .find(|p| p.info_type != QuakeInfoType::ScalePrompt)
            .map(|p| p.domestic_tsunami.as_str());
        if prev != Some(q.domestic_tsunami.as_str()) {
            out.extend(tsunami_note(&q.domestic_tsunami));
        }
    }
    if out.is_empty() {
        return out;
    }
    std::iter::once(FOLLOW_UP.to_string()).chain(out).collect()
}

/// 名前 + 「。」を最大 limit 件、超えたら「ほかの地域。」。
fn names_block(head: String, names: &[&str], limit: usize) -> Vec<String> {
    if names.is_empty() {
        return vec![];
    }
    let mut out = vec![head];
    out.extend(names.iter().take(limit).map(|n| format!("{n}。")));
    if names.len() > limit {
        out.push(TSUNAMI_MORE.to_string());
    }
    out
}

fn announce_tsunami(ev: &Event, priors: &[&Event]) -> Vec<String> {
    let EventBody::Tsunami(t) = &ev.body else { return vec![] };
    if t.cancelled {
        return vec![TSUNAMI_CANCELLED.to_string()];
    }
    let last = priors.iter().rev().find_map(|p| match &p.body {
        EventBody::Tsunami(pt) => Some(pt),
        _ => None,
    });
    let Some(last) = last.filter(|l| !l.cancelled) else {
        return segments(ev);
    };
    let known = |a: &&crate::quake::model::TsunamiArea| a.grade != TsunamiGrade::Unknown;
    let grade_in = |areas: &[crate::quake::model::TsunamiArea], name: &str| {
        areas.iter().filter(known).find(|a| a.name == name).map(|a| a.grade)
    };
    // 今回の地域のうち、条件に合うものの名前 (今回の並び順)
    let now = |f: &dyn Fn(Option<TsunamiGrade>, TsunamiGrade) -> bool| -> Vec<&str> {
        t.areas
            .iter()
            .filter(known)
            .filter(|a| f(grade_in(&last.areas, &a.name), a.grade))
            .map(|a| a.name.as_str())
            .collect()
    };
    use TsunamiGrade::{MajorWarning, Warning, Watch};
    let major = now(&|p, g| g == MajorWarning && p != Some(MajorWarning));
    let warn = now(&|p, g| g == Warning && matches!(p, None | Some(Watch)));
    let watch = now(&|p, g| g == Watch && p.is_none());
    let mut out = names_block(issued_sentence(MajorWarning), &major, usize::MAX);
    out.extend(names_block(issued_sentence(Warning), &warn, usize::MAX));
    out.extend(names_block(issued_sentence(Watch), &watch, WATCH_AREA_LIMIT));
    let urgent = !major.is_empty() || !warn.is_empty();
    for (old, new) in DOWNGRADES {
        let names = now(&|p, g| g == new && p == Some(old));
        out.extend(names_block(switch_sentence(old, new), &names, WATCH_AREA_LIMIT));
    }
    for old in GRADES {
        let names: Vec<&str> = last
            .areas
            .iter()
            .filter(|a| a.grade == old && grade_in(&t.areas, &a.name).is_none())
            .map(|a| a.name.as_str())
            .collect();
        out.extend(names_block(released_sentence(old), &names, WATCH_AREA_LIMIT));
    }
    if urgent {
        out.push(EVACUATE.to_string());
    }
    out
}

/// 続報は前の報 (priors) との差分だけ読む。docs/tts.md S8。
pub fn announce_segments(ev: &Event, priors: &[&Event]) -> Vec<String> {
    if priors.is_empty() {
        return segments(ev);
    }
    match &ev.body {
        EventBody::Eew(_) => announce_eew(ev, priors),
        EventBody::Quake(_) => announce_quake(ev, priors),
        EventBody::Tsunami(_) => announce_tsunami(ev, priors),
        EventBody::Userquake(_) => segments(ev),
        EventBody::EewDetection(_) => vec![],
    }
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
        // 「マグニチュードは…に更新されました。」は除いて数える
        assert_eq!(
            f.iter()
                .filter(|x| x.starts_with("マグニチュード") && !x.starts_with("マグニチュードは"))
                .count(),
            99
        );
        for want in [
            "マグニチュード0.1。",
            "マグニチュード9.9。",
            "緊急地震速報。",
            "最大震度5弱以上と推定。",
        ] {
            assert!(f.contains(&want.to_string()), "{want}");
        }
    }

    // ---- announce_segments (S8) ----

    fn eew_ev(warning: bool, cancelled: bool, scale: Scale) -> Event {
        ev("p2pquake", EventBody::Eew(eew(warning, cancelled, false, scale)))
    }

    fn pq(t: QuakeInfoType, name: &str, mag: Option<f64>, scale: Scale, tsu: &str, prefs: &[(&str, i32)]) -> Event {
        let mut q = quake(t, name, mag, scale, tsu);
        q.pref_max = prefs
            .iter()
            .map(|(p, sc)| PrefScale {
                pref: (*p).into(),
                scale: Scale(*sc as _),
            })
            .collect();
        quake_ev(q)
    }

    fn ts(cancelled: bool, areas: &[(&str, TsunamiGrade)]) -> Event {
        tsunami_ev(cancelled, areas.iter().map(|(n, g)| area(n, *g)).collect())
    }

    #[test]
    fn announce_empty_priors_equals_segments() {
        let cases = [
            eew_ev(true, false, Scale(55)),
            pq(
                QuakeInfoType::Destination,
                "能登半島沖",
                Some(5.7),
                Scale(50),
                "None",
                &[],
            ),
            ts(false, &[("A", TsunamiGrade::Warning)]),
        ];
        for e in &cases {
            assert_eq!(announce_segments(e, &[]), segments(e));
        }
    }

    #[test]
    fn announce_eew_forecast_then_warning_reads_full_warning() {
        let p = eew_ev(false, false, Scale(45));
        let e = eew_ev(true, false, Scale(55));
        let got = announce_segments(&e, &[&p]);
        assert_eq!(got, segments(&e));
        assert!(!got.contains(&"続報。".to_string()));
    }

    #[test]
    fn announce_eew_max_scale_raised() {
        let p = eew_ev(false, false, Scale(45));
        let e = eew_ev(false, false, Scale(60));
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&["続報。", "予想される最大震度は6強に引き上げられました。"])
        );
    }

    #[test]
    fn announce_eew_lowered_is_silent_and_cancel_is_read() {
        let p = eew_ev(false, false, Scale(60));
        assert!(announce_segments(&eew_ev(false, false, Scale(55)), &[&p]).is_empty());
        assert_eq!(
            announce_segments(&eew_ev(false, true, Scale(55)), &[&p]),
            s(&["先ほどの緊急地震速報は取り消されました。"])
        );
    }

    #[test]
    fn announce_quake_destination_after_scale_prompt() {
        let p = pq(QuakeInfoType::ScalePrompt, "", None, Scale(50), "Checking", &[]);
        let e = pq(
            QuakeInfoType::Destination,
            "能登半島沖",
            Some(5.7),
            Scale(50),
            "Checking",
            &[],
        );
        assert_eq!(
            announce_segments(&e, &[&p]),
            // 震度速報の初報は津波の文を読まないので、震源情報で初めて津波の見込みを伝える
            s(&[
                "続報。",
                "震源は能登半島沖。",
                "マグニチュード5.7。",
                "津波の有無は現在調査中です。"
            ])
        );
    }

    #[test]
    fn announce_quake_magnitude_update() {
        let d = QuakeInfoType::DetailScale;
        let p = pq(d, "能登半島沖", Some(7.4), Scale(70), "Warning", &[]);
        let e = pq(d, "能登半島沖", Some(7.6), Scale(70), "Warning", &[]);
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&["続報。", "マグニチュードは7.6に更新されました。"])
        );
    }

    #[test]
    fn scales_outside_the_table_are_not_read() {
        // 旧い震度階級 (関東大震災の再現の 47・57 など) は「震度不明」と読まない
        let d = QuakeInfoType::DetailScale;
        let first = pq(d, "相模湾", Some(7.9), Scale(57), "None", &[("東京都", 57)]);
        assert!(!segments(&first).iter().any(|x| x.contains("不明")));
        let p = pq(d, "相模湾", Some(7.9), Scale(40), "None", &[("東京都", 40)]);
        let e = pq(
            d,
            "相模湾",
            Some(7.9),
            Scale(57),
            "None",
            &[("東京都", 57), ("千葉県", 47)],
        );
        assert!(announce_segments(&e, &[&p]).is_empty());
    }

    #[test]
    fn announce_quake_pref_raised_only() {
        let d = QuakeInfoType::DetailScale;
        let p = pq(
            d,
            "能登半島沖",
            Some(7.4),
            Scale(70),
            "None",
            &[("新潟県", 45), ("石川県", 70)],
        );
        let e = pq(
            d,
            "能登半島沖",
            Some(7.4),
            Scale(70),
            "None",
            &[("新潟県", 50), ("石川県", 70)],
        );
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&["続報。", "新潟県で震度5強を観測しました。"])
        );
    }

    #[test]
    fn announce_quake_pref_limited_to_three_with_more() {
        let d = QuakeInfoType::DetailScale;
        let p = pq(d, "能登半島沖", Some(7.4), Scale(30), "None", &[]);
        let prefs = [
            ("石川県", 70),
            ("新潟県", 60),
            ("富山県", 55),
            ("福井県", 50),
            ("長野県", 45),
        ];
        let e = pq(d, "能登半島沖", Some(7.4), Scale(70), "None", &prefs);
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&[
                "続報。",
                "石川県で震度7を観測しました。",
                "新潟県で震度6強を観測しました。",
                "富山県で震度6弱を観測しました。",
                "ほかの地域。"
            ])
        );
    }

    #[test]
    fn announce_quake_max_scale_raised_without_big_pref() {
        let d = QuakeInfoType::DetailScale;
        let p = pq(d, "能登半島沖", Some(5.0), Scale(20), "None", &[]);
        let e = pq(d, "能登半島沖", Some(5.0), Scale(30), "None", &[]);
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&["続報。", "最大震度3を観測しました。"])
        );
    }

    #[test]
    fn announce_quake_identical_is_silent() {
        let d = QuakeInfoType::DetailScale;
        let p = pq(d, "能登半島沖", Some(5.0), Scale(30), "None", &[("石川県", 30)]);
        let e = p.clone();
        assert!(announce_segments(&e, &[&p]).is_empty());
    }

    #[test]
    fn announce_quake_tsunami_changed() {
        let d = QuakeInfoType::DetailScale;
        let p = pq(d, "能登半島沖", Some(5.0), Scale(30), "Checking", &[]);
        let e = pq(d, "能登半島沖", Some(5.0), Scale(30), "None", &[]);
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&["続報。", "この地震による津波の心配はありません。"])
        );
    }

    #[test]
    fn announce_tsunami_new_major_warning() {
        use TsunamiGrade::*;
        let p = ts(false, &[("A", Warning), ("B", Warning)]);
        let e = ts(false, &[("A", MajorWarning), ("B", Warning)]);
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&[
                "大津波警報を発表しました。",
                "A。",
                "海岸から離れ、高台に避難してください。"
            ])
        );
    }

    #[test]
    fn announce_tsunami_downgrade() {
        use TsunamiGrade::*;
        let p = ts(false, &[("A", MajorWarning), ("B", Warning)]);
        let e = ts(false, &[("A", Warning), ("B", Warning)]);
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&["大津波警報は津波警報に切り替えられました。", "A。"])
        );
    }

    #[test]
    fn announce_tsunami_area_released() {
        use TsunamiGrade::*;
        let p = ts(false, &[("A", Watch), ("B", Watch)]);
        let e = ts(false, &[("A", Watch)]);
        assert_eq!(
            announce_segments(&e, &[&p]),
            s(&["津波注意報は解除されました。", "B。"])
        );
    }

    #[test]
    fn announce_tsunami_no_change_silent_and_cancel_read() {
        use TsunamiGrade::*;
        let p = ts(false, &[("A", Warning)]);
        assert!(announce_segments(&ts(false, &[("A", Warning)]), &[&p]).is_empty());
        assert_eq!(
            announce_segments(&ts(true, &[]), &[&p]),
            s(&["津波予報は解除されました。"])
        );
    }

    // ---- 能登 2024 の記録 ----

    fn noto() -> Vec<Event> {
        let raw = include_str!("../../../../web/public/demo/noto2024.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        serde_json::from_value(v["events"].clone()).unwrap()
    }

    // S9 を真似たテスト用の priors 集め (tts::priors には依存しない)
    fn priors_in<'a>(events: &'a [Event], id: &str) -> (&'a Event, Vec<&'a Event>) {
        let i = events.iter().position(|e| e.id == id).expect("id");
        let ev = &events[i];
        let before = &events[..i];
        let priors = before
            .iter()
            .filter(|x| match (&ev.body, &x.body) {
                (EventBody::Eew(a), EventBody::Eew(b)) => a.event_id == b.event_id,
                (EventBody::Quake(a), EventBody::Quake(b)) => a.origin_time_ms == b.origin_time_ms,
                (EventBody::Tsunami(_), EventBody::Tsunami(_)) => true,
                _ => false,
            })
            .collect();
        (ev, priors)
    }

    fn noto_announce(id: &str) -> Vec<String> {
        let events = noto();
        let (ev, priors) = priors_in(&events, id);
        announce_segments(ev, &priors)
    }

    #[test]
    fn noto_print_all_quake_and_tsunami() {
        let events = noto();
        for e in events
            .iter()
            .filter(|e| matches!(e.body, EventBody::Quake(_) | EventBody::Tsunami(_)))
        {
            let (ev, priors) = priors_in(&events, &e.id);
            println!(
                "{} priors={} -> {:?}",
                ev.id,
                priors.len(),
                announce_segments(ev, &priors)
            );
        }
    }

    #[test]
    fn noto_scale_prompt_seven() {
        assert_eq!(
            noto_announce("65926682f0f6de0007564792"),
            s(&["続報。", "石川県で震度7を観測しました。"])
        );
    }

    #[test]
    fn noto_scale_prompt_after_destination_does_not_say_tsunami_is_checking() {
        // 震度速報はいつも「調査中」を持つ。震源情報で「心配なし」と伝えた後に「調査中」へ戻ったように読まない
        assert!(noto_announce("65926504f0f6de00075645e0").is_empty());
    }

    #[test]
    fn noto_detail_scale_does_not_repeat_the_tsunami_note_of_the_destination() {
        // 震源情報 (心配なし) の後の各地の震度 (心配なし) では、津波の文を繰り返さない
        assert!(noto_announce("65926560f0f6de00075645f3").is_empty());
    }

    #[test]
    fn noto_scale_prompt_unchanged_is_silent() {
        assert!(noto_announce("65926682f0f6de000756477e").is_empty());
    }

    #[test]
    fn noto_tsunami_major_warning_issued() {
        // 前 (65926607...) から、石川県能登が大津波警報へ、山形・福井・兵庫北部が津波警報へ、
        // 北海道2区域・福岡・佐賀・壱岐対馬が新たに注意報
        assert_eq!(
            noto_announce("65926857f0f6de0007564895"),
            s(&[
                "大津波警報を発表しました。",
                "石川県能登。",
                "津波警報を発表しました。",
                "山形県。",
                "福井県。",
                "兵庫県北部。",
                "津波注意報を発表しました。",
                "北海道太平洋沿岸西部。",
                "北海道日本海沿岸北部。",
                "福岡県日本海沿岸。",
                "佐賀県北部。",
                "壱岐・対馬。",
                "海岸から離れ、高台に避難してください。"
            ])
        );
    }

    #[test]
    fn noto_tsunami_major_downgraded() {
        let got = noto_announce("6592a25af0f6de0007564dab");
        assert_eq!(
            &got[..2],
            &s(&["大津波警報は津波警報に切り替えられました。", "石川県能登。"])[..]
        );
    }

    #[test]
    fn noto_magnitude_update_without_hypocenter() {
        let got = noto_announce("659268caf0f6de00075648b1");
        assert!(got.contains(&"マグニチュードは7.6に更新されました。".to_string()));
        assert!(got.iter().all(|x| !x.starts_with("震源は")));
    }

    // ---- 地震感知情報 (docs/tts.md S12) ----

    fn uq_ev(areas: &[(u32, f64)], confidence: f64) -> Event {
        let u = Userquake {
            started_at: "2026/09/29 10:00:00.000".into(),
            updated_at: "2026/09/29 10:00:10.000".into(),
            count: 9,
            confidence,
            areas: areas
                .iter()
                .map(|&(code, c)| crate::quake::model::UserquakeArea {
                    code,
                    count: 3,
                    confidence: c,
                })
                .collect(),
        };
        ev("p2pquake", EventBody::Userquake(u))
    }

    const UQ_TAIL: &str = "揺れを感じたという報告が集まっています。";

    #[test]
    fn userquake_one_prefecture() {
        // 205 は茨城
        assert_eq!(segments(&uq_ev(&[(205, 0.9)], 0.97)), s(&["茨城県で、", UQ_TAIL]));
    }

    #[test]
    fn userquake_two_prefectures_in_confidence_order() {
        // 241 は千葉。信頼度の高い茨城が先
        assert_eq!(
            segments(&uq_ev(&[(241, 0.7), (205, 0.9)], 0.97)),
            s(&["茨城県、", "千葉県で、", UQ_TAIL])
        );
    }

    #[test]
    fn userquake_three_or_more_prefectures_keep_two_and_add_nado() {
        // 215 は栃木。3 県目は読まず「など」
        assert_eq!(
            segments(&uq_ev(&[(241, 0.7), (205, 0.9), (215, 0.65)], 0.97)),
            s(&["茨城県、", "千葉県などで、", UQ_TAIL])
        );
    }

    #[test]
    fn userquake_is_not_read_unless_credible() {
        assert!(segments(&uq_ev(&[(205, 0.9)], 0.5)).is_empty());
        assert!(segments(&uq_ev(&[(205, 0.59)], 0.97)).is_empty());
        assert!(segments(&uq_ev(&[], 0.97)).is_empty());
        // 続報の経路も同じ
        assert_eq!(
            announce_segments(&uq_ev(&[(205, 0.9)], 0.97), &[]),
            s(&["茨城県で、", UQ_TAIL])
        );
    }

    #[test]
    fn userquake_wording_never_claims_detection() {
        let all = userquake_pref_segments().join("") + UQ_TAIL;
        assert!(!all.contains("検知") && !all.contains("観測"));
    }

    #[test]
    fn userquake_parts_are_all_prewarmable() {
        let pre = userquake_pref_segments();
        assert_eq!(pre.len(), 47 * 3);
        for areas in [
            vec![(205, 0.9)],
            vec![(241, 0.7), (205, 0.9)],
            vec![(241, 0.7), (205, 0.9), (215, 0.65)],
        ] {
            for seg in segments(&uq_ev(&areas, 0.97)) {
                assert!(pre.contains(&seg) || fixed_segments().contains(&seg), "{seg}");
            }
        }
    }
}
