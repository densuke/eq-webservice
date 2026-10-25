//! 左下の凡例: 平時に雨の点が出ている間は雨の凡例に替わる

use super::data::{rain_color, CityWeather};
use super::placed::Placed;
use super::tests::{renderer, scene};
use super::text::Text;

/// 凡例の上から i 段目の色の画素
fn row_px(pm: &tiny_skia::Pixmap, i: usize) -> [u8; 3] {
    let l = Placed::builtin().unwrap().legend.unwrap();
    let p = pm
        .pixel((l.x + 10.0) as u32, (l.y + 6.0 + i as f32 * 15.0 + 7.0) as u32)
        .unwrap();
    [p.red(), p.green(), p.blue()]
}

#[test]
fn the_legend_switches_to_rain_while_rain_dots_are_shown() {
    let mut r = renderer(Text::none());
    let dry = CityWeather {
        cities: vec![],
        rain: vec![],
    };
    let wet = CityWeather {
        cities: vec![],
        rain: vec![[35.0, 137.0, 12.0]],
    };
    // 震度の凡例: 上から 7, 6+, ...
    let pm = r.render(&scene(None, &[], None, Some(&dry)));
    assert_eq!(row_px(&pm, 0), [0xb4, 0x00, 0x68]);
    assert_eq!(row_px(&pm, 1), [0xa5, 0x00, 0x21]);
    // 雨の凡例: 1 段目は単位、上から 80, 50, ... <1
    let pm = r.render(&scene(None, &[], None, Some(&wet)));
    assert_eq!(row_px(&pm, 1), rain_color(80.0));
    assert_eq!(row_px(&pm, 2), rain_color(50.0));
    assert_eq!(row_px(&pm, 8), rain_color(0.0));
}
