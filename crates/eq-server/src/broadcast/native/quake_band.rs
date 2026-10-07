//! 地震の画面の警報の帯 (定義の banners)。緊急性の高いものだけを出す: 大津波警報 → 津波警報 → 気象の特別警報。
//! 該当が無ければ空 (平時の「ありません」も出さない)。判断と文の組み立ては純粋な関数 (quake_band)。

use tiny_skia::Pixmap;

use super::banner::{draw_pages, Section};
use super::data::{warning_level, warning_summary, WarningLevel, Warnings};
use super::layout_resolve::Rect;
use super::text::Text;
use crate::quake::model::{TsunamiArea, TsunamiGrade};
use crate::quake::{Event, EventBody};

const PER_PREF: usize = 5;

/// 帯に出す種類。重い順 (出す順)。地の色は平時の帯と同じ (大津波警報は紫 = 危険、津波警報は赤 = 警報、特別警報は黒)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    MajorTsunami,
    Tsunami,
    Special,
}

impl Tone {
    pub fn level(self) -> WarningLevel {
        match self {
            Tone::MajorTsunami => WarningLevel::Danger,
            Tone::Tsunami => WarningLevel::Warning,
            Tone::Special => WarningLevel::Emergency,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct QuakeBand {
    /// 帯の地の色を決める、いちばん重い種類 (= 先頭の文)
    pub top: Tone,
    /// 種類ごとの見出しと本文 (「【大津波警報】」「宮城県・岩手県」)。重い順
    pub sections: Vec<Section>,
    /// sections と同じ順の、各種別の種類 (枠の見出しの色に使う)
    pub tones: Vec<Tone>,
}

/// いま発表中の津波予報区 (届いた津波の報のうち発行が最新のもの。解除なら空)。web/src/tsunami.ts と同じ規則
pub fn current_areas(events: &[Event]) -> &[TsunamiArea] {
    events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Tsunami(t) => Some(t),
            _ => None,
        })
        .reduce(|cur, t| if t.issued_at < cur.issued_at { cur } else { t })
        .filter(|t| !t.cancelled)
        .map_or(&[], |t| &t.areas)
}

fn tsunami_section(areas: &[TsunamiArea], grade: TsunamiGrade, heading: &str) -> Option<Section> {
    let names: Vec<&str> = areas
        .iter()
        .filter(|a| a.grade == grade)
        .map(|a| a.name.as_str())
        .collect();
    (!names.is_empty()).then(|| Section::new(heading, names.join("・")))
}

/// 帯に出す内容。該当が無ければ None
pub fn quake_band(areas: &[TsunamiArea], w: Option<&Warnings>) -> Option<QuakeBand> {
    let mut parts = Vec::new();
    if let Some(s) = tsunami_section(areas, TsunamiGrade::MajorWarning, "【大津波警報】") {
        parts.push((Tone::MajorTsunami, s));
    }
    if let Some(s) = tsunami_section(areas, TsunamiGrade::Warning, "【津波警報】") {
        parts.push((Tone::Tsunami, s));
    }
    if let Some(w) = w {
        // 特別警報だけを残した写しを、平時の帯と同じ文にする
        let mut special = w.clone();
        for kinds in special.areas.values_mut() {
            kinds.retain(|k| warning_level(&k.name) == WarningLevel::Emergency);
        }
        special.areas.retain(|_, kinds| !kinds.is_empty());
        if let Some(sum) = warning_summary(&special, PER_PREF) {
            parts.push((Tone::Special, Section::new("【特別警報】", sum.lines.join("、"))));
        }
    }
    let top = parts.first()?.0;
    Some(QuakeBand {
        top,
        tones: parts.iter().map(|p| p.0).collect(),
        sections: parts.into_iter().map(|p| p.1).collect(),
    })
}

/// 帯の矩形 at の中に描く。該当が無ければ何も描かない
pub fn draw(pm: &mut Pixmap, text: &mut Text, areas: &[TsunamiArea], w: Option<&Warnings>, at: Rect, now_ms: u64) {
    let Some(b) = quake_band(areas, w) else { return };
    draw_pages(pm, text, at, b.top.level(), &b.sections, now_ms);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::native::data::Kind;
    use crate::quake::Tsunami;

    fn area(name: &str, grade: TsunamiGrade) -> TsunamiArea {
        TsunamiArea {
            name: name.into(),
            grade,
            immediate: false,
            first_height: None,
            max_height: None,
        }
    }

    pub(crate) fn warnings(items: &[(&str, &str, &str)]) -> Warnings {
        let mut w = Warnings::default();
        for (code, name, kind) in items {
            w.areas
                .entry(code.to_string())
                .or_default()
                .push(Kind { name: (*kind).into() });
            w.names.insert(code.to_string(), (*name).into());
        }
        w
    }

    fn ev(id: &str, issued: &str, cancelled: bool, areas: Vec<TsunamiArea>) -> Event {
        Event {
            id: id.into(),
            source: "t".into(),
            received_at_ms: 0,
            body: EventBody::Tsunami(Tsunami {
                cancelled,
                issued_at: issued.into(),
                areas,
            }),
        }
    }

    #[test]
    fn nothing_urgent_is_none() {
        assert_eq!(quake_band(&[], None), None);
        // 津波注意報・予報不明・ふつうの警報・注意報は出さない
        let w = warnings(&[
            ("1310100", "千代田区", "大雨警報"),
            ("1310100", "千代田区", "強風注意報"),
        ]);
        let a = [
            area("宮城県", TsunamiGrade::Watch),
            area("岩手県", TsunamiGrade::Unknown),
        ];
        assert_eq!(quake_band(&a, Some(&w)), None);
    }

    #[test]
    fn the_order_is_major_tsunami_then_tsunami_then_special() {
        let a = [
            area("岩手県", TsunamiGrade::Warning),
            area("宮城県", TsunamiGrade::MajorWarning),
            area("青森県", TsunamiGrade::Watch),
            area("福島県", TsunamiGrade::MajorWarning),
            area("茨城県", TsunamiGrade::Warning),
        ];
        let w = warnings(&[("0420100", "仙台市", "大雨特別警報"), ("0420100", "仙台市", "大雨警報")]);
        let b = quake_band(&a, Some(&w)).unwrap();
        assert_eq!(b.top, Tone::MajorTsunami);
        assert_eq!(
            b.sections,
            [
                Section::new("【大津波警報】", "宮城県・福島県".into()),
                Section::new("【津波警報】", "岩手県・茨城県".into()),
                Section::new("【特別警報】", "大雨特別警報: 宮城県 仙台市".into()),
            ]
        );
    }

    #[test]
    fn the_ground_follows_the_heaviest_kind_present() {
        let w = warnings(&[("0420100", "仙台市", "暴風特別警報")]);
        let tsu = [area("岩手県", TsunamiGrade::Warning)];
        assert_eq!(quake_band(&tsu, None).unwrap().top, Tone::Tsunami);
        assert_eq!(quake_band(&tsu, Some(&w)).unwrap().top, Tone::Tsunami);
        assert_eq!(quake_band(&[], Some(&w)).unwrap().top, Tone::Special);
        assert_eq!(Tone::MajorTsunami.level(), WarningLevel::Danger);
        assert_eq!(Tone::Tsunami.level(), WarningLevel::Warning);
        assert_eq!(Tone::Special.level(), WarningLevel::Emergency);
    }

    #[test]
    fn current_areas_follow_the_latest_report_and_a_cancel_clears_them() {
        let a = vec![area("宮城県", TsunamiGrade::Warning)];
        let b = vec![area("宮城県", TsunamiGrade::MajorWarning)];
        assert!(current_areas(&[]).is_empty());
        let evs = [
            ev("1", "2026-01-01T00:10", false, a.clone()),
            ev("2", "2026-01-01T00:20", false, b.clone()),
        ];
        assert_eq!(current_areas(&evs), b.as_slice());
        // 遅れて届いた古い予報は無視する
        let late = [evs[1].clone(), evs[0].clone()];
        assert_eq!(current_areas(&late), b.as_slice());
        // 解除
        let cancelled = [evs[0].clone(), ev("3", "2026-01-01T00:30", true, vec![])];
        assert!(current_areas(&cancelled).is_empty());
    }
}
