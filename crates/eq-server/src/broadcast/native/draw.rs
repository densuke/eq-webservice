//! 画面 (1280x720) の地図の部分を描く。動かない部分 (海・陸・県境・凡例の枠) は一度だけ描いて使い回す。
//! 右パネル・上部バー・時計は panel.rs。

use std::collections::HashMap;

use tiny_skia::{Path, Pixmap};

use super::banner;
use super::calm;
use super::camera::Fit;
use super::cards::CardCache;
use super::data::{CityWeather, Warnings};
use super::eew::{forecast_tag, EewSummary, Wave};
use super::frame::{BoxRect, Clip, Frame, OKINAWA};
use super::geo::{Shape, View};
use super::hindsight::Hindsight;
use super::icon::Icons;
use super::layout_resolve::Rect;
use super::model::{scale_color, scale_text_color, QuakeSummary};
use super::notice::{self, LayoutCache, Notices};
use super::paint::{epicenter, ghost_epicenter, rect, rrect, LAND, LAND_EDGE, MUTED, NEIGHBOR, NEIGHBOR_EDGE, SEA};
use super::panel;
use super::placed::Placed;
use super::shaken::{Stations, Zones};
use super::test_mark;
use super::text::Text;
use crate::broadcast::status::Notice;
use crate::quake::{Hypocenter, Scale};

pub const W: u32 = 1280;
pub const H: u32 = 720;

// 部品の枠 (地図・サブの地図・右パネルなど) は定数ではなく、レイアウトの定義から割り付けた placed::Placed が持つ。
// サブの地図 (試験: BroadcastConfig::sub_map) は地震の画面だけに描く。有効なら右の列は broadcast-quake の矩形で、
// 詳細 → サブの地図 → 履歴 → 出典の順に並ぶ (無効なら broadcast と同じ詳細・履歴・出典)

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
    /// 天気の札を今と明日で切り替える間隔 (秒。0 は今だけ)
    pub flip_s: u64,
    /// サーバの時計での今 (epoch ミリ秒)
    pub now_ms: u64,
    pub connected: bool,
    pub bgm_title: &'a str,
    /// 上部バーの右に出す配信元の名前 (空なら出さない)
    pub label: &'a str,
    /// テスト配信 (赤い帯・TEST の透かし・[テスト] を必ず描く)
    pub test: bool,
    /// 記録から描き直すとき、のちの報で分かった震源 (まだ本物の震源が届いていない間だけ。薄い印で出す)。ライブは None
    pub hindsight: Option<&'a Hindsight>,
    /// 記録から描き直すとき、時計を飛ばした直後 (時計の枠に「早送り」を出す)。ライブは false
    pub fast_forward: bool,
    /// 上部バーの右に出す状態の札 (docs/broadcast-status.md)。ライブだけが入れる。再現動画は None
    pub status: Option<Notice>,
    /// 上部バーに出す同接 (取れて新しいときだけ。ライブだけが入れる。再現動画は None)
    pub viewers: Option<u64>,
    /// 平時の右パネルの下に出すお知らせ (docs/broadcast-native.md)。ライブだけが入れる。再現動画は None
    pub notices: Option<&'a Notices>,
}

pub struct Renderer {
    pub(super) text: Text,
    /// 日本全体の本図 (path はこの座標で作ってある)
    main: Frame,
    /// 離島の別枠
    insets: Vec<Frame>,
    /// 寄った本図 (set_view で入れる)。あれば、本図と別枠の代わりにこれを描く
    zoomed: Option<Frame>,
    /// 寄りの設定が有効なとき: 地図の枠の型と、地震情報細分区域の外接矩形 (名前 -> 地図の座標)
    clip: Option<Clip>,
    zones: Zones,
    /// サブの地図の枠の型 (sub_map が有効なときだけ)
    sub_clip: Option<Clip>,
    /// 次に描くサブの地図 (set_sub_view で入れる)。無ければ描かない
    sub: Option<Frame>,
    /// 周辺国の陸地 (都道府県より下に描く。無ければ空)
    neighbors: Vec<Shape>,
    prefs: Vec<Shape>,
    areas: HashMap<String, Shape>,
    base: Pixmap,
    /// 天気の札の置き場所 (警報を避けた位置) を面ごとに覚える
    cards: CardCache,
    /// お知らせの並べた行の覚え
    notice_lines: LayoutCache,
    /// 部品ごとの枠 (レイアウトの定義から割り付けたもの。replay-video は組み込みの定義)
    placed: Placed,
}

