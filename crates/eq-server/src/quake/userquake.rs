//! 地震感知情報 (P2P地震情報の利用者による「揺れた」報告の集計) を読み上げてよいかの判断。docs/tts.md S12。
//! 機器による検知ではないので、信頼できる評価のときだけ、気象庁の発表より先の知らせとして扱う。
//! 地図に出す条件 (web/src/userquake.ts の userquakeShown) と、読む条件の閾値はここと web で揃える。

use super::area::PREFS;
use super::jst;
use super::model::{Userquake, UserquakeArea};

/// 評価全体の信頼度の下限。P2P地震情報 Beta3 では 0 が非表示、表示される水準 (レベル1〜4) は 0.96774〜0.98052
pub const MIN_CONFIDENCE: f64 = 0.96;
/// 地域ごとの信頼度の下限 (P2P の区分で A か B)
pub const MIN_AREA_CONFIDENCE: f64 = 0.6;
/// 最後の更新からこの時間までを「いまの揺れ」とする (地図に出す時間と同じ)
const SHOW_MS: i64 = 2 * 60_000;
/// 報告の始まりのこの時間前以降に気象庁の地震の情報が届いていれば、その揺れの報告とみなす
const OFFICIAL_LEAD_MS: i64 = 30_000;
/// 読み上げの間隔の下限
pub const COOLDOWN_MS: i64 = 10 * 60_000;

/// 読み上げる都道府県の数の上限 (これを超えるときは「など」を付ける)
pub const MAX_PREFS: usize = 2;

const TABLE: &str = include_str!("userquake_prefs.txt");

/// P2P地震情報の地域コードの都道府県。分からなければ None
pub fn pref_of(code: u32) -> Option<&'static str> {
    TABLE
        .lines()
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| {
            let (c, p) = l.split_once(' ')?;
            (c.parse::<u32>().ok()? == code).then_some(p)
        })
        // 表の都道府県は「東京」「鹿児島」のように県・府・都を省いた形
        .and_then(|short| PREFS.iter().find(|p| p.starts_with(short)).copied())
}

fn credible_area(a: &UserquakeArea) -> bool {
    a.confidence >= MIN_AREA_CONFIDENCE
}

/// 信頼できる地域の都道府県。地域の信頼度の高い順 (同じなら件数の多い順) で、重複なし
pub fn credible_prefs(u: &Userquake) -> Vec<&'static str> {
    let mut areas: Vec<&UserquakeArea> = u.areas.iter().filter(|a| credible_area(a)).collect();
    areas.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then(b.count.cmp(&a.count))
            .then(a.code.cmp(&b.code))
    });
    let mut out: Vec<&'static str> = Vec::new();
    for p in areas.iter().filter_map(|a| pref_of(a.code)) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// 読んでよいほど信頼できる評価か (全体の信頼度と、地域の信頼度の両方)
pub fn credible(u: &Userquake) -> bool {
    u.confidence >= MIN_CONFIDENCE && !credible_prefs(u).is_empty()
}

/// いま読む対象の揺れか。最後の更新から 2 分以内で、報告の始まりの前後に気象庁の地震の情報が届いていない。
/// official_ms は届いた気象庁の地震の情報 (緊急地震速報・地震情報) の受信時刻
pub fn shown(u: &Userquake, now_ms: i64, official_ms: &[i64]) -> bool {
    let (Some(started), Some(updated)) = (jst::parse_ms(&u.started_at), jst::parse_ms(&u.updated_at)) else {
        return false;
    };
    now_ms - updated <= SHOW_MS && !official_ms.iter().any(|&t| t >= started - OFFICIAL_LEAD_MS)
}

/// 同じ揺れを 1 回だけ、間隔を空けて読むための記憶
#[derive(Debug, Default)]
pub struct Gate {
    last: Option<(String, i64)>,
}

