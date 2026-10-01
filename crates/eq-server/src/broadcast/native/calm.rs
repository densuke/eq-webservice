//! 平時の画面の地図の中身: 警報・注意報の塗り、雨の点、主要都市の天気の札。
//! 本図にも別枠 (frame) にも同じように描く。

use std::collections::HashMap;

use tiny_skia::{Pixmap, PixmapPaint, Transform};

use super::data::{
    city_side, city_side_tomorrow, rain_color, range_label, temp_label, top_level, warning_fill, weather_caption,
    weather_char, City, Side, Tomorrow, WeatherView,
};
use super::draw::{Scene, INSET_LINE};
use super::frame::{BoxRect, Frame};
use super::geo::Shape;
use super::icon;
use super::paint::{circle, rrect, LAND_EDGE, PANEL, TEXT};
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
    let view = w.view(scene.now_ms, scene.flip_s);
    for c in w.cities.iter().filter(|c| frame.contains(c.lon, c.lat)) {
        draw_city(pm, text, frame, scene, c, view);
    }
    if !frame.is_inset() {
        draw_info_window(pm, text, &weather_caption(view, scene.now_ms));
    }
}

/// 情報の窓 (x, y, 幅, 高さ)。日本海の北の空いた海 (別枠の右・北海道の左・警報の帯の下)。
/// 陸・別枠・警報の帯と重ならないことはテストで確かめる
pub(super) const INFO_WINDOW: BoxRect = (274.0, 198.0, 242.0, 44.0);
const INFO_PX: f32 = 22.0;

/// 今何を出しているか (今の天気 / 明日の天気) の案内を、情報の窓に出す。
/// 窓は文字が描けなくても出す。中身の文 (caption) は窓とは別に決める
fn draw_info_window(pm: &mut Pixmap, text: &mut Text, caption: &str) {
    let (x, y, w, h) = INFO_WINDOW;
    rrect(pm, x, y, w, h, 8.0, INSET_LINE, 1.0);
    rrect(pm, x + 1.0, y + 1.0, w - 2.0, h - 2.0, 7.0, PANEL, 0.9);
    text.draw_fit(
        pm,
        caption,
        x + 12.0,
        y + h / 2.0 + INFO_PX * 0.35,
        INFO_PX,
        TEXT,
        w - 24.0,
    );
}

/// 札の左上 (点 (x, y) からの向き side と、札の幅・高さで決める)
fn card_origin(side: Side, x: f32, y: f32, bw: f32, bh: f32) -> (f32, f32) {
    let (bx, by) = match side {
        Side::Up => (-bw / 2.0, -8.0 - bh),
        Side::Down => (-bw / 2.0, 8.0),
        Side::Left => (-bw - 7.0, -bh / 2.0),
        Side::Right => (7.0, -bh / 2.0),
    };
    (x + bx, y + by)
}

/// 都市の点と、天気のアイコン (取れていなければ漢字 1 文字) と気温の札。
/// 明日の番 (view) で、明日の予報があれば明日の札にする
fn draw_city(pm: &mut Pixmap, text: &mut Text, frame: &Frame, scene: &Scene, c: &City, view: WeatherView) {
    let (x, y) = frame.view.px(c.lon, c.lat);
    circle(pm, x, y, 3.0, [255, 255, 255], 1.0);
    if let Some(t) = c.tomorrow.as_ref().filter(|_| view == WeatherView::Tomorrow) {
        return draw_tomorrow(pm, text, scene, c, t, (x, y));
    }
    let temp = temp_label(c.temp);
    let temp_w = temp.chars().count() as f32 * 8.0;
    let icon = icon::name_for(&c.code, scene.now_ms).and_then(|n| scene.icons.get(n));
    let bw = match icon {
        Some(ic) => 8.0 + ic.width() as f32 + temp_w,
        None => 22.0 + temp_w,
    };
    let (left, top) = card_origin(city_side(&c.name), x, y, bw, 22.0);
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

/// 明日の札 (幅を抑えるため 2 段): 上に天気のアイコン (無ければ漢字 1 文字) と最高/最低気温、
/// 下に小さい降水確率 (無ければ 1 段)。明日は昼の予報なので昼のアイコンを使う
fn draw_tomorrow(pm: &mut Pixmap, text: &mut Text, scene: &Scene, c: &City, t: &Tomorrow, (x, y): (f32, f32)) {
    let icon = icon::names(&t.code).and_then(|(day, _)| scene.icons.get(day));
    let kanji: String = weather_char(&t.code).into_iter().collect();
    let range = range_label(t.temp_max, t.temp_min);
    let pop = t.pop.map(|p| format!("{p}%")).unwrap_or_default();
    let h = if pop.is_empty() { 22.0 } else { 32.0 };
    let (icon_w, range_w) = (
        icon.map_or(text.width(&kanji, 13.0), |ic| ic.width() as f32),
        text.width(&range, 12.0),
    );
    let bw = 10.0 + icon_w + 3.0 + range_w;
    let (left, top) = card_origin(city_side_tomorrow(&c.name), x, y, bw, h);
    rrect(
        pm,
        left,
        top,
        bw,
        h,
        if pop.is_empty() { 11.0 } else { 10.0 },
        [240, 244, 248],
        0.92,
    );
    let mut at = left + 5.0;
    match icon {
        Some(ic) => {
            let pos = (at.round() as i32, (top + 2.0).round() as i32);
            pm.draw_pixmap(
                pos.0,
                pos.1,
                ic.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            at += icon_w + 3.0;
        }
        None => at += text.draw(pm, &kanji, at, top + 15.0, 13.0, LAND_EDGE) + 3.0,
    }
    text.draw(pm, &range, at, top + 15.0, 12.0, LAND_EDGE);
    text.draw_center(pm, &pop, left + bw / 2.0, top + 28.0, 11.0, [30, 90, 170]);
}
