//! 配信の寄り (zoom)。web/src/camera.ts と scene.ts の renderScene の移植で、純粋な関数だけ。
//! 座標は地図の座標 (geo::project の単位。1 度 ≒ 100 単位なので 1km ≒ 100/111 単位)。時刻は引数。

use super::eew::{surface_radius_km, VS_KM_S, WAVE_MAX_MS};
use super::geo::{home_bounds, project};

pub const KM_TO_UNITS: f64 = 100.0 / 111.0;
/// 最初に寄るときの半径 (km)
pub const MIN_RADIUS_KM: f64 = 80.0;
/// 揺れた地域が分からないときに引く上限 (km)
pub const DEFAULT_STOP_KM: f64 = 300.0;
/// 範囲が目標へ近づく時定数 (ミリ秒。web の map.ts の step)
const TIME_CONSTANT_MS: f64 = 250.0;
/// 時計がこれより飛んだら、近づかずに目標へ移る (記録から描き直すときの飛び越し)
const SNAP_AFTER_MS: u64 = 1_500;

/// 地図の座標の外接矩形
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl MapBox {
    /// 点 (x, y) から r_km の四方
    pub fn around(x: f64, y: f64, r_km: f64) -> MapBox {
        let r = r_km * KM_TO_UNITS;
        MapBox {
            x0: x - r,
            y0: y - r,
            x1: x + r,
            y1: y + r,
        }
    }

    pub fn union(a: Option<MapBox>, b: Option<MapBox>) -> Option<MapBox> {
        match (a, b) {
            (Some(a), Some(b)) => Some(MapBox {
                x0: a.x0.min(b.x0),
                y0: a.y0.min(b.y0),
                x1: a.x1.max(b.x1),
                y1: a.y1.max(b.y1),
            }),
            (a, b) => a.or(b),
        }
    }

    /// 周囲に余白を付け、小さすぎる範囲は 2 x MIN_RADIUS_KM 四方まで広げる
    pub fn pad(self) -> MapBox {
        const RATIO: f64 = 0.15;
        let min = 2.0 * MIN_RADIUS_KM * KM_TO_UNITS;
        let (cx, cy) = ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0);
        let hw = ((self.x1 - self.x0) * (1.0 + RATIO)).max(min) / 2.0;
        let hh = ((self.y1 - self.y0) * (1.0 + RATIO)).max(min) / 2.0;
        MapBox {
            x0: cx - hw,
            y0: cy - hh,
            x1: cx + hw,
            y1: cy + hh,
        }
    }
}

/// 震央から揺れた地域 (の外接矩形) の一番遠い角までの距離 (km)。ここまで引いたら止める
pub fn stop_radius_km(x: f64, y: f64, shaken: Option<MapBox>) -> f64 {
    let Some(b) = shaken else { return DEFAULT_STOP_KM };
    let dx = (b.x0 - x).abs().max((b.x1 - x).abs());
    let dy = (b.y0 - y).abs().max((b.y1 - y).abs());
    (dx.hypot(dy) / KM_TO_UNITS).max(MIN_RADIUS_KM)
}

/// S 波の半径 s_km のときに見せる半径 (km)
pub fn follow_radius_km(s_km: Option<f64>, stop_km: f64) -> f64 {
    s_km.unwrap_or(0.0).max(MIN_RADIUS_KM).min(stop_km)
}