/// 別枠の枠線の色 (web/public/style.css の .inset)
pub(super) const INSET_LINE: [u8; 3] = [0x3a, 0x44, 0x52];

impl Renderer {
    pub fn new(
        view: View,
        neighbors: Vec<Shape>,
        prefs: Vec<Shape>,
        areas: Vec<Shape>,
        text: Text,
        placed: Placed,
    ) -> Renderer {
        let mut r = Renderer {
            text,
            insets: placed
                .inset
                .and_then(|at| Frame::inset(&view, &OKINAWA, at))
                .into_iter()
                .collect(),
            main: Frame::main(view),
            zoomed: None,
            clip: None,
            zones: Zones::default(),
            sub_clip: None,
            sub: None,
            neighbors,
            prefs,
            areas: areas.into_iter().map(|s| (s.key.clone(), s)).collect(),
            base: Pixmap::new(W, H).expect("size"),
            cards: CardCache::default(),
            notice_lines: LayoutCache::default(),
            placed,
        };
        r.base = r.draw_base();
        r
    }

    /// 地震情報細分区域 (範囲の計算にだけ使い、塗りは描かない) と観測点の表を入れる。寄りとサブの地図が使う
    pub fn set_zones(&mut self, areas: Vec<Shape>, stations: Stations) {
        self.zones = Zones::new(areas, &self.prefs, stations);
    }

    /// 地図の枠の縦横比 (寄りの範囲の計算に使う)
    pub fn map_aspect(&self) -> f64 {
        self.placed.map_aspect()
    }

    /// サブの地図の枠の縦横比 (定義に置かれていなければ None)
    pub fn sub_aspect(&self) -> Option<f64> {
        self.placed.sub_aspect()
    }

    /// 寄りを有効にする (地図の枠の型を立てる)
    pub fn enable_zoom(&mut self) {
        self.clip = Frame::map_clip(self.placed.main);
    }

    /// サブの地図を有効にする (試験)
    pub fn enable_sub_map(&mut self) {
        self.sub_clip = self.placed.sub.and_then(Frame::sub_clip);
    }

    pub fn sub_map_enabled(&self) -> bool {
        self.sub_clip.is_some()
    }

    /// 次に描くサブの地図の表示範囲 (地図の座標)。None なら描かない。有効でなければ何もしない
    pub fn set_sub_view(&mut self, fit: Option<Fit>) {
        self.sub = fit
            .zip(self.sub_clip.as_ref())
            .zip(self.placed.sub)
            .map(|((f, clip), at)| Frame::zoomed(&self.main.view, View::from_fit(&f, at.tuple64()), clip));
    }

    pub fn zoom_enabled(&self) -> bool {
        self.clip.is_some()
    }

    /// 寄りの範囲を求める表 (細分区域・観測点・県)
    pub fn zones(&self) -> &Zones {
        &self.zones
    }

    /// 次に描く地図の表示範囲 (地図の座標)。None なら日本全体 (別枠も出す)。寄りが有効でなければ何もしない
    pub fn set_view(&mut self, fit: Option<Fit>) {
        self.zoomed = fit.zip(self.clip.as_ref()).map(|(f, clip)| {
            Frame::zoomed(
                &self.main.view,
                super::geo::View::from_fit(&f, self.placed.main.tuple64()),
                clip,
            )
        });
    }

    /// 動かない部分: 海・陸・県境、別枠、パネルの枠
    fn draw_base(&mut self) -> Pixmap {
        let mut pm = Pixmap::new(W, H).expect("size");
        let main = self.placed.main;
        rect(&mut pm, main.x, main.y, main.w, main.h, SEA, 1.0);
        for f in std::iter::once(&self.main).chain(&self.insets) {
            if let Some(((x, y, w, h), _)) = f.inset_box() {
                rrect(&mut pm, x - 1.0, y - 1.0, w + 2.0, h + 2.0, 4.0, INSET_LINE, 1.0);
                rect(&mut pm, x, y, w, h, SEA, 1.0);
            }
            draw_land(&mut pm, f, &self.neighbors, &self.prefs);
            if let Some(((x, y, _, _), title)) = f.inset_box() {
                self.text.draw(&mut pm, title, x + 4.0, y + 12.0, 10.0, MUTED);
            }
        }
        panel::draw_frame(&mut pm, &mut self.text, &self.placed);
        pm
    }

