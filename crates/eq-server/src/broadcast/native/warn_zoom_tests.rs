//! 平時に危険警報以上の県へ毎分の終わりに寄せる (実際の地図データで。文字は描かない設定)。
//! 2026-10-25 の和歌山県の土砂災害危険警報 (有田市など 5 区域) を使う

use super::data::{rain_color, CityWeather, Kind, Warnings};
use super::step::Stepper;
use super::zoom_tests::{config_sub, input};
use super::*;

/// 2026-10-25 12:00:00 JST (分のはじめ)
const MINUTE: u64 = 1_792_896_000_000;

fn wakayama() -> Warnings {
    let codes = ["3020400", "3030400", "3036100", "3036601", "3036602"];
    Warnings {
        areas: codes
            .iter()
            .map(|c| {
                (
                    c.to_string(),
                    vec![
                        Kind {
                            name: "レベル４土砂災害危険警報".into(),
                        },
                        Kind {
                            name: "レベル３大雨警報".into(),
                        },
                    ],
                )
            })
            .collect(),
        names: [("3020400", "有田市"), ("3030400", "紀美野町"), ("3036100", "有田川町")]
            .iter()
            .map(|(c, n)| (c.to_string(), n.to_string()))
            .collect(),
    }
}

/// from から 200ms ごとに n コマ進める
fn run(st: &mut Stepper, w: &Warnings, from: u64, n: u64) {
    run_with(st, w, None, from, n);
}

fn run_with(st: &mut Stepper, w: &Warnings, weather: Option<&CityWeather>, from: u64, n: u64) {
    let icons = Icons::new();
    for k in 0..n {
        st.step(&Input {
            warnings: Some(w),
            weather,
            ..input(&[], from + k * 200, &icons)
        });
    }
}

#[test]
fn the_map_zooms_on_a_danger_warning_at_the_end_of_each_minute_and_comes_back() {
    let mut st = Stepper::new(load_renderer(&config_sub(true, false)).unwrap());
    let w = wakayama();
    run(&mut st, &w, MINUTE + 30_000, 10);
    assert_eq!(st.zoom_ratio(), 1.0);
    // 毎分の後ろ 15 秒は和歌山県へ寄る (400 km 四方ほど)
    run(&mut st, &w, MINUTE + 46_000, 20);
    assert!(st.zoom_ratio() > 2.0, "{}", st.zoom_ratio());
    // 次の分のはじめには日本全体へ戻る
    run(&mut st, &w, MINUTE + 61_000, 20);
    assert_eq!(st.zoom_ratio(), 1.0);
    // 寄りの設定が無効なら寄せない
    let mut off = Stepper::new(load_renderer(&config_sub(false, false)).unwrap());
    run(&mut off, &w, MINUTE + 46_000, 20);
    assert_eq!(off.zoom_ratio(), 1.0);
}

#[test]
fn write_warn_zoom_png_when_asked() {
    let Ok(out) = std::env::var("EQ_NATIVE_PNG_DIR") else {
        return;
    };
    let mut cfg = config_sub(true, false);
    cfg.font = std::env::var("EQ_NATIVE_FONT").unwrap_or(cfg.font);
    let mut st = Stepper::new(load_renderer(&cfg).unwrap());
    let mut w = wakayama();
    w.names.extend(
        [("3036601", "有田川町吉備金屋"), ("3036602", "有田川町清水")].map(|(c, n)| (c.to_string(), n.to_string())),
    );
    let weather = CityWeather {
        cities: vec![],
        rain: vec![[34.1, 135.2, 12.0], [37.2, 135.3, 80.0], [34.3, 139.5, 80.0]],
    };
    run_with(&mut st, &w, Some(&weather), MINUTE + 46_000, 30);
    st.still_pixmap()
        .unwrap()
        .save_png(std::path::Path::new(&out).join("warn_zoom.png"))
        .unwrap();
}

#[test]
fn rain_dots_outside_the_zoomed_map_are_not_drawn_on_the_bar_or_the_side_panel() {
    let mut st = Stepper::new(load_renderer(&config_sub(true, false)).unwrap());
    // 寄った地図の外: 北 (上部バー・警報の帯の位置) と東 (右パネルの位置) の強い雨
    let weather = CityWeather {
        cities: vec![],
        rain: vec![[37.2, 135.3, 80.0], [34.3, 139.5, 80.0], [35.6, 139.7, 80.0]],
    };
    run_with(&mut st, &wakayama(), Some(&weather), MINUTE + 46_000, 30);
    assert!(st.zoom_ratio() > 2.0);
    let pm = st.still_pixmap().unwrap();
    let placed = super::placed::Placed::builtin().unwrap();
    let main = placed.main;
    let c = rain_color(80.0);
    let outside = pm.pixels().iter().enumerate().any(|(i, p)| {
        let (x, y) = ((i as u32 % pm.width()) as f32, (i as u32 / pm.width()) as f32);
        let in_map = x >= main.x && x < main.x + main.w && y >= main.y && y < main.y + main.h;
        !in_map && [p.red(), p.green(), p.blue()] == c
    });
    assert!(!outside, "a rain dot was drawn outside the map");
}