impl Gate {
    /// この評価を読むか。読むと決めたら記憶する
    pub fn should_read(&mut self, u: &Userquake, now_ms: i64, official_ms: &[i64]) -> bool {
        if !credible(u) || !shown(u, now_ms, official_ms) {
            return false;
        }
        if let Some((started, at)) = &self.last {
            if *started == u.started_at || now_ms - at < COOLDOWN_MS {
                return false;
            }
        }
        self.last = Some((u.started_at.clone(), now_ms));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026/09/29 10:00:00 JST
    const T0: i64 = 1_790_643_600_000;

    fn area(code: u32, count: u32, confidence: f64) -> UserquakeArea {
        UserquakeArea {
            code,
            count,
            confidence,
        }
    }

    fn uq(started: &str, updated: &str, confidence: f64, areas: Vec<UserquakeArea>) -> Userquake {
        Userquake {
            started_at: started.into(),
            updated_at: updated.into(),
            count: 5,
            confidence,
            areas,
        }
    }

    fn good() -> Userquake {
        uq(
            "2026/09/29 10:00:00.000",
            "2026/09/29 10:00:10.000",
            0.97,
            vec![area(205, 6, 0.9)],
        )
    }

    #[test]
    fn every_listed_area_resolves_to_a_prefecture() {
        let n = TABLE.lines().filter(|l| !l.starts_with('#')).count();
        assert_eq!(n, 138);
        for l in TABLE.lines().filter(|l| !l.starts_with('#')) {
            let code: u32 = l.split_once(' ').unwrap().0.parse().unwrap();
            assert!(pref_of(code).is_some(), "{l}");
        }
        assert_eq!(pref_of(10), Some("北海道"));
        assert_eq!(pref_of(255), Some("東京都"));
        assert_eq!(pref_of(680), Some("鹿児島県"));
        assert_eq!(pref_of(900), None);
    }

    #[test]
    fn prefectures_are_ordered_by_confidence_and_deduplicated() {
        // 100 と 105 は同じ県 (青森)
        let a = pref_of(100).unwrap();
        assert_eq!(pref_of(105), Some(a));
        let u = uq(
            "s",
            "u",
            0.97,
            vec![
                area(10, 2, 0.7),
                area(100, 3, 0.95),
                area(105, 9, 0.9),
                area(900, 9, 0.99),
            ],
        );
        let v = credible_prefs(&u);
        assert_eq!(v.iter().filter(|p| **p == a).count(), 1);
        assert_eq!(v, vec![a, "北海道"]);
        // 地域の信頼度が低い地域は入れない
        assert!(credible_prefs(&uq("s", "u", 0.97, vec![area(10, 9, 0.59)])).is_empty());
    }

    #[test]
    fn credibility_needs_both_overall_and_area_confidence() {
        assert!(credible(&good()));
        let mut low = good();
        low.confidence = 0.5;
        assert!(!credible(&low));
        let mut zero = good();
        zero.confidence = 0.0;
        assert!(!credible(&zero));
        let mut weak_area = good();
        weak_area.areas = vec![area(205, 6, 0.59)];
        assert!(!credible(&weak_area));
        // 位置の分からない地域だけでは読む地名がない
        let mut unknown = good();
        unknown.areas = vec![area(900, 6, 0.99)];
        assert!(!credible(&unknown));
    }

    #[test]
    fn official_report_since_the_start_suppresses_it() {
        let u = good();
        let now = T0 + 20_000;
        assert!(shown(&u, now, &[]));
        // 始まりより 30 秒前以降に気象庁の情報が届いていたら出さない
        assert!(!shown(&u, now, &[T0 + 15_000]));
        assert!(!shown(&u, now, &[T0 - 30_000]));
        // ずっと前の別の地震は関係ない
        assert!(shown(&u, now, &[T0 - 10 * 60_000]));
        // 最後の更新から 2 分を過ぎたら出さない
        assert!(!shown(&u, T0 + 10_000 + SHOW_MS + 1, &[]));
        assert!(!shown(&uq("bad", "bad", 0.97, vec![]), now, &[]));
    }

    #[test]
    fn gate_reads_once_per_shaking() {
        let mut g = Gate::default();
        let u = good();
        assert!(g.should_read(&u, T0 + 20_000, &[]));
        // 同じ揺れの更新では読み直さない
        let mut again = good();
        again.updated_at = "2026/09/29 10:00:30.000".into();
        assert!(!g.should_read(&again, T0 + 40_000, &[]));
    }

    #[test]
    fn gate_waits_ten_minutes_between_readings() {
        let mut g = Gate::default();
        assert!(g.should_read(&good(), T0 + 20_000, &[]));
        let next = |s: &str, u: &str| uq(s, u, 0.97, vec![area(205, 6, 0.9)]);
        let soon = next("2026/09/29 10:05:00.000", "2026/09/29 10:05:10.000");
        assert!(!g.should_read(&soon, T0 + 5 * 60_000 + 20_000, &[]));
        // 間隔を空ければ別の揺れは読む。読めなかった揺れは記憶に入らない
        let later = next("2026/09/29 10:11:00.000", "2026/09/29 10:11:10.000");
        assert!(g.should_read(&later, T0 + 11 * 60_000 + 20_000, &[]));
    }

    #[test]
    fn gate_ignores_unreadable_evaluations_without_remembering_them() {
        let mut g = Gate::default();
        let mut weak = good();
        weak.confidence = 0.5;
        assert!(!g.should_read(&weak, T0 + 20_000, &[]));
        // 同じ揺れでも、あとで信頼できる評価になれば読む
        assert!(g.should_read(&good(), T0 + 25_000, &[]));
        // 気象庁の情報が届いていれば読まない
        let mut g = Gate::default();
        assert!(!g.should_read(&good(), T0 + 20_000, &[T0 + 5_000]));
    }
}
