//! 作り始めてよい時間帯 (日本時間の「時」の範囲。docs/replay-video.md 5.5 の 3)。
//! 作りかけは時間帯の終わりを過ぎても続ける (始めるときだけ見る)。

const JST_OFFSET_MS: u64 = 9 * 3_600_000;
const MINUTE_MS: u64 = 60_000;

/// `"1-5"` は 1 時から 5 時まで (1:00 以上 5:00 未満)。`"22-5"` のように始めが大きければ日をまたぐ。`"0-24"` なら一日中。
/// `"10-15,21:30-23"` のようにカンマで区切って複数書け、`21:30` のように分も書ける
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hours {
    /// (始め, 終わり) の日本時間の分 (0〜1440)
    ranges: Vec<(u64, u64)>,
}

impl Hours {
    pub fn parse(text: &str) -> anyhow::Result<Hours> {
        let bad = || {
            anyhow::anyhow!("hours は \"1-5\" や \"10-15,21:30-23\" の形 (日本時間、0〜24) で書いてください: {text:?}")
        };
        let minute = |t: &str| -> Option<u64> {
            let (h, m) = t.trim().split_once(':').unwrap_or((t.trim(), "0"));
            let (h, m) = (h.parse::<u64>().ok()?, m.parse::<u64>().ok()?);
            (m < 60 && h * 60 + m <= 1440).then_some(h * 60 + m)
        };
        let ranges = text
            .split(',')
            .map(|r| {
                let (a, b) = r.split_once('-')?;
                let (start, end) = (minute(a)?, minute(b)?);
                (start != end).then_some((start, end))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(bad)?;
        Ok(Hours { ranges })
    }

    /// epoch ミリ秒の今が、時間帯のどれかの中か
    pub fn contains(&self, now_ms: u64) -> bool {
        let m = (now_ms + JST_OFFSET_MS) / MINUTE_MS % 1440;
        self.ranges.iter().any(|&(start, end)| {
            if start < end {
                (start..end).contains(&m)
            } else {
                m >= start || m < end
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-01 の日本時間 h 時 m 分 (epoch ミリ秒)。2026-09-30 15:00 UTC = 10-01 00:00 JST
    fn jst(h: u64, m: u64) -> u64 {
        1_790_780_400_000 + (h * 60 + m) * 60_000
    }

    #[test]
    fn one_to_five_is_one_oclock_up_to_but_not_including_five() {
        let hs = Hours::parse("1-5").unwrap();
        assert!(!hs.contains(jst(0, 59)));
        assert!(hs.contains(jst(1, 0)));
        assert!(hs.contains(jst(4, 59)));
        assert!(!hs.contains(jst(5, 0)));
        assert!(!hs.contains(jst(15, 0)));
    }

    #[test]
    fn a_range_that_wraps_past_midnight_and_the_whole_day() {
        let night = Hours::parse("22-5").unwrap();
        assert!(night.contains(jst(23, 0)) && night.contains(jst(0, 0)) && night.contains(jst(4, 59)));
        assert!(!night.contains(jst(5, 0)) && !night.contains(jst(21, 59)));
        let all = Hours::parse("0-24").unwrap();
        assert!([0, 6, 12, 23].iter().all(|&h| all.contains(jst(h, 30))));
    }

    #[test]
    fn nonsense_is_refused() {
        for t in ["", "5", "a-b", "1-25", "3-3", "-1-5", "1-5,", "21:60-23", "24:30-1"] {
            assert!(Hours::parse(t).is_err(), "{t}");
        }
    }

    #[test]
    fn several_ranges_with_minutes() {
        let hs = Hours::parse("10-15, 21:30-23").unwrap();
        assert!(hs.contains(jst(10, 0)) && hs.contains(jst(14, 59)));
        assert!(!hs.contains(jst(15, 0)) && !hs.contains(jst(21, 29)));
        assert!(hs.contains(jst(21, 30)) && hs.contains(jst(22, 59)));
        assert!(!hs.contains(jst(23, 0)) && !hs.contains(jst(3, 0)));
    }
}
