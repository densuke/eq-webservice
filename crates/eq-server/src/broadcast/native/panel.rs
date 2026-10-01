//! 画面の地図以外の部分: 上部バー・右パネル (詳細・履歴・出典)・凡例・時計。

use tiny_skia::Pixmap;

use super::draw::{Scene, BAR_H, H, MAP_W, W};
use super::eew::{eew_forecast_text, eew_kind_text, EewSummary};
use super::model::{hypo_text, scale_color, scale_text_color, tsunami_text, QuakeSummary};
use super::paint::{rect, rrect, BG, LINE, MUTED, PANEL, TEXT};
use super::text::Text;
use crate::quake::{jst, Scale};

const SIDE_X: f32 = MAP_W;
const PAD: f32 = 16.0;
const CREDIT: [&str; 6] = [
    "情報: P2P地震情報 (気象庁発表)",
    "地図: 地球地図日本 (国土地理院) を加工",
    "津波予報区・細分区域・震度観測点: 気象庁のデータを加工",
    "天気・アメダス: 気象庁",
    "天気アイコン: 気象庁ホームページを加工",
    "(https://www.jma.go.jp/bosai/forecast/)",
];
/// 震度の凡例の色 (震度 1 から 7)
const GAUGE: [(&str, [u8; 3]); 9] = [
    ("1", [0xf2, 0xf2, 0xff]),
    ("2", [0x00, 0xaa, 0xff]),
    ("3", [0x00, 0x41, 0xff]),
    ("4", [0xfa, 0xf5, 0x00]),
    ("5-", [0xff, 0xe6, 0x00]),
    ("5+", [0xff, 0x99, 0x00]),
    ("6-", [0xff, 0x28, 0x00]),
    ("6+", [0xa5, 0x00, 0x21]),
    ("7", [0xb4, 0x00, 0x68]),
];
const WARN_LEGEND: [(&str, [u8; 3]); 4] = [
    ("注意報", [0xf2, 0xe7, 0x00]),
    ("警報", [0xff, 0x28, 0x00]),
    ("危険警報", [0xaa, 0x00, 0xff]),
    ("特別警報", [0xff, 0xff, 0xff]),
];

/// 動かない部分: バーと右パネルの地、題、震度の凡例、出典
pub fn draw_frame(pm: &mut Pixmap, text: &mut Text) {
    rect(pm, 0.0, 0.0, W as f32, BAR_H, PANEL, 1.0);
    rect(pm, 0.0, BAR_H - 1.0, W as f32, 1.0, LINE, 1.0);
    rect(pm, SIDE_X, BAR_H, W as f32 - SIDE_X, H as f32 - BAR_H, PANEL, 1.0);
    rect(pm, SIDE_X, BAR_H, 1.0, H as f32 - BAR_H, LINE, 1.0);
    let w = text.draw(pm, "地震情報マップ", PAD, 24.0, 15.0, TEXT);
    text.draw(
        pm,
        concat!("v", env!("CARGO_PKG_VERSION")),
        PAD + w + 8.0,
        24.0,
        12.0,
        MUTED,
    );
    // 震度の凡例 (左下)。1 が下
    let (x, y0) = (10.0, H as f32 - 10.0 - 147.0);
    rrect(pm, x, y0, 34.0, 147.0, 6.0, BG, 0.8);
    for (i, (label, color)) in GAUGE.iter().rev().enumerate() {
        let y = y0 + 6.0 + i as f32 * 15.0;
        rect(pm, x + 7.0, y, 7.0, 15.0, *color, 1.0);
        text.draw(pm, label, x + 18.0, y + 12.0, 10.0, MUTED);
    }
    for (i, line) in CREDIT.iter().enumerate() {
        text.draw(
            pm,
            line,
            SIDE_X + PAD,
            H as f32 - 5.0 - (CREDIT.len() - 1 - i) as f32 * 15.0,
            11.0,
            MUTED,
        );
    }
}

