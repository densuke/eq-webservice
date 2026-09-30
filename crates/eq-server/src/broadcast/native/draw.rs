//! 画面 (1280x720) の地図の部分を描く。動かない部分 (海・陸・県境・凡例の枠) は一度だけ描いて使い回す。
//! 右パネル・上部バー・時計は panel.rs。

use std::collections::HashMap;

use tiny_skia::{Path, Pixmap};

use super::calm;
use super::data::{CityWeather, Warnings};
use super::eew::{forecast_tag, EewSummary, Wave};
use super::frame::{Frame, OKINAWA};
use super::geo::{Shape, View};
use super::icon::Icons;
use super::model::{scale_color, scale_text_color, QuakeSummary};
use super::paint::{epicenter, rect, rrect, LAND, LAND_EDGE, MUTED, NEIGHBOR, NEIGHBOR_EDGE, SEA};
use super::panel;
use super::test_mark;
use super::text::Text;
use crate::quake::{Hypocenter, Scale};

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
    /// 地震の画面に出す緊急地震速報 (地震情報が無いとき。どちらも無ければ平時)
    pub eew: Option<&'a EewSummary>,
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
    /// 上部バーの右に出す配信元の名前 (空なら出さない)
    pub label: &'a str,
    /// テスト配信 (赤い帯・TEST の透かし・[テスト] を必ず描く)
    pub test: bool,
}

pub struct Renderer {
    pub(super) text: Text,
    main: Frame,
    /// 離島の別枠
    insets: Vec<Frame>,
    /// 周辺国の陸地 (都道府県より下に描く。無ければ空)
    neighbors: Vec<Shape>,
    prefs: Vec<Shape>,
    areas: HashMap<String, Shape>,
    base: Pixmap,
}

/// 別枠の枠線の色 (web/public/style.css の .inset)
const INSET_LINE: [u8; 3] = [0x3a, 0x44, 0x52];

