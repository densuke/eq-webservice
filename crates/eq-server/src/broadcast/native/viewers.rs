//! 上部バーの右に出す「同接 N 人」。取った値の整形・古さの判断・右側の並べ方 (BGM・状態の札とあわせて)。

use serde::Deserialize;

/// これより古い値は出さない (web の viewers.ts と同じ 5 分)
pub const STALE_MS: u64 = 5 * 60_000;
/// 項目のあいだ
pub const GAP: f32 = 16.0;
/// 左の表示 ([テスト] まで) がここまで来ている。右の項目の左端はここより右に置く
pub const LEFT_LIMIT: f32 = 340.0;

/// GET /api/viewers の応答
#[derive(Debug, Deserialize)]
pub struct Wire {
    pub viewers: Option<u64>,
    #[serde(default)]
    pub updated_ms: u64,
}

/// 取れた値。updated_ms はサーバの時計、fetched_ms は自分の時計 (取った時刻)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub count: u64,
    pub updated_ms: u64,
    pub fetched_ms: u64,
}

impl Sample {
    /// 値が null の応答は None (出さない)
    pub fn from_wire(w: &Wire, fetched_ms: u64) -> Option<Sample> {
        Some(Sample {
            count: w.viewers?,
            updated_ms: w.updated_ms,
            fetched_ms,
        })
    }
}

/// 出す人数。サーバの値が古いとき、または取りに行けない状態が続いて手元の値が古いときは None
pub fn visible(s: Option<Sample>, server_now_ms: u64, local_now_ms: u64) -> Option<u64> {
    let s = s?;
    let fresh =
        server_now_ms.saturating_sub(s.updated_ms) <= STALE_MS && local_now_ms.saturating_sub(s.fetched_ms) <= STALE_MS;
    fresh.then_some(s.count)
}

/// 「同接 1,234 人」
pub fn label(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("同接 {out} 人")
}

/// 右から BGM・同接・札の順に並べたときの、それぞれの右端 (出さないものは None)
#[derive(Debug, PartialEq)]
pub struct Arranged {
    pub bgm: Option<f32>,
    pub viewers: Option<f32>,
    pub chip: Option<f32>,
}

/// 各項目の幅 (出さなければ None) から並べる。right は右端の表示 (配信元の名前) の左の端。
/// 左の表示にかかるときは、BGM、同接の順にあきらめる (札は常に出す)。
/// 同接も札も無いときは、BGM は長くても出す
pub fn arrange(right: f32, bgm_w: Option<f32>, viewers_w: Option<f32>, chip_w: Option<f32>) -> Arranged {
    if viewers_w.is_none() && chip_w.is_none() {
        return Arranged {
            bgm: bgm_w.map(|_| right),
            viewers: None,
            chip: None,
        };
    }
    let place = |bgm: bool, viewers: bool| {
        let (mut x, mut left) = (right, right);
        let mut put = |w: Option<f32>, on: bool| {
            let w = w.filter(|_| on)?;
            let at = x;
            x -= w + GAP;
            left = at - w;
            Some(at)
        };
        let a = Arranged {
            bgm: put(bgm_w, bgm),
            viewers: put(viewers_w, viewers),
            chip: put(chip_w, true),
        };
        (a, left)
    };
    for (bgm, viewers) in [(true, true), (false, true)] {
        let (a, left) = place(bgm, viewers);
        if left >= LEFT_LIMIT {
            return a;
        }
    }
    place(false, false).0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(count: u64, updated: u64, fetched: u64) -> Option<Sample> {
        Some(Sample {
            count,
            updated_ms: updated,
            fetched_ms: fetched,
        })
    }

    #[test]
    fn the_label_groups_thousands() {
        assert_eq!(label(0), "同接 0 人");
        assert_eq!(label(999), "同接 999 人");
        assert_eq!(label(1000), "同接 1,000 人");
        assert_eq!(label(1234567), "同接 1,234,567 人");
    }

    #[test]
    fn a_null_response_is_not_a_sample() {
        let w = |v| Wire {
            viewers: v,
            updated_ms: 5,
        };
        assert_eq!(Sample::from_wire(&w(None), 9), None);
        assert_eq!(Sample::from_wire(&w(Some(3)), 9), sample(3, 5, 9));
    }

    #[test]
    fn old_values_are_hidden() {
        let t = 10_000_000;
        assert_eq!(visible(None, t, t), None);
        assert_eq!(visible(sample(7, t, t), t, t), Some(7));
        // ちょうど 5 分は出す、超えたら出さない (サーバの値)
        assert_eq!(visible(sample(7, t - STALE_MS, t), t, t), Some(7));
        assert_eq!(visible(sample(7, t - STALE_MS - 1, t), t, t), None);
        // 取りに行けない状態が続いて手元の値が古い
        assert_eq!(visible(sample(7, t, t - STALE_MS), t, t), Some(7));
        assert_eq!(visible(sample(7, t, t - STALE_MS - 1), t, t), None);
        // サーバの時計が先に進んでいる (updated が未来) でも出す
        assert_eq!(visible(sample(7, t + 5_000, t), t, t), Some(7));
    }

    #[test]
    fn viewers_sit_left_of_the_bgm_and_the_chip_left_of_them() {
        let r = 1264.0;
        assert_eq!(
            arrange(r, Some(200.0), None, None),
            Arranged {
                bgm: Some(r),
                viewers: None,
                chip: None
            }
        );
        // 同接だけ: 右端寄せ
        assert_eq!(
            arrange(r, None, Some(80.0), None),
            Arranged {
                bgm: None,
                viewers: Some(r),
                chip: None
            }
        );
        let a = arrange(r, Some(200.0), Some(80.0), None);
        assert_eq!(
            a,
            Arranged {
                bgm: Some(r),
                viewers: Some(r - 200.0 - GAP),
                chip: None
            }
        );
        let a = arrange(r, Some(200.0), Some(80.0), Some(250.0));
        assert_eq!(a.chip, Some(r - 200.0 - GAP - 80.0 - GAP));
    }

    #[test]
    fn a_long_bgm_gives_way_to_the_viewers_and_the_chip() {
        let r = 1264.0;
        // 同接があって BGM が長い: BGM を出さない
        assert_eq!(
            arrange(r, Some(900.0), Some(80.0), None),
            Arranged {
                bgm: None,
                viewers: Some(r),
                chip: None
            }
        );
        // 札があって BGM が長い: 札と同接
        let a = arrange(r, Some(700.0), Some(80.0), Some(250.0));
        assert_eq!(
            a,
            Arranged {
                bgm: None,
                viewers: Some(r),
                chip: Some(r - 80.0 - GAP)
            }
        );
        // 札だけで BGM が長い (従来どおり)
        assert_eq!(
            arrange(r, Some(700.0), None, Some(250.0)),
            Arranged {
                bgm: None,
                viewers: None,
                chip: Some(r)
            }
        );
        // 札・同接のどちらも無ければ、長い BGM もそのまま出す
        assert_eq!(arrange(r, Some(2000.0), None, None).bgm, Some(r));
        // 右が狭く同接も入らない: 札だけ
        assert_eq!(
            arrange(500.0, None, Some(80.0), Some(250.0)),
            Arranged {
                bgm: None,
                viewers: None,
                chip: Some(500.0)
            }
        );
    }
}
