//! jsonl の記録の置き場。設定の path (例 data/events.jsonl) はそのままで、書き込みは日付ごとのファイル
//! (data/events-YYYY-MM-DD.jsonl) に分ける。日付は「書き込んだ時刻 (現在時刻、UTC)」で決める。
//! 行の received_at_ms は書き込み時刻以前なので、範囲 [from, to] の行は from の日付以降のファイルにしかない。
//! 分割前の path そのもの (旧ファイル) は移行せず、あれば常に読む。

use std::path::{Path, PathBuf};

const DAY_MS: u64 = 86_400_000;

/// epoch からの日数 → (年, 月, 日) (UTC)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// (年, 月, 日) → epoch からの日数 (UTC)
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// base の旧ファイル名から、日付ファイルの名前の前後 (`events-` と `.jsonl`)
fn affixes(base: &Path) -> (String, String) {
    let stem = base
        .file_stem()
        .map_or_else(|| "events".into(), |s| s.to_string_lossy().into_owned());
    let ext = base
        .extension()
        .map_or_else(|| "jsonl".into(), |s| s.to_string_lossy().into_owned());
    (format!("{stem}-"), format!(".{ext}"))
}

/// 時刻 (ms) を書くファイル: base の隣の events-YYYY-MM-DD.jsonl
pub fn daily_path(base: &Path, now_ms: u64) -> PathBuf {
    let (y, m, d) = civil_from_days((now_ms / DAY_MS) as i64);
    let (pre, suf) = affixes(base);
    base.with_file_name(format!("{pre}{y:04}-{m:02}-{d:02}{suf}"))
}

/// 日付ファイルの名前から日数を取る
fn day_of_name(name: &str, pre: &str, suf: &str) -> Option<u64> {
    let date = name.strip_prefix(pre)?.strip_suffix(suf)?;
    let b = date.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, m, d) = (
        date[..4].parse().ok()?,
        date[5..7].parse().ok()?,
        date[8..].parse().ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    u64::try_from(days_from_civil(y, m, d)).ok()
}

/// 範囲 [from, to] (ms) を読むファイル: 旧ファイル (あれば) と、from の日付 〜 to の日付 + 1 日 の日付ファイル (日付順)。
/// +1 日は、日付をまたいで遅れて書かれた過去時刻の行 (p2pquake 再接続時の津波) のため。
/// 小さなディレクトリの一覧なので blocking のまま呼ぶ。
/// ponytail: 1 日を超える切断のあとに取り込まれた行は、取りこぼしうる
pub fn files_in_range(base: &Path, from: u64, to: u64) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if std::fs::metadata(base).is_ok_and(|m| m.is_file()) {
        files.push(base.to_path_buf());
    }
    let (first, last) = (from / DAY_MS, (to / DAY_MS).saturating_add(1));
    let (pre, suf) = affixes(base);
    let dir = base
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(e),
    };
    let mut daily = Vec::new();
    for ent in rd {
        let ent = ent?;
        let day = ent.file_name().to_str().and_then(|n| day_of_name(n, &pre, &suf));
        if let Some(day) = day.filter(|d| (first..=last).contains(d)) {
            daily.push((day, ent.path()));
        }
    }
    daily.sort();
    files.extend(daily.into_iter().map(|(_, p)| p));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: u64 = DAY_MS;

    #[test]
    fn civil_dates_round_trip() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        for n in [0, 1, 11_016, 19_723, 20_000, 47_482] {
            let (y, m, d) = civil_from_days(n);
            assert_eq!(days_from_civil(y, m, d), n);
        }
    }

    #[test]
    fn daily_path_uses_the_utc_date_of_the_write_time() {
        let base = Path::new("data/events.jsonl");
        let t = 19_723 * D; // 2024-01-01T00:00:00Z
        assert_eq!(daily_path(base, t - 1), Path::new("data/events-2023-12-31.jsonl"));
        assert_eq!(daily_path(base, t), Path::new("data/events-2024-01-01.jsonl"));
        assert_eq!(daily_path(Path::new("e.jsonl"), t), Path::new("e-2024-01-01.jsonl"));
    }

    fn touch(dir: &Path, names: &[&str]) {
        for n in names {
            std::fs::write(dir.join(n), "").unwrap();
        }
    }

    fn names(files: Vec<PathBuf>) -> Vec<String> {
        files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn picks_from_day_to_the_day_after_to_plus_the_legacy_file() {
        let dir = tempfile::tempdir().unwrap();
        touch(
            dir.path(),
            &[
                "events.jsonl",
                "events-2023-12-30.jsonl",
                "events-2023-12-31.jsonl",
                "events-2024-01-01.jsonl",
                "events-2024-01-02.jsonl",
                "events-2024-01-03.jsonl",
                "events-bad.jsonl",
                "other-2024-01-01.jsonl",
            ],
        );
        let base = dir.path().join("events.jsonl");
        // 範囲は 2023-12-31 23:59:59.999 〜 2024-01-01 00:00:00.000 (日付をまたぐ)
        let got = files_in_range(&base, 19_723 * D - 1, 19_723 * D).unwrap();
        assert_eq!(
            names(got),
            [
                "events.jsonl",
                "events-2023-12-31.jsonl",
                "events-2024-01-01.jsonl",
                "events-2024-01-02.jsonl"
            ]
        );
    }

    #[test]
    fn works_without_the_legacy_file_or_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), &["events-2024-01-01.jsonl"]);
        let base = dir.path().join("events.jsonl");
        assert_eq!(
            names(files_in_range(&base, 0, u64::MAX).unwrap()),
            ["events-2024-01-01.jsonl"]
        );
        assert!(files_in_range(&dir.path().join("no/events.jsonl"), 0, 1)
            .unwrap()
            .is_empty());
    }
}