impl Renderer {
    pub fn new(view: View, neighbors: Vec<Shape>, prefs: Vec<Shape>, areas: Vec<Shape>, text: Text) -> Renderer {
        let mut r = Renderer {
            text,
            insets: Frame::inset(&view, &OKINAWA).into_iter().collect(),
            main: Frame::main(view),
            neighbors,
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
            for s in &self.neighbors {
                f.fill(&mut pm, &s.path, NEIGHBOR, 1.0);
                f.stroke(&mut pm, &s.path, NEIGHBOR_EDGE, 1.0, 0.6);
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

    /// 1 コマ描く (premultiplied RGBA。全面が不透明)。地震波は含まない。
    /// 文字や塗りは重いので、波が動く間も 1 秒ごとにここで描き、コマごとには draw_waves だけを重ねる
    pub fn render(&mut self, scene: &Scene) -> Pixmap {
        let mut pm = self.base.clone();
        for f in std::iter::once(&self.main).chain(&self.insets) {
            match Shake::of(scene) {
                Some(s) => draw_shake(&mut pm, &mut self.text, &self.prefs, f, &s),
                None => calm::draw(&mut pm, &mut self.text, &self.areas, f, scene),
            }
        }
        panel::draw_dynamic(&mut pm, &mut self.text, scene);
        if scene.test {
            test_mark::draw(&mut pm, &mut self.text);
        }
        pm
    }

    /// 地震波の円 (P 波は青、S 波は赤で、内側をうっすら塗る) を重ねる
    pub fn draw_waves(&self, pm: &mut Pixmap, waves: &[Wave]) {
        for (p, s) in wave_paths(&self.main.view, waves) {
            for f in std::iter::once(&self.main).chain(&self.insets) {
                if let Some(path) = &s {
                    f.fill(pm, path, S_WAVE, 0.05);
                    f.stroke(pm, path, S_WAVE, 1.0, WAVE_WIDTH);
                }
                if let Some(path) = &p {
                    f.stroke(pm, path, P_WAVE, 1.0, WAVE_WIDTH);
                }
            }
        }
    }
}

/// 地震の画面に描く揺れ (地震情報の観測、または緊急地震速報の予測)
struct Shake<'a> {
    scales: &'a [(String, Scale)],
    /// 予測か (半透明の塗りと白い縁で、観測と見分ける)
    forecast: bool,
    hypocenter: Option<&'a Hypocenter>,
    /// 震源の印の近くに出す札
    tag: Option<String>,
}

impl<'a> Shake<'a> {
    /// 地震情報があればそれ、無ければ緊急地震速報。どちらも無ければ平時
    fn of(scene: &Scene<'a>) -> Option<Shake<'a>> {
        match (scene.quake, scene.eew) {
            (Some(q), _) => Some(Shake {
                scales: &q.pref_scales,
                forecast: false,
                hypocenter: q.hypocenter.as_ref(),
                tag: None,
            }),
            (None, Some(e)) => Some(Shake {
                scales: &e.pref_scales,
                forecast: true,
                hypocenter: e.hypocenter.as_ref(),
                tag: forecast_tag(e),
            }),
            (None, None) => None,
        }
    }
}

/// P 波・S 波の色と線の太さ (web/public/style.css の --p-wave・--s-wave、map.css の .wave)
const P_WAVE: [u8; 3] = [0x4f, 0xc3, 0xf7];
const S_WAVE: [u8; 3] = [0xff, 0x52, 0x52];
const WAVE_WIDTH: f32 = 2.5;
/// 予測の塗りの不透明度
const FORECAST_ALPHA: f32 = 0.6;

/// 波の円 (P 波・S 波) の path。本図の座標で 1 度だけ作り、別枠は変換して映す
type WavePaths = (Option<Path>, Option<Path>);

fn wave_paths(view: &View, waves: &[Wave]) -> Vec<WavePaths> {
    let circle = |w: &Wave, km: Option<f64>| km.and_then(|km| view.circle(w.lat, w.lon, km));
    waves.iter().map(|w| (circle(w, w.p_km), circle(w, w.s_km))).collect()
}

/// 揺れの画面: 都道府県を震度の色で塗り、震度の札と震央 (と札) を出す (札は本図だけ)
fn draw_shake(pm: &mut Pixmap, text: &mut Text, prefs: &[Shape], frame: &Frame, shake: &Shake) {
    let hit: Vec<(&Shape, Scale)> = shake
        .scales
        .iter()
        .filter_map(|(name, sc)| Some((prefs.iter().find(|s| &s.key == name)?, *sc)))
        .collect();
    let (alpha, edge) = if shake.forecast {
        (FORECAST_ALPHA, [255, 255, 255])
    } else {
        (1.0, LAND_EDGE)
    };
    for (s, sc) in &hit {
        frame.fill(pm, &s.path, scale_color(*sc), alpha);
        frame.stroke(pm, &s.path, edge, 1.0, if shake.forecast { 1.2 } else { 1.0 });
    }
    // 数字の札 (文字が描けないときは出さない)
    if text.enabled() && !frame.is_inset() {
        for (s, sc) in &hit {
            let label = sc.label();
            let w = 12.0 + 9.0 * label.chars().count() as f32;
            let (x, y) = s.center;
            rrect(pm, x - w / 2.0, y - 9.0, w, 18.0, 4.0, edge, 0.85);
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
    if let Some((x, y)) = shake.hypocenter.and_then(|h| frame.marker(h.longitude?, h.latitude?)) {
        epicenter(pm, x, y);
        if let (Some(tag), true) = (&shake.tag, text.enabled()) {
            let w = text.width(tag, 13.0) + 14.0;
            // 印の右に置く。別枠の右端をはみ出すときは左に置く
            let edge = frame.inset_box().map_or(MAP_W, |((bx, _, bw, _), _)| bx + bw);
            let left = if x + 14.0 + w > edge { x - 14.0 - w } else { x + 14.0 };
            rrect(pm, left, y - 11.0, w, 22.0, 5.0, [255, 255, 255], 0.9);
            rrect(pm, left + 1.0, y - 10.0, w - 2.0, 20.0, 4.0, [0xb3, 0x59, 0x00], 1.0);
            text.draw(pm, tag, left + 7.0, y + 5.0, 13.0, [255, 255, 255]);
        }
    }
}
