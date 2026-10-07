//! 平時の画面の地図の中身: 警報・注意報の塗り、雨の点、主要都市の天気の札。
//! 本図にも別枠 (frame) にも同じように描く。

use std::collections::HashMap;

use tiny_skia::{Pixmap, PixmapPaint, Transform};

use super::cards::{place, signature, Card, CardCache, Placed, Zones};
use super::data::{
    city_side, city_side_tomorrow, rain_color, range_label, temp_label, top_level, warning_fill, weather_caption,
    weather_char, City, Side, Tomorrow, WarningLevel, Warnings, WeatherView,
};
use super::draw::{Scene, Target, INSET_LINE};
use super::frame::{BoxRect, Frame};
use super::geo::Shape;
use super::icon;
use super::layout_resolve::Rect;
use super::paint::{circle, line, rrect, LAND_EDGE, PANEL, TEXT};
use super::text::Text;

/// 札の置き場所を決めるための、面ごとの材料
pub struct CardEnv<'a> {
    /// 面の番号 (置き場所を覚える入れ物の見出し)
    pub slot: usize,
    /// 陸の判定に使う都道府県
    pub prefs: &'a [Shape],
    /// 札を置かない所 (凡例・別枠・案内の窓)
    pub fixed: &'a [BoxRect],
    /// 札を置いてよい範囲 (地図の枠から、警報の帯・テストの帯を除いたもの)
    pub bounds: BoxRect,
    /// 情報の窓の枠 (info_window)。本図だけが描く
    pub info: BoxRect,
}

/// 札を置いてよい範囲 (本図 = 定義の main の矩形)。警報の帯は main の外 (定義の banners) なので、
/// 上はテスト配信の帯の下まで
pub fn main_bounds(test: bool, main: Rect) -> BoxRect {
    let band = if test { super::test_mark::BAND_H } else { 0.0 };
    let top = main.y + band;
    (main.x, top, main.w, main.bottom() - band - top)
}

pub fn draw(
    t: &mut Target,
    text: &mut Text,
    areas: &HashMap<String, Shape>,
    frame: &Frame,
    scene: &Scene,
    env: &CardEnv,
    cache: &mut CardCache,
) {
    if let Some(w) = scene.warnings {
        for (code, kinds) in &w.areas {
            let (Some(level), Some(shape)) = (top_level(kinds), areas.get(code)) else {
                continue;
            };
            let (c, a) = warning_fill(level);
            frame.fill(t.low(), &shape.path, c, a);
            frame.stroke(t.low(), &shape.path, [0, 0, 0], 0.35, 0.5);
        }
    }
    let Some(w) = scene.weather else { return };
    // 雨の点と天気の札は波の円より上
    let pm = t.top();
    // 雨の強い地点ほど上に
    let mut rain: Vec<_> = w.rain.iter().filter(|r| frame.contains(r[1], r[0])).collect();
    rain.sort_by(|a, b| a[2].total_cmp(&b[2]));
    for &[lat, lon, mm] in rain {
        let (x, y) = frame.view.px(lon, lat);
        circle(pm, x, y, 2.5, rain_color(mm), 1.0);
    }
    let view = w.view(scene.now_ms, scene.flip_s);
    let tomorrow = view == WeatherView::Tomorrow;
    let cities: Vec<&City> = w.cities.iter().filter(|c| frame.contains(c.lon, c.lat)).collect();
    let cards: Vec<Card> = cities
        .iter()
        .map(|c| card_of(text, frame, scene, c, tomorrow))
        .collect();
    let placed = match scene.warnings {
        Some(ws) => placements(frame, ws, areas, env, cache, &cards, tomorrow),
        None => cards
            .iter()
            .map(|c| Placed {
                rect: c.rect,
                leader: None,
            })
            .collect(),
    };
    for (c, p) in cities.iter().zip(&placed) {
        draw_city(pm, text, frame, scene, c, view, p);
    }
    if !frame.is_inset() {
        draw_info_window(pm, text, env.info, &weather_caption(view, scene.now_ms));
    }
}

/// 警報以上 (注意報は含めない) の区域
fn warned_codes(ws: &Warnings) -> Vec<&str> {
    ws.areas
        .iter()
        .filter(|(_, kinds)| top_level(kinds).is_some_and(|l| l > WarningLevel::Advisory))
        .map(|(code, _)| code.as_str())
        .collect()
}

/// 札の置き場所。警報以上が無ければ元の位置のまま。警報・札・面が変わらない間は覚えた置き場所を使う
/// (平時の画面は低い fps で回り続けるので、毎コマは置き直さない)
fn placements(
    frame: &Frame,
    ws: &Warnings,
    areas: &HashMap<String, Shape>,
    env: &CardEnv,
    cache: &mut CardCache,
    cards: &[Card],
    tomorrow: bool,
) -> Vec<Placed> {
    let codes = warned_codes(ws);
    if codes.is_empty() {
        return cards
            .iter()
            .map(|c| Placed {
                rect: c.rect,
                leader: None,
            })
            .collect();
    }
    let key = signature(codes.iter().copied(), cards, env.fixed, env.bounds);
    cache
        .get((env.slot, tomorrow), key, || {
            let warn = Zones::new(frame, codes.iter().filter_map(|c| areas.get(*c)));
            let land = Zones::new(frame, env.prefs.iter());
            place(cards, env.fixed, env.bounds, &|r| warn.hit(r), &|r| land.hit(r))
        })
        .to_vec()
}

