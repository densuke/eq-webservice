//! 気象庁/P2P地震情報形式の JST 時刻文字列 ("YYYY/MM/DD HH:MM:SS[.fff]") と
//! UNIX epoch ミリ秒の相互変換。外部クレートを増やさないための最小実装。

const JST_OFFSET_MS: i64 = 9 * 3600 * 1000;

/// "2026/09/28 16:24:00" や "2026/09/28 16:26:56.811" を epoch ミリ秒にする。
pub fn parse_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, time) = s.split_once(' ')?;
    let mut d = date.split(['/', '-']);
    let y: i64 = d.next()?.parse().ok()?;
    let mo: u32 = d.next()?.parse().ok()?;
    let da: u32 = d.next()?.parse().ok()?;
    let (hms, frac) = match time.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (time, None),
    };
    let mut t = hms.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let se: i64 = t.next().unwrap_or("0").parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let ms: i64 = match frac {
        Some(f) => {
            let f: String = f.chars().chain("000".chars()).take(3).collect();
            f.parse().ok()?
        }
        None => 0,
    };
    let days = days_from_civil(y, mo, da);
    Some(((days * 24 + h) * 60 + mi) * 60_000 + se * 1000 + ms - JST_OFFSET_MS)
}

/// epoch ミリ秒を "YYYY/MM/DD HH:MM:SS" (JST) にする。
pub fn format(ms: i64) -> String {
    let local = ms + JST_OFFSET_MS;
    let days = local.div_euclid(86_400_000);
    let rem = local.rem_euclid(86_400_000) / 1000;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}/{m:02}/{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// 文字列時刻を delta ミリ秒ずらす (リプレイ用)。解釈できなければそのまま。
pub fn shift_str(s: &str, delta_ms: i64) -> String {
    match parse_ms(s) {
        Some(ms) => format(ms + delta_ms),
        None => s.to_string(),
    }
}

// Howard Hinnant の days_from_civil / civil_from_days
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 1970-01-01 からの日数を (年, 月, 日) にする
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_instant() {
        // 2026-09-28T07:24:00Z
        assert_eq!(parse_ms("2026/09/28 16:24:00"), Some(1_790_580_240_000));
        assert_eq!(parse_ms("2026/09/28 16:24:00.811"), Some(1_790_580_240_811));
        assert_eq!(parse_ms("garbage"), None);
    }

    #[test]
    fn roundtrip() {
        for s in ["2026/09/28 16:24:00", "2024/01/01 00:00:00", "2000/02/29 23:59:59"] {
            assert_eq!(format(parse_ms(s).unwrap()), s);
        }
        assert_eq!(shift_str("2026/09/28 23:59:30", 60_000), "2026/09/29 00:00:30");
    }
}