/// 状態で変わる部分
pub fn draw_dynamic(pm: &mut Pixmap, text: &mut Text, scene: &Scene) {
    let shaking = scene.quake.is_some() || scene.eew.is_some();
    let mode = if shaking { "[地震]" } else { "[平時]" };
    text.draw(pm, mode, 210.0, 24.0, 12.0, MUTED);
    // 右端に配信元 (label)、その左に BGM の曲名
    let mut right = W as f32 - PAD;
    if !scene.label.is_empty() {
        text.draw_right(pm, scene.label, right, 24.0, 12.0, MUTED);
        right -= text.width(scene.label, 12.0) + 16.0;
    }
    if !scene.bgm_title.is_empty() {
        let s = format!("BGM: {}", scene.bgm_title);
        text.draw_right(pm, &s, right, 24.0, 12.0, MUTED);
    }
    if !shaking {
        draw_warn_legend(pm, text);
    }
    match (scene.quake, scene.eew) {
        (None, Some(e)) => draw_eew_detail(pm, text, e),
        (q, _) => draw_detail(pm, text, q.or(scene.history.first()), q.is_some()),
    }
    draw_history(pm, text, scene.history);
    draw_clock(pm, text, scene.now_ms, scene.connected, scene.fast_forward);
}

fn draw_warn_legend(pm: &mut Pixmap, text: &mut Text) {
    let (x, y0) = (10.0, H as f32 - 10.0 - 147.0 - 6.0 - 78.0);
    rrect(pm, x, y0, 66.0, 78.0, 6.0, BG, 0.8);
    for (i, (label, color)) in WARN_LEGEND.iter().enumerate() {
        let y = y0 + 8.0 + i as f32 * 17.0;
        text.draw(pm, label, x + 8.0, y + 10.0, 10.0, MUTED);
        rect(pm, x + 8.0, y + 12.0, 50.0, 3.0, *color, 1.0);
    }
}

fn scale_label(s: Scale) -> &'static str {
    if s.is_known() {
        s.label()
    } else {
        "-"
    }
}

fn badge(pm: &mut Pixmap, text: &mut Text, s: Scale, x: f32, y: f32, size: f32) {
    rrect(pm, x, y, size, size, size / 8.0, scale_color(s), 1.0);
    let label = scale_label(s);
    let px = if label.chars().count() > 1 {
        size * 0.4
    } else {
        size * 0.55
    };
    text.draw_center(
        pm,
        label,
        x + size / 2.0,
        y + size / 2.0 + px * 0.35,
        px,
        scale_text_color(s),
    );
}

/// 右パネルの上: 地震の詳細 (地震の画面のときはその地震、平時は最新の地震)
fn draw_detail(pm: &mut Pixmap, text: &mut Text, q: Option<&QuakeSummary>, live: bool) {
    let x = SIDE_X + PAD;
    let Some(q) = q else {
        text.draw(pm, "受信した情報はまだありません。", x, 70.0, 13.0, MUTED);
        return;
    };
    badge(pm, text, q.max_scale, x, 50.0, 56.0);
    let place = q.hypocenter.as_ref().map_or("", |h| &h.name);
    let title = if place.is_empty() { "震源調査中" } else { place };
    text.draw(
        pm,
        if live { "地震情報" } else { "最新の地震" },
        x + 70.0,
        62.0,
        12.0,
        MUTED,
    );
    text.draw_fit(pm, title, x + 70.0, 86.0, 20.0, TEXT, 270.0);
    text.draw(pm, &format!("{} 発生", q.origin_time), x + 70.0, 104.0, 12.0, MUTED);
    for (i, (k, v)) in [
        ("震源", hypo_text(q.hypocenter.as_ref())),
        ("津波", tsunami_text(&q.tsunami).into()),
    ]
    .iter()
    .enumerate()
    {
        let y = 134.0 + i as f32 * 22.0;
        text.draw(pm, k, x, y, 13.0, MUTED);
        text.draw_fit(pm, v, x + 44.0, y, 13.0, TEXT, 300.0);
    }
    rect(pm, SIDE_X + 1.0, 180.0, W as f32 - SIDE_X - 1.0, 1.0, LINE, 1.0);
}

/// 見出しの札の色。警報は赤、予報は橙 (web の緊急地震速報のバナーの色)
const EEW_WARNING: [u8; 3] = [0xd7, 0x26, 0x3d];
const EEW_FORECAST: [u8; 3] = [0xb3, 0x59, 0x00];