/// 情報の窓 (x, y, 幅, 高さ)。日本海の北の空いた海 (別枠の右・北海道の左)。地図の枠 main の原点からの位置で、
/// 日本全体の地図の地理 (main の高さで縮尺が決まる) に合わせてある。陸・別枠と重ならないことはテストで確かめる
pub(super) fn info_window(main: Rect) -> BoxRect {
    (main.x + 270.0, main.y + 154.0, 242.0, 44.0)
}
const INFO_PX: f32 = 22.0;

/// 今何を出しているか (今の天気 / 明日の天気) の案内を、情報の窓に出す。
/// 窓は文字が描けなくても出す。中身の文 (caption) は窓とは別に決める
fn draw_info_window(pm: &mut Pixmap, text: &mut Text, at: BoxRect, caption: &str) {
    let (x, y, w, h) = at;
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

/// 札の大きさ (幅, 高さ)。明日の札は 2 段 (降水確率があるとき) で、今より幅も高さもある
fn card_size(text: &mut Text, scene: &Scene, c: &City, tomorrow: Option<&Tomorrow>) -> (f32, f32) {
    match tomorrow {
        Some(t) => {
            let icon = icon::names(&t.code).and_then(|(day, _)| scene.icons.get(day));
            let kanji: String = weather_char(&t.code).into_iter().collect();
            let range = range_label(t.temp_max, t.temp_min);
            let h = if t.pop.is_some() { 32.0 } else { 22.0 };
            let icon_w = icon.map_or(text.width(&kanji, 13.0), |ic| ic.width() as f32);
            (10.0 + icon_w + 3.0 + text.width(&range, 12.0), h)
        }
        None => {
            let temp_w = temp_label(c.temp).chars().count() as f32 * 8.0;
            let icon = icon::name_for(&c.code, scene.now_ms).and_then(|n| scene.icons.get(n));
            (icon.map_or(22.0, |ic| 8.0 + ic.width() as f32) + temp_w, 22.0)
        }
    }
}

/// 明日の番で、明日の予報がある都市だけ、その予報
fn tomorrow_of(c: &City, tomorrow: bool) -> Option<&Tomorrow> {
    c.tomorrow.as_ref().filter(|_| tomorrow)
}

/// 札の元の位置の四角と、都市の点
fn card_of(text: &mut Text, frame: &Frame, scene: &Scene, c: &City, tomorrow: bool) -> Card {
    let (x, y) = frame.view.px(c.lon, c.lat);
    let t = tomorrow_of(c, tomorrow);
    let (bw, bh) = card_size(text, scene, c, t);
    let side = if t.is_some() {
        city_side_tomorrow(&c.name)
    } else {
        city_side(&c.name)
    };
    let (left, top) = card_origin(side, x, y, bw, bh);
    Card {
        rect: (left, top, bw, bh),
        dot: (x, y),
    }
}

/// 都市の点と、天気のアイコン (取れていなければ漢字 1 文字) と気温の札。
/// 明日の番 (view) で、明日の予報があれば明日の札にする。札を動かしたときは、点から札の縁へ細い線を引く
fn draw_city(pm: &mut Pixmap, text: &mut Text, frame: &Frame, scene: &Scene, c: &City, view: WeatherView, p: &Placed) {
    let (x, y) = frame.view.px(c.lon, c.lat);
    if let Some(to) = p.leader {
        // web の .city-leader と同じ色・太さ
        line(pm, (x, y), to, 1.2, [240, 244, 248], 0.85);
    }
    circle(pm, x, y, 3.0, [255, 255, 255], 1.0);
    let (left, top, bw, bh) = p.rect;
    match tomorrow_of(c, view == WeatherView::Tomorrow) {
        Some(t) => draw_tomorrow(pm, text, scene, t, (left, top, bw, bh)),
        None => draw_now(pm, text, scene, c, (left, top, bw, bh)),
    }
}

fn draw_now(pm: &mut Pixmap, text: &mut Text, scene: &Scene, c: &City, (left, top, bw, _): BoxRect) {
    let temp = temp_label(c.temp);
    let icon = icon::name_for(&c.code, scene.now_ms).and_then(|n| scene.icons.get(n));
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
fn draw_tomorrow(pm: &mut Pixmap, text: &mut Text, scene: &Scene, t: &Tomorrow, (left, top, bw, h): BoxRect) {
    let icon = icon::names(&t.code).and_then(|(day, _)| scene.icons.get(day));
    let kanji: String = weather_char(&t.code).into_iter().collect();
    let range = range_label(t.temp_max, t.temp_min);
    let pop = t.pop.map(|p| format!("{p}%")).unwrap_or_default();
    let icon_w = icon.map_or(0.0, |ic| ic.width() as f32);
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
