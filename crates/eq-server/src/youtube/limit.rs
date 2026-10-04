//! 1 日に上げる本数の数え方。YouTube の割り当ては太平洋時間 (PT) の 0 時に戻るので、数える日も PT で区切る。
//! 日付の計算と、状態ファイルの中身を決める純粋な関数。

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::quake::jst::{civil_from_days, days_from_civil};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;

/// 月の最初の日曜日の、1970-01-01 からの日数 (`nth` = 0 なら最初、1 なら 2 番目)
fn sunday_of(year: i64, month: u32, nth: i64) -> i64 {
    let first = days_from_civil(year, month, 1);
    let dow = (first + 4).rem_euclid(7); // 1970-01-01 は木曜。0 = 日曜
    first + (7 - dow) % 7 + 7 * nth
}

/// 太平洋時間の UTC との差 (時間。夏時間は -7、標準時は -8)。
/// 夏時間は 3 月の第 2 日曜の 2:00 (PST = UTC 10:00) から、11 月の第 1 日曜の 2:00 (PDT = UTC 9:00) まで
pub fn pt_offset_hours(utc_ms: i64) -> i64 {
    let (year, _, _) = civil_from_days(utc_ms.div_euclid(DAY_MS));
    let start = sunday_of(year, 3, 1) * DAY_MS + 10 * HOUR_MS;
    let end = sunday_of(year, 11, 0) * DAY_MS + 9 * HOUR_MS;
    if (start..end).contains(&utc_ms) {
        -7
    } else {
        -8
    }
}

/// 太平洋時間の日付 ("2026-10-02")
pub fn pt_day(utc_ms: i64) -> String {
    let local = utc_ms + pt_offset_hours(utc_ms) * HOUR_MS;
    let (y, m, d) = civil_from_days(local.div_euclid(DAY_MS));
    format!("{y:04}-{m:02}-{d:02}")
}

/// その日に上げた本数と、割り当ての超過で止めたか
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Limits {
    /// 太平洋時間の日付
    pub day: String,
    pub count: u32,
    /// quotaExceeded などが返った (この日はもう上げない)
    #[serde(default)]
    pub blocked: bool,
}

impl Limits {
    /// today の分だけにする (日が変わっていれば、0 本・止めていない状態から)
    pub fn on(self, today: &str) -> Limits {
        if self.day == today {
            self
        } else {
            Limits {
                day: today.to_string(),
                count: 0,
                blocked: false,
            }
        }
    }

    pub fn allows(&self, daily_limit: u32) -> bool {
        !self.blocked && self.count < daily_limit
    }

    pub fn uploaded(self) -> Limits {
        Limits {
            count: self.count + 1,
            ..self
        }
    }

    pub fn quota_hit(self) -> Limits {
        Limits { blocked: true, ..self }
    }
}

/// 状態ファイルを読む。無い・壊れているときは空 (今日 0 本)
pub fn load(path: &Path) -> Limits {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// 書きかけを残さないよう、別名で書いて置き換える
pub fn save(path: &Path, limits: &Limits) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(limits)?).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quake::jst::parse_ms;

    fn utc(s: &str) -> i64 {
        // parse_ms は JST の文字列なので、9 時間戻して UTC として読む
        parse_ms(s).unwrap() + 9 * HOUR_MS
    }

    #[test]
    fn the_day_changes_at_midnight_pacific_in_summer() {
        assert_eq!(pt_day(utc("2026/10/03 06:59:59")), "2026-10-02"); // PDT 23:59:59
        assert_eq!(pt_day(utc("2026/10/03 07:00:00")), "2026-10-03");
    }

    #[test]
    fn the_day_changes_at_midnight_pacific_in_winter() {
        assert_eq!(pt_day(utc("2026/12/03 07:59:59")), "2026-12-02"); // PST 23:59:59
        assert_eq!(pt_day(utc("2026/12/03 08:00:00")), "2026-12-03");
    }

    #[test]
    fn daylight_saving_switches_on_the_right_sundays() {
        // 2026 年は 3/8 と 11/1
        assert_eq!(pt_offset_hours(utc("2026/03/08 09:59:59")), -8);
        assert_eq!(pt_offset_hours(utc("2026/03/08 10:00:00")), -7);
        assert_eq!(pt_offset_hours(utc("2026/11/01 08:59:59")), -7);
        assert_eq!(pt_offset_hours(utc("2026/11/01 09:00:00")), -8);
        // 2027 年は 3/14 と 11/7
        assert_eq!(pt_offset_hours(utc("2027/03/14 10:00:00")), -7);
        assert_eq!(pt_offset_hours(utc("2027/11/07 09:00:00")), -8);
    }

    #[test]
    fn jst_morning_is_still_the_previous_pacific_day() {
        // 日本時間の 10/03 08:00 は、太平洋時間ではまだ 10/02 (前日)
        assert_eq!(pt_day(parse_ms("2026/10/03 08:00:00").unwrap()), "2026-10-02");
        assert_eq!(pt_day(parse_ms("2026/10/03 16:00:00").unwrap()), "2026-10-03");
    }

    #[test]
    fn the_daily_limit_counts_and_resets_on_a_new_pacific_day() {
        let l = Limits::default().on("2026-10-02");
        assert!(l.allows(2));
        let l = l.uploaded();
        assert!(l.allows(2));
        let l = l.uploaded();
        assert!(!l.allows(2));
        // 同じ日のままなら、数えたまま
        assert_eq!(l.clone().on("2026-10-02").count, 2);
        // 日が変われば、0 本から
        let next = l.on("2026-10-03");
        assert_eq!((next.count, next.allows(2)), (0, true));
    }

    #[test]
    fn a_quota_hit_stops_the_rest_of_that_day_only() {
        let l = Limits::default().on("2026-10-02").quota_hit();
        assert!(!l.allows(3));
        assert!(!l.clone().on("2026-10-02").allows(3));
        assert!(l.on("2026-10-03").allows(3));
    }

    #[test]
    fn the_state_file_round_trips_and_a_missing_one_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("youtube-state.json");
        assert_eq!(load(&path), Limits::default());
        let l = Limits::default().on("2026-10-02").uploaded().quota_hit();
        save(&path, &l).unwrap();
        assert_eq!(load(&path), l);
    }
}
