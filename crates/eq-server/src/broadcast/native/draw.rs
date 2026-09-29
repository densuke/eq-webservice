//! 画面 (1280x720) の地図の部分を描く。動かない部分 (海・陸・県境・凡例の枠) は一度だけ描いて使い回す。
//! 右パネル・上部バー・時計は panel.rs。

use std::collections::HashMap;

use tiny_skia::{FillRule, Pixmap, Stroke, Transform};

use super::data::{
    city_side, rain_color, temp_label, top_level, warning_fill, weather_char, CityWeather, Side, Warnings,
};
use super::geo::{Shape, View};
use super::model::{scale_color, scale_text_color, QuakeSummary};
use super::paint::{circle, epicenter, paint, rect, rrect, LAND, LAND_EDGE, SEA};
use super::panel;
use super::text::Text;
use crate::quake::Scale;

pub const W: u32 = 1280;
pub const H: u32 = 720;
pub const BAR_H: f32 = 36.0;
pub const MAP_W: f32 = 900.0;

/// 地図の枠 (x, y, 幅, 高さ)
pub const MAP_RECT: (f64, f64, f64, f64) = (0.0, BAR_H as f64, MAP_W as f64, H as f64 - BAR_H as f64);

/// 描くときに渡す、そのときの状態
pub struct Scene<'a> {
    /// 地震の画面に出す地震 (無ければ平時)
    pub quake: Option<&'a QuakeSummary>,
    /// 直近の地震 (新しい順)
    pub history: &'a [QuakeSummary],
    pub warnings: Option<&'a Warnings>,
    pub weather: Option<&'a CityWeather>,
    /// サーバの時計での今 (epoch ミリ秒)
    pub now_ms: u64,
    pub connected: bool,
    pub bgm_title: &'a str,
}

pub struct Renderer {
    pub(super) text: Text,
    view: View,
    prefs: Vec<Shape>,
    areas: HashMap<String, Shape>,
    base: Pixmap,
}

impl Renderer {
    pub fn new(view: View, prefs: Vec<Shape>, areas: Vec<Shape>, text: Text) -> Renderer {
        let mut r = Renderer {
            text,
            view,
            prefs,
            areas: areas.into_iter().map(|s| (s.key.clone(), s)).collect(),
            base: Pixmap::new(W, H).expect("size"),
        };
        r.base = r.draw_base();
        r
    }

    fn draw_base(&mut self) -> Pixmap {
        let mut pm = Pixmap::new(W, H).expect("size");
        rect(&mut pm, 0.0, BAR_H, MAP_W, H as f32 - BAR_H, SEA, 1.0);
        let edge = Stroke {
            width: 0.8,
            ..Stroke::default()
        };
        for s in &self.prefs {
            pm.fill_path(
                &s.path,
                &paint(LAND, 1.0),
                FillRule::EvenOdd,
                Transform::identity(),
                None,
            );
            pm.stroke_path(&s.path, &paint(LAND_EDGE, 1.0), &edge, Transform::identity(), None);
        }
        panel::draw_frame(&mut pm, &mut self.text);
        pm
    }

    /// 1 コマ描く (premultiplied RGBA。全面が不透明)
    pub fn render(&mut self, scene: &Scene) -> Pixmap {
        let mut pm = self.base.clone();
        match scene.quake {
            Some(q) => self.draw_quake(&mut pm, q),
            None => self.draw_calm(&mut pm, scene),
        }
        panel::draw_dynamic(&mut pm, &mut self.text, scene);
        pm
    }

    /// 地震の画面: 都道府県を最大震度の色で塗り、震度の札と震央を出す
    fn draw_quake(&mut self, pm: &mut Pixmap, q: &QuakeSummary) {
        let edge = Stroke {
            width: 1.0,
            ..Stroke::default()
        };
        let hit: Vec<(&Shape, Scale)> = q
            .pref_scales
            .iter()
            .filter_map(|(name, sc)| Some((self.prefs.iter().find(|s| &s.key == name)?, *sc)))
            .collect();
        for (s, sc) in &hit {
            pm.fill_path(
                &s.path,
                &paint(scale_color(*sc), 1.0),
                FillRule::EvenOdd,
                Transform::identity(),
                None,
            );
            pm.stroke_path(&s.path, &paint(LAND_EDGE, 1.0), &edge, Transform::identity(), None);
        }
        // 数字の札 (文字が描けないときは出さない)
        if self.text.enabled() {
            for (s, sc) in &hit {
                let label = sc.label();
                let w = 12.0 + 9.0 * label.chars().count() as f32;
                let (x, y) = s.center;
                rrect(pm, x - w / 2.0, y - 9.0, w, 18.0, 4.0, LAND_EDGE, 0.85);
                rrect(
                    pm,
                    x - w / 2.0 + 1.0,
                    y - 8.0,
                    w - 2.0,
                    16.0,
                    3.0,
                    scale_color(*sc),
                    1.0,
                );
                self.text
                    .draw_center(pm, label, x, y + 5.0, 14.0, scale_text_color(*sc));
            }
        }
        if let Some((lat, lon)) = q.hypocenter.as_ref().and_then(|h| Some((h.latitude?, h.longitude?))) {
            let (x, y) = self.view.px(lon, lat);
            epicenter(pm, x, y);
        }
    }

    /// 平時の画面: 警報・注意報の塗り、雨の点、主要都市の天気
    fn draw_calm(&mut self, pm: &mut Pixmap, scene: &Scene) {
        if let Some(w) = scene.warnings {
            let edge = Stroke {
                width: 0.5,
                ..Stroke::default()
            };
            for (code, kinds) in &w.areas {
                let (Some(level), Some(shape)) = (top_level(kinds), self.areas.get(code)) else {
                    continue;
                };
                let (c, a) = warning_fill(level);
                pm.fill_path(
                    &shape.path,
                    &paint(c, a),
                    FillRule::EvenOdd,
                    Transform::identity(),
                    None,
                );
                pm.stroke_path(&shape.path, &paint([0, 0, 0], 0.35), &edge, Transform::identity(), None);
            }
        }
        let Some(w) = scene.weather else { return };
        // 雨の強い地点ほど上に
        let mut rain = w.rain.clone();
        rain.sort_by(|a, b| a[2].total_cmp(&b[2]));
        for [lat, lon, mm] in rain {
            let (x, y) = self.view.px(lon, lat);
            circle(pm, x, y, 2.5, rain_color(mm), 1.0);
        }
        for c in &w.cities {
            let (x, y) = self.view.px(c.lon, c.lat);
            circle(pm, x, y, 3.0, [255, 255, 255], 1.0);
            let temp = temp_label(c.temp);
            let label: String = weather_char(&c.code).into_iter().collect::<String>() + &temp;
            let bw = 22.0 + temp.chars().count() as f32 * 8.0;
            let (bx, by) = match city_side(&c.name) {
                Side::Up => (-bw / 2.0, -30.0),
                Side::Down => (-bw / 2.0, 8.0),
                Side::Left => (-bw - 7.0, -11.0),
                Side::Right => (7.0, -11.0),
            };
            rrect(pm, x + bx, y + by, bw, 22.0, 11.0, [240, 244, 248], 0.92);
            self.text
                .draw_center(pm, &label, x + bx + bw / 2.0, y + by + 16.0, 13.0, LAND_EDGE);
        }
    }
}