/// 寄りの目標を決める材料
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aim {
    pub epicenter: Option<Epicenter>,
    /// 揺れた範囲 (緊急地震速報なら予想の地域、無ければ県の本土)
    pub shaken: Option<MapBox>,
    /// 緊急地震速報の予想か (揺れる範囲を、波が届く前から収める)
    pub forecast: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Epicenter {
    pub lat: f64,
    pub lon: f64,
    pub depth_km: f64,
    pub origin_ms: Option<i64>,
}

/// いま見せる範囲 (web の renderScene)。None は日本全体
pub fn target_box(aim: &Aim, now_ms: u64) -> Option<MapBox> {
    let Some(c) = aim.epicenter else {
        return aim.shaken.map(MapBox::pad);
    };
    let (x, y) = project(c.lon, c.lat);
    let t_ms = c.origin_ms.map(|o| now_ms as i64 - o);
    let waving = t_ms.is_some_and(|t| t < WAVE_MAX_MS);
    if !waving {
        return MapBox::union(aim.shaken, Some(MapBox::around(x, y, 0.0))).map(MapBox::pad);
    }
    let s_km = t_ms.and_then(|t| surface_radius_km(VS_KM_S, c.depth_km, t as f64 / 1000.0));
    let follow = MapBox::around(x, y, follow_radius_km(s_km, stop_radius_km(x, y, aim.shaken)));
    let shown = if aim.forecast {
        MapBox::union(Some(follow), aim.shaken)
    } else {
        Some(follow)
    };
    shown.map(MapBox::pad)
}

/// 地図の枠の縦横比に合わせた表示範囲 (地図の座標。左上と大きさ)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 地図の枠に box 全体が収まる表示範囲
pub fn fit_box(b: MapBox, aspect: f64) -> Fit {
    let w = (b.x1 - b.x0).max((b.y1 - b.y0) * aspect);
    let h = w / aspect;
    Fit {
        x: (b.x0 + b.x1) / 2.0 - w / 2.0,
        y: (b.y0 + b.y1) / 2.0 - h / 2.0,
        w,
        h,
    }
}

/// 日本全体の表示範囲 (aspect は地図の枠の縦横比)
pub fn home_fit(aspect: f64) -> Fit {
    let (x0, y0, x1, y1) = home_bounds();
    fit_box(MapBox { x0, y0, x1, y1 }, aspect)
}

/// 目標へ指数的に近づける。近ければ目標そのもの (web の map.ts の step)
pub fn approach(cur: Fit, target: Fit, dt_ms: f64) -> Fit {
    let k = 1.0 - (-dt_ms.max(0.0) / TIME_CONSTANT_MS).exp();
    let eps = cur.w * 0.002;
    let far = (target.x - cur.x)
        .abs()
        .max((target.y - cur.y).abs())
        .max((target.w - cur.w).abs())
        .max((target.h - cur.h).abs());
    if far < eps {
        return target;
    }
    let lerp = |a: f64, b: f64| a + (b - a) * k;
    Fit {
        x: lerp(cur.x, target.x),
        y: lerp(cur.y, target.y),
        w: lerp(cur.w, target.w),
        h: lerp(cur.h, target.h),
    }
}

/// 寄りの状態 (コマをまたいで持つ)。時刻はコマの時刻 (記録から描き直すときは仮の時計)
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    cur: Fit,
    /// 日本全体の表示範囲
    home: Fit,
    last_ms: Option<u64>,
}

impl Camera {
    /// aspect は地図の枠の縦横比
    pub fn new(aspect: f64) -> Camera {
        let home = home_fit(aspect);
        Camera {
            cur: home,
            home,
            last_ms: None,
        }
    }

    /// 日本全体の表示範囲
    #[cfg(test)]
    pub fn home(&self) -> Fit {
        self.home
    }

    /// now_ms の表示範囲を進める。target が None なら日本全体へ一気に戻す。
    /// snap か、前のコマから大きく (または戻って) 時計が飛んだときは、近づかずに目標へ移る
    pub fn advance(&mut self, target: Option<Fit>, now_ms: u64, snap: bool) {
        let home = self.home;
        let last = self.last_ms.replace(now_ms);
        // 日本全体より広くは引かない (別枠を出す判断を、日本全体かどうかだけにする)
        let Some(target) = target.map(|t| if t.w >= home.w { home } else { t }) else {
            self.cur = home;
            return;
        };
        let jumped = last.is_none_or(|l| now_ms < l || now_ms - l > SNAP_AFTER_MS);
        self.cur = if snap || jumped {
            target
        } else {
            approach(self.cur, target, (now_ms - last.unwrap_or(now_ms)) as f64)
        };
    }

    pub fn fit(&self) -> Fit {
        self.cur
    }