    /// 1 コマ描く (premultiplied RGBA。全面が不透明)。地震波は含まない。
    /// 文字や塗りは重いので、波が動く間も 1 秒ごとにここで描き、コマごとには draw_waves だけを重ねる
    pub fn render(&mut self, scene: &Scene) -> Pixmap {
        let mut pm = self.base.clone();
        // サブの地図を描く地震の画面は、右の列を broadcast-quake の矩形で描く (描かないときは平時の矩形のまま)
        let placed = self
            .placed
            .for_screen(self.sub.is_some() && (scene.quake.is_some() || scene.eew.is_some()));
        let main = placed.main;
        if let Some(z) = &self.zoomed {
            // 日本全体の地図 (と別枠) を海で隠し、寄った地図を描き直す
            rect(&mut pm, main.x, main.y, main.w, main.h, SEA, 1.0);
            draw_land(&mut pm, z, &self.neighbors, &self.prefs);
            if let Some(legend) = placed.legend {
                panel::draw_legend(&mut pm, &mut self.text, legend);
            }
        }
        for (slot, f) in frames(&self.main, &self.insets, &self.zoomed).into_iter().enumerate() {
            match Shake::of(scene) {
                Some(s) => draw_shake(&mut pm, &mut self.text, &self.prefs, f, &s, true, main.right()),
                None => {
                    let (fixed, bounds) = card_room(f, &self.insets, self.zoomed.is_some(), scene.test, &placed);
                    let env = calm::CardEnv {
                        slot,
                        prefs: &self.prefs,
                        fixed: &fixed,
                        bounds,
                        info: calm::info_window(placed.main),
                    };
                    calm::draw(&mut pm, &mut self.text, &self.areas, f, scene, &env, &mut self.cards)
                }
            }
            if let Some(h) = scene.hindsight {
                draw_hindsight(&mut pm, &mut self.text, f, h);
            }
        }
        panel::draw_dynamic(&mut pm, &mut self.text, scene, &placed);
        if let (Some(f), Some(at), true) = (&self.sub, placed.sub, scene.quake.is_some() || scene.eew.is_some()) {
            draw_sub_map(&mut pm, &mut self.text, &self.neighbors, &self.prefs, f, scene, at);
        }
        // お知らせは平時だけ (右パネルの下半分。地震の画面では出さない)
        if let (None, None, Some(n), Some(area)) = (scene.quake, scene.eew, scene.notices, placed.notice) {
            notice::draw(&mut pm, &mut self.text, &mut self.notice_lines, n, scene.now_ms, area);
        }
        // 警報の帯は平時だけ。定義の banners の矩形の中に描く (地震の画面では矩形は空のまま)
        if let (None, None, Some(at)) = (scene.quake, scene.eew, placed.banners) {
            banner::draw(&mut pm, &mut self.text, scene.warnings, at, scene.now_ms);
        }
        if scene.test {
            test_mark::draw(&mut pm, &mut self.text, &placed);
        }
        pm
    }