/// 右パネルの上: 緊急地震速報の詳細 (見出しの札に、緊急地震速報であることと警報か予報か)
fn draw_eew_detail(pm: &mut Pixmap, text: &mut Text, e: &EewSummary) {
    let x = SIDE_X + PAD;
    badge(pm, text, e.max_scale, x, 58.0, 56.0);
    let kind = eew_kind_text(e);
    let w = text.width(&kind, 11.0).min(260.0) + 12.0;
    let color = if e.warning { EEW_WARNING } else { EEW_FORECAST };
    rrect(pm, x + 70.0, 57.0, w, 16.0, 4.0, color, 1.0);
    text.draw_fit(pm, &kind, x + 76.0, 69.0, 11.0, [255, 255, 255], 260.0);
    let place = e.hypocenter.as_ref().map_or("", |h| &h.name);
    let place = if place.is_empty() { "震源不明" } else { place };
    text.draw_fit(pm, place, x + 70.0, 94.0, 20.0, TEXT, 270.0);
    text.draw(pm, &format!("{} 発生", e.origin_time), x + 70.0, 110.0, 12.0, MUTED);
    for (i, (k, v)) in [
        ("震源", hypo_text(e.hypocenter.as_ref())),
        ("予測", eew_forecast_text(e)),
    ]
    .iter()
    .enumerate()
    {
        let y = 138.0 + i as f32 * 22.0;
        text.draw(pm, k, x, y, 13.0, MUTED);
        text.draw_fit(pm, v, x + 44.0, y, 13.0, TEXT, 300.0);
    }
    rect(pm, SIDE_X + 1.0, 180.0, W as f32 - SIDE_X - 1.0, 1.0, LINE, 1.0);
}

/// 右パネルの中: 直近の地震 (最大 5 件)
fn draw_history(pm: &mut Pixmap, text: &mut Text, history: &[QuakeSummary]) {
    let x = SIDE_X + PAD;
    text.draw(pm, "履歴", x, 204.0, 13.0, TEXT);
    for (i, q) in history.iter().take(5).enumerate() {
        let y = 216.0 + i as f32 * 52.0;
        badge(pm, text, q.max_scale, x, y + 4.0, 38.0);
        let place = q.hypocenter.as_ref().map_or("", |h| &h.name);
        text.draw_fit(
            pm,
            if place.is_empty() { "震源調査中" } else { place },
            x + 50.0,
            y + 22.0,
            14.0,
            TEXT,
            290.0,
        );
        let mag = q
            .hypocenter
            .as_ref()
            .and_then(|h| h.magnitude)
            .map_or(String::new(), |m| format!(" M{m:.1}"));
        let time: String = q.origin_time.chars().skip(5).take(11).collect();
        text.draw(pm, &format!("{time}{mag}"), x + 50.0, y + 40.0, 12.0, MUTED);
    }
}

/// 日本語の曜日 (days は 1970-01-01 からの日数。その日は木曜)
pub fn weekday(days: i64) -> &'static str {
    ["日", "月", "火", "水", "木", "金", "土"][(days + 4).rem_euclid(7) as usize]
}

/// 地図の右下の時計。枠の色は接続の状態 (緑 = つながっている / 赤 = 切れている)
fn draw_clock(pm: &mut Pixmap, text: &mut Text, now_ms: u64, connected: bool, fast_forward: bool) {
    let (w, h) = (176.0, 74.0);
    let (x, y) = (MAP_W - 10.0 - w, H as f32 - 10.0 - h);
    let (border, fill) = if connected {
        ([0x3f, 0xb9, 0x50], BG)
    } else {
        ([0xf8, 0x51, 0x49], [0x5a, 0x0a, 0x10])
    };
    rrect(pm, x, y, w, h, 8.0, border, 1.0);
    rrect(pm, x + 2.0, y + 2.0, w - 4.0, h - 4.0, 6.0, fill, 0.95);
    let jst = jst::format(now_ms as i64); // "2026/09/30 12:34:56"
    let (date, time) = jst.split_once(' ').unwrap_or(("", ""));
    let days = (now_ms as i64 + 9 * 3_600_000).div_euclid(86_400_000);
    text.draw(
        pm,
        &format!("{date} ({})", weekday(days)),
        x + 12.0,
        y + 22.0,
        13.0,
        MUTED,
    );
    if fast_forward {
        text.draw_right(pm, "早送り", x + w - 12.0, y + 22.0, 13.0, [0xff, 0xb3, 0x00]);
    }
    let (hm, sec) = (time.get(..5).unwrap_or(""), time.get(6..).unwrap_or(""));
    let wd = text.draw(pm, hm, x + 12.0, y + 60.0, 34.0, TEXT);
    text.draw(pm, sec, x + 16.0 + wd, y + 60.0, 18.0, MUTED);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays_are_japanese() {
        assert_eq!(weekday(0), "木"); // 1970-01-01
        assert_eq!(weekday(20_726), "水"); // 2026-09-30
    }
}
