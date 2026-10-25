//! 平時に危険警報以上の県へ毎分の終わりに寄せる (実際の地図データで。文字は描かない設定)。
//! 2026-10-25 の和歌山県の土砂災害危険警報 (有田市など 5 区域) を使う

use super::data::{Kind, Warnings};
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
    let icons = Icons::new();
    for k in 0..n {
        st.step(&Input {
            warnings: Some(w),
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
    run(&mut st, &wakayama(), MINUTE + 46_000, 30);
    st.still_pixmap()
        .unwrap()
        .save_png(std::path::Path::new(&out).join("warn_zoom.png"))
        .unwrap();
}