    /// 地震波の円 (P 波は青、S 波は赤で、内側をうっすら塗る) を重ねる
    pub fn draw_waves(&self, pm: &mut Pixmap, waves: &[Wave]) {
        // 円の path は日本全体の本図の座標で作り、寄った本図・別枠は変換して映す
        for (p, s) in wave_paths(&self.main.view, waves) {
            for f in frames(&self.main, &self.insets, &self.zoomed) {
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

/// 描く面: 寄っていれば寄った本図だけ、そうでなければ本図と別枠
/// 札を置かない所と、置いてよい範囲。別枠は枠の中だけ。本図は左下の凡例・情報の窓・別枠 (寄っていないとき) を避ける
fn card_room(f: &Frame, insets: &[Frame], zoomed: bool, test: bool, placed: &Placed) -> (Vec<BoxRect>, BoxRect) {
    if let Some((rect, _)) = f.inset_box() {
        return (Vec::new(), rect);
    }
    let mut fixed: Vec<BoxRect> = placed.legend.map(Rect::tuple).into_iter().collect();
    fixed.push(calm::info_window(placed.main));
    if !zoomed {
        fixed.extend(insets.iter().filter_map(|i| i.inset_box().map(|(r, _)| r)));
    }
    (fixed, calm::main_bounds(test, placed.main))
}

fn frames<'a>(main: &'a Frame, insets: &'a [Frame], zoomed: &'a Option<Frame>) -> Vec<&'a Frame> {
    match zoomed {
        Some(z) => vec![z],
        None => std::iter::once(main).chain(insets).collect(),
    }
}

/// 周辺国の陸地と都道府県をその面に描く
fn draw_land(pm: &mut Pixmap, f: &Frame, neighbors: &[Shape], prefs: &[Shape]) {
    for s in neighbors {
        f.fill(pm, &s.path, NEIGHBOR, 1.0);
        f.stroke(pm, &s.path, NEIGHBOR_EDGE, 1.0, 0.6);
    }
    for s in prefs {
        f.fill(pm, &s.path, LAND, 1.0);
        f.stroke(pm, &s.path, LAND_EDGE, 1.0, 0.8);
    }
}

/// サブの地図 (試験): 矩形 at (定義の map-sub) を海で塗り、陸と、地震の揺れ (県の塗り・震央) を描いて、1px の枠を付ける。
/// 震度の札・緊急地震速報の札 (tag) は出さない (枠の外にはみ出しうる)。区域の塗りと観測点の点は無い
fn draw_sub_map(
    pm: &mut Pixmap,
    text: &mut Text,
    neighbors: &[Shape],
    prefs: &[Shape],
    f: &Frame,
    scene: &Scene,
    at: Rect,
) {
    let Rect { x, y, w, h } = at;
    rect(pm, x, y, w, h, SEA, 1.0);
    draw_land(pm, f, neighbors, prefs);
    if let Some(s) = Shake::of(scene) {
        draw_shake(pm, text, prefs, f, &s, false, x + w);
    }
    for (rx, ry, rw, rh) in [
        (x, y, w, 1.0),
        (x, y + h - 1.0, w, 1.0),
        (x, y, 1.0, h),
        (x + w - 1.0, y, 1.0, h),
    ] {
        rect(pm, rx, ry, rw, rh, INSET_LINE, 1.0);
    }
}

/// のちの報で分かった震源の薄い印 (札は本図だけ)。本物の震源が届くまでの間に出す
fn draw_hindsight(pm: &mut Pixmap, text: &mut Text, frame: &Frame, h: &Hindsight) {
    let Some((x, y)) = frame.marker(h.lon, h.lat) else {
        return;
    };
    ghost_epicenter(pm, x, y);
    if text.enabled() && !frame.is_inset() {
        text.draw(pm, "のちに判明する震源", x + 14.0, y + 4.0, 12.0, MUTED);
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

/// 揺れの画面: 都道府県を震度の色で塗り、震度の札と震央 (と札) を出す (札は本図だけ)。
/// map_right は地図の枠 (定義の main) の右端 (緊急地震速報の札が本図の右端をはみ出さないため)
fn draw_shake(
    pm: &mut Pixmap,
    text: &mut Text,
    prefs: &[Shape],
    frame: &Frame,
    shake: &Shake,
    cards: bool,
    map_right: f32,
) {
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
    // 震度の札・緊急地震速報の札 (cards のときだけ。文字が描けないときは出さない)
    if cards && text.enabled() && !frame.is_inset() {
        for (s, sc) in &hit {
            let label = sc.label();
            let w = 12.0 + 9.0 * label.chars().count() as f32;
            let Some((x, y)) = frame.point(s.center) else {
                continue;
            };
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
        // サブの地図 (cards でない面) は、印も矩形で切る (縁の近くの震央でも詳細・履歴にはみ出さない)
        epicenter(pm, x, y, if cards { None } else { frame.mask() });
        if let (Some(tag), true) = (&shake.tag, cards && text.enabled()) {
            let w = text.width(tag, 13.0) + 14.0;
            // 印の右に置く。別枠の右端をはみ出すときは左に置く
            let edge = frame.inset_box().map_or(map_right, |((bx, _, bw, _), _)| bx + bw);
            let left = if x + 14.0 + w > edge { x - 14.0 - w } else { x + 14.0 };
            rrect(pm, left, y - 11.0, w, 22.0, 5.0, [255, 255, 255], 0.9);
            rrect(pm, left + 1.0, y - 10.0, w - 2.0, 20.0, 4.0, [0xb3, 0x59, 0x00], 1.0);
            text.draw(pm, tag, left + 7.0, y + 5.0, 13.0, [255, 255, 255]);
        }
    }
}