    /// 日本全体の表示か (別枠を出してよい)
    pub fn is_home(&self) -> bool {
        self.cur.w >= self.home.w * (1.0 - 1e-6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 組み込みの定義の main (900x634) の縦横比
    const ASPECT: f64 = 900.0 / 634.0;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn width_km(b: MapBox) -> f64 {
        (b.x1 - b.x0) / KM_TO_UNITS
    }

    /// 千葉県北東部のあたり (地図の座標)
    fn chiba() -> (f64, f64) {
        project(140.8, 35.7)
    }

    fn epicenter(origin_ms: Option<i64>) -> Option<Epicenter> {
        Some(Epicenter {
            lat: 35.7,
            lon: 140.8,
            depth_km: 40.0,
            origin_ms,
        })
    }

    #[test]
    fn a_padded_box_gets_a_margin_and_never_gets_smaller_than_160km() {
        let tiny = MapBox::around(0.0, 0.0, 0.0).pad();
        assert!(close(width_km(tiny), 160.0) && close((tiny.y1 - tiny.y0) / KM_TO_UNITS, 160.0));
        // 80km の四方 (160km) には 15% の余白が付く
        let b = MapBox::around(5.0, 5.0, 80.0).pad();
        assert!(close(width_km(b), 160.0 * 1.15), "{}", width_km(b));
        assert!(close((b.x0 + b.x1) / 2.0, 5.0));
        // 縦長はそのまま (縦だけ余白が付く)
        let tall = MapBox {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 1000.0,
        }
        .pad();
        assert!(close(tall.y1 - tall.y0, 1150.0) && close(tall.x1 - tall.x0, 160.0 * KM_TO_UNITS));
    }

    #[test]
    fn the_stop_radius_reaches_the_farthest_corner_with_a_floor_and_a_default() {
        let (x, y) = (0.0, 0.0);
        assert_eq!(stop_radius_km(x, y, None), DEFAULT_STOP_KM);
        let far = MapBox {
            x0: -100.0,
            y0: -30.0,
            x1: 50.0,
            y1: 40.0,
        };
        let want = (100.0f64.hypot(40.0)) / KM_TO_UNITS;
        assert!(close(stop_radius_km(x, y, Some(far)), want));
        assert_eq!(stop_radius_km(x, y, Some(MapBox::around(0.0, 0.0, 1.0))), MIN_RADIUS_KM);
    }

    #[test]
    fn the_follow_radius_grows_with_the_s_wave_between_the_floor_and_the_stop() {
        assert_eq!(follow_radius_km(None, 300.0), MIN_RADIUS_KM);
        assert_eq!(follow_radius_km(Some(10.0), 300.0), MIN_RADIUS_KM);
        assert_eq!(follow_radius_km(Some(150.0), 300.0), 150.0);
        assert_eq!(follow_radius_km(Some(900.0), 300.0), 300.0);
    }

    #[test]
    fn the_union_ignores_a_missing_side() {
        let a = MapBox::around(0.0, 0.0, 10.0);
        let b = MapBox::around(100.0, 0.0, 10.0);
        assert_eq!(MapBox::union(None, None), None);
        assert_eq!(MapBox::union(Some(a), None), Some(a));
        let u = MapBox::union(Some(a), Some(b)).unwrap();
        assert!(u.x0 == a.x0 && u.x1 == b.x1);
    }

    #[test]
    fn while_the_waves_spread_the_target_follows_the_s_wave_and_then_settles_on_the_shaken_area() {
        const T0: i64 = 1_790_000_000_000;
        let (x, y) = chiba();
        let shaken = MapBox::around(x + 30.0, y - 20.0, 60.0);
        let aim = |forecast| Aim {
            epicenter: epicenter(Some(T0)),
            shaken: Some(shaken),
            forecast,
        };
        // 発生の 10 秒後: S 波はまだ小さい。最小の半径 (80km) で寄る
        let early = target_box(&aim(false), (T0 + 10_000) as u64).unwrap();
        assert!(close(width_km(early), 160.0 * 1.15), "{}", width_km(early));
        assert!(close((early.x0 + early.x1) / 2.0, x));
        // 60 秒後: S 波は 3.75 x 60 = 225km 近く。揺れた範囲の端 (stop) までで止まる
        let stop = stop_radius_km(x, y, Some(shaken));
        let later = target_box(&aim(false), (T0 + 60_000) as u64).unwrap();
        let s = surface_radius_km(VS_KM_S, 40.0, 60.0).unwrap();
        let r = s.min(stop).max(MIN_RADIUS_KM);
        assert!(close(width_km(later), 2.0 * r * 1.15), "{} {}", width_km(later), r);
        // 緊急地震速報の予想は、揺れる範囲を最初から収める
        let fc = target_box(&aim(true), (T0 + 10_000) as u64).unwrap();
        assert!(fc.x1 >= shaken.x1 && fc.x0 <= shaken.x0 && fc.y0 <= shaken.y0);
        // 180 秒を過ぎたら、揺れた範囲と震央を収めて止まる
        let end = target_box(&aim(false), (T0 + 181_000) as u64).unwrap();
        let want = MapBox::union(Some(shaken), Some(MapBox::around(x, y, 0.0)))
            .unwrap()
            .pad();
        assert_eq!(end, want);
    }

    #[test]
    fn without_an_epicenter_the_shaken_area_or_the_whole_country_is_shown() {
        let b = MapBox::around(0.0, 0.0, 100.0);
        let aim = |shaken| Aim {
            epicenter: None,
            shaken,
            forecast: false,
        };
        assert_eq!(target_box(&aim(Some(b)), 0), Some(b.pad()));
        assert_eq!(target_box(&aim(None), 0), None);
        // 発生時刻が分からない震央は、波を追わず、震央と揺れた範囲を収める
        let (x, y) = chiba();
        let a = Aim {
            epicenter: epicenter(None),
            shaken: None,
            forecast: false,
        };
        assert_eq!(target_box(&a, 5), Some(MapBox::around(x, y, 0.0).pad()));
    }

    #[test]
    fn a_box_is_fitted_to_the_aspect_of_the_map_rect_around_its_center() {
        let f = fit_box(
            MapBox {
                x0: 0.0,
                y0: 0.0,
                x1: 100.0,
                y1: 10.0,
            },
            2.0,
        );
        assert!(close(f.w, 100.0) && close(f.h, 50.0) && close(f.x, 0.0) && close(f.y, -20.0));
        let tall = fit_box(
            MapBox {
                x0: 0.0,
                y0: 0.0,
                x1: 10.0,
                y1: 100.0,
            },
            2.0,
        );
        assert!(close(tall.h, 200.0 / 2.0) && close(tall.w, 200.0) && close(tall.x, -95.0));
        let home = home_fit(ASPECT);
        assert!(close(home.w / home.h, ASPECT));
    }

    #[test]
    fn the_view_approaches_the_target_exponentially_and_snaps_when_near() {
        let cur = Fit {
            x: 0.0,
            y: 0.0,
            w: 1000.0,
            h: 500.0,
        };
        let target = Fit {
            x: 100.0,
            y: 50.0,
            w: 200.0,
            h: 100.0,
        };
        assert_eq!(approach(cur, target, 0.0), cur);
        let one = approach(cur, target, 250.0); // 時定数 1 つ分で 1 - 1/e だけ近づく
        let k = 1.0 - (-1.0f64).exp();
        assert!(close(one.x, 100.0 * k) && close(one.w, 1000.0 + (200.0 - 1000.0) * k));
        let many = (0..40).fold(cur, |c, _| approach(c, target, 200.0));
        assert_eq!(many, target); // 近づききったら目標そのもの
    }

    #[test]
    fn the_camera_snaps_on_the_first_frame_a_jump_and_a_return_home() {
        let target = fit_box(MapBox::around(0.0, 0.0, 80.0).pad(), ASPECT);
        let mut c = Camera::new(ASPECT);
        assert!(c.is_home());
        c.advance(Some(target), 10_000, false); // 最初のコマは前が無いので、そのまま目標
        assert_eq!(c.fit(), target);
        assert!(!c.is_home());
        // 同じ時刻・近い時刻では、近づく (別の目標へ)
        let other = fit_box(MapBox::around(500.0, 0.0, 80.0).pad(), ASPECT);
        c.advance(Some(other), 10_200, false);
        assert!(c.fit() != other && c.fit() != target && c.fit().x > target.x);
        // 大きく時計が飛んだ・戻った・snap のときは目標へ
        c.advance(Some(other), 10_200 + 5_000, false);
        assert_eq!(c.fit(), other);
        c.advance(Some(target), 100, false);
        assert_eq!(c.fit(), target);
        c.advance(Some(other), 200, true);
        assert_eq!(c.fit(), other);
        // None (平時) は日本全体へ一気に戻る
        c.advance(None, 300, false);
        assert_eq!(c.fit(), home_fit(ASPECT));
        assert!(c.is_home());
    }
}
