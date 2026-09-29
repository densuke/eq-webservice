//! 平時の画面の地図の中身: 警報・注意報の塗り、雨の点、主要都市の天気の札。
//! 本図にも別枠 (frame) にも同じように描く。

use std::collections::HashMap;

use tiny_skia::{Pixmap, PixmapPaint, Transform};

use super::data::{city_side, rain_color, temp_label, top_level, warning_fill, weather_char, City, Side};
use super::draw::Scene;
use super::frame::Frame;
use super::geo::Shape;
use super::icon;
use super::paint::{circle, rrect, LAND_EDGE};
use super::text::Text;

pub fn draw(pm: &mut Pixmap, text: &mut Text, areas: &HashMap<String, Shape>, frame: &Frame, scene: &Scene) {
    if let Some(w) = scene.warnings {
        for (code, kinds) in &w.areas {
            let (Some(level), Some(shape)) = (top_level(kinds), areas.get(code)) else {
                continue;
            };
            let (c, a) = warning_fill(level);
            frame.fill(pm, &shape.path, c, a);
            frame.stroke(pm, &shape.path, [0, 0, 0], 0.35, 0.5);
        }
    }
    let Some(w) = scene.weather else { return };
    // 雨の強い地点ほど上に
    let mut rain: Vec<_> = w.rain.iter().filter(|r| frame.contains(r[1], r[0])).collect();
    rain.sort_by(|a, b| a[2].total_cmp(&b[2]));
    for &[lat, lon, mm] in rain {
        let (x, y) = frame.view.px(lon, lat);
        circle(pm, x, y, 2.5, rain_color(mm), 1.0);
    }
    for c in w.cities.iter().filter(|c| frame.contains(c.lon, c.lat)) {
        draw_city(pm, text, frame, scene, c);
    }
}

/// 都市の点と、天気のアイコン (取れていなければ漢字 1 文字) と気温の札
fn draw_city(pm: &mut Pixmap, text: &mut Text, frame: &Frame, scene: &Scene, c: &City) {
    let (x, y) = frame.view.px(c.lon, c.lat);
    circle(pm, x, y, 3.0, [255, 255, 255], 1.0);
    let temp = temp_label(c.temp);
    let temp_w = temp.chars().count() as f32 * 8.0;
    let icon = icon::name_for(&c.code, scene.now_ms).and_then(|n| scene.icons.get(n));
    let bw = match icon {
        Some(ic) => 8.0 + ic.width() as f32 + temp_w,
        None => 22.0 + temp_w,
    };
    let (bx, by) = match city_side(&c.name) {
        Side::Up => (-bw / 2.0, -30.0),
        Side::Down => (-bw / 2.0, 8.0),
        Side::Left => (-bw - 7.0, -11.0),
        Side::Right => (7.0, -11.0),
    };
    let (left, top) = (x + bx, y + by);
    rrect(pm, left, top, bw, 22.0, 11.0, [240, 244, 248], 0.92);
    match icon {
        Some(ic) => {
            let at = ((left + 5.0).round() as i32, (top + 2.0).round() as i32);
            pm.draw_pixmap(
                at.0,
                at.1,
                ic.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            text.draw(
                pm,
                &temp,
                left + 5.0 + ic.width() as f32 + 3.0,
                top + 16.0,
                13.0,
                LAND_EDGE,
            );
        }
        None => {
            let label: String = weather_char(&c.code).into_iter().collect::<String>() + &temp;
            text.draw_center(pm, &label, left + bw / 2.0, top + 16.0, 13.0, LAND_EDGE);
        }
    }
}
