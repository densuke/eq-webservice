//! 画面 (1280x720) の地図の部分を描く。動かない部分 (海・陸・県境・凡例の枠) は一度だけ描いて使い回す。
//! 右パネル・上部バー・時計は panel.rs。

use std::collections::HashMap;

use tiny_skia::Pixmap;

use super::calm;
use super::data::{CityWeather, Warnings};
use super::frame::{Frame, OKINAWA};
use super::geo::{Shape, View};
use super::icon::Icons;
use super::model::{scale_color, scale_text_color, QuakeSummary};
use super::paint::{epicenter, rect, rrect, LAND, LAND_EDGE, MUTED, SEA};
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
    /// 取得済みの天気アイコン
    pub icons: &'a Icons,
    /// サーバの時計での今 (epoch ミリ秒)
    pub now_ms: u64,
    pub connected: bool,
    pub bgm_title: &'a str,
}

pub struct Renderer {
    pub(super) text: Text,
    main: Frame,
    /// 離島の別枠
    insets: Vec<Frame>,
    prefs: Vec<Shape>,
    areas: HashMap<String, Shape>,
    base: Pixmap,
}

/// 別枠の枠線の色 (web/public/style.css の .inset)
const INSET_LINE: [u8; 3] = [0x3a, 0x44, 0x52];

impl Renderer {
    pub fn new(view: View, prefs: Vec<Shape>, areas: Vec<Shape>, text: Text) -> Renderer {
        let mut r = Renderer {
            text,
            insets: Frame::inset(&view, &OKINAWA).into_iter().collect(),
            main: Frame::main(view),
            prefs,
            areas: areas.into_iter().map(|s| (s.key.clone(), s)).collect(),
            base: Pixmap::new(W, H).expect("size"),
        };
        r.base = r.draw_base();
        r
    }

    /// 動かない部分: 海・陸・県境、別枠、パネルの枠
    fn draw_base(&mut self) -> Pixmap {
        let mut pm = Pixmap::new(W, H).expect("size");
        rect(&mut pm, 0.0, BAR_H, MAP_W, H as f32 - BAR_H, SEA, 1.0);
        for f in std::iter::once(&self.main).chain(&self.insets) {
            if let Some(((x, y, w, h), _)) = f.inset_box() {
                rrect(&mut pm, x - 1.0, y - 1.0, w + 2.0, h + 2.0, 4.0, INSET_LINE, 1.0);
                rect(&mut pm, x, y, w, h, SEA, 1.0);
            }
            for s in &self.prefs {
                f.fill(&mut pm, &s.path, LAND, 1.0);
                f.stroke(&mut pm, &s.path, LAND_EDGE, 1.0, 0.8);
            }
            if let Some(((x, y, _, _), title)) = f.inset_box() {
                self.text.draw(&mut pm, title, x + 4.0, y + 12.0, 10.0, MUTED);
            }
        }
        panel::draw_frame(&mut pm, &mut self.text);
        pm
    }

    /// 1 コマ描く (premultiplied RGBA。全面が不透明)
    pub fn render(&mut self, scene: &Scene) -> Pixmap {
        let mut pm = self.base.clone();
        for f in std::iter::once(&self.main).chain(&self.insets) {
            match scene.quake {
                Some(q) => draw_quake(&mut pm, &mut self.text, &self.prefs, f, q),
                None => calm::draw(&mut pm, &mut self.text, &self.areas, f, scene),
            }
        }
        panel::draw_dynamic(&mut pm, &mut self.text, scene);
        pm
    }
}

/// 地震の画面: 都道府県を最大震度の色で塗り、震度の札と震央を出す (札は本図だけ)
fn draw_quake(pm: &mut Pixmap, text: &mut Text, prefs: &[Shape], frame: &Frame, q: &QuakeSummary) {
    let hit: Vec<(&Shape, Scale)> = q
        .pref_scales
        .iter()
        .filter_map(|(name, sc)| Some((prefs.iter().find(|s| &s.key == name)?, *sc)))
        .collect();
    for (s, sc) in &hit {
        frame.fill(pm, &s.path, scale_color(*sc), 1.0);
        frame.stroke(pm, &s.path, LAND_EDGE, 1.0, 1.0);
    }
    // 数字の札 (文字が描けないときは出さない)
    if text.enabled() && !frame.is_inset() {
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
            text.draw_center(pm, label, x, y + 5.0, 14.0, scale_text_color(*sc));
        }
    }
    if let Some((lat, lon)) = q.hypocenter.as_ref().and_then(|h| Some((h.latitude?, h.longitude?))) {
        if frame.contains(lon, lat) {
            let (x, y) = frame.view.px(lon, lat);
            epicenter(pm, x, y);
        }
    }
}
