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
use super::geo::{Coast, Shape, View};
use super::hindsight::Hindsight;
use super::icon::Icons;
use super::layout_resolve::Rect;
use super::model::{scale_color, scale_text_color, QuakeSummary};
use super::notice::{self, LayoutCache, Notices};
use super::paint::{epicenter, ghost_epicenter, rect, rrect, LAND, LAND_EDGE, MUTED, NEIGHBOR, NEIGHBOR_EDGE, SEA};
use super::panel;
use super::placed::Placed;
use super::pref_list;
use super::quake_band;
use super::shaken::{Stations, Zones};
use super::test_mark;
use super::text::Text;
use super::tsunami_coast;
use crate::broadcast::status::Notice;
use crate::quake::model::TsunamiArea;
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
    /// いま発表中の津波予報区 (地震の画面の警報の帯に使う。無ければ空)
    pub tsunami: &'a [TsunamiArea],
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

/// 波の円を挟んで描くための絵: 札・時計・凡例まで重ねた 1 枚と、そのうち札などが塗っている画素 (波はそこを塗らない)。
/// 札が塗っていない画素は、札を重ねる前 (地図・震度の塗り・海岸線) と同じなので、波を後から描いても波が札の下になる
pub struct Layers {
    full: Pixmap,
    /// 地図の中で、札などが塗っている画素の並び (バイト位置と長さ)。札や時計の箱は半透明なので、
    /// 下に波があるとそのまま透けて見えてしまう
    covered: Vec<(usize, usize)>,
}

/// pm の rect の中で、塗られている (透明でない) 画素の連なり
fn covered_runs(pm: &Pixmap, rect: Rect) -> Vec<(usize, usize)> {
    let (w, h) = (W as usize, H as usize);
    let (x0, x1) = (
        (rect.x.max(0.0) as usize).min(w),
        ((rect.x + rect.w).ceil() as usize).min(w),
    );
    let (y0, y1) = (
        (rect.y.max(0.0) as usize).min(h),
        ((rect.y + rect.h).ceil() as usize).min(h),
    );
    let data = pm.data();
    let mut runs = Vec::new();
    for y in y0..y1 {
        let mut start = None;
        for x in x0..=x1 {
            let on = x < x1 && data[(y * w + x) * 4 + 3] != 0;
            match (start, on) {
                (None, true) => start = Some(x),
                (Some(s), false) => {
                    runs.push(((y * w + s) * 4, (x - s) * 4));
                    start = None;
                }
                _ => {}
            }
        }
    }
    runs
}

/// 描き先。split でなければ 1 枚だけで、上も下も同じ絵に重ねる (描く順序は今までどおり)
pub(super) struct Target {
    under: Pixmap,
    over: Option<Pixmap>,
}

impl Target {
    fn new(under: Pixmap, split: bool) -> Target {
        let over = split.then(|| Pixmap::new(W, H).expect("size"));
        Target { under, over }
    }

    /// 波の下に描くもの
    pub(super) fn low(&mut self) -> &mut Pixmap {
        &mut self.under
    }

    /// 波の上に描くもの
    pub(super) fn top(&mut self) -> &mut Pixmap {
        self.over.as_mut().unwrap_or(&mut self.under)
    }

    /// 上の絵の r の中を消す (下の絵を海で塗り直したとき、古い札を残さない)
    fn clear_over(&mut self, r: Rect) {
        let Some(over) = self.over.as_mut() else { return };
        let (x0, x1) = (r.x as usize, ((r.x + r.w) as usize).min(W as usize));
        let (y0, y1) = (r.y as usize, ((r.y + r.h) as usize).min(H as usize));
        let data = over.data_mut();
        for y in y0..y1 {
            data[(y * W as usize + x0) * 4..(y * W as usize + x1) * 4].fill(0);
        }
    }

    fn flat(self) -> Pixmap {
        self.under
    }
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
    /// 津波予報区の海岸線 (無ければ空)
    coast: Vec<Coast>,
    areas: HashMap<String, Shape>,
    base: Pixmap,
    /// base を波の下 (海・陸・県境) と上 (凡例・別枠の題) に分けたもの。波の円を挟む描き方で初めて要るときに作る
    base_split: Option<(Pixmap, Pixmap)>,
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
        let insets: Vec<Frame> = placed
            .inset
            .and_then(|at| Frame::inset(&view, &OKINAWA, at))
            .into_iter()
            .collect();
        let mut r = Renderer {
            text,
            main: Frame::main(view).bounded(placed.main, insets.first()),
            insets,
            zoomed: None,
            clip: None,
            zones: Zones::default(),
            sub_clip: None,
            sub: None,
            neighbors,
            prefs,
            coast: Vec::new(),
            areas: areas.into_iter().map(|s| (s.key.clone(), s)).collect(),
            base: Pixmap::new(W, H).expect("size"),
            base_split: None,
            cards: CardCache::default(),
            notice_lines: LayoutCache::default(),
            placed,
        };
        r.base = r.draw_base(false).flat();
        r
    }

    /// 津波予報区の海岸線を入れる
    pub fn set_coast(&mut self, coast: Vec<Coast>) {
        self.coast = coast;
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

    /// 動かない部分: 海・陸・県境、別枠、パネルの枠。split なら、凡例と別枠の題 (波より上の札) を別の絵に分ける
    fn draw_base(&mut self, split: bool) -> Target {
        let mut t = Target::new(Pixmap::new(W, H).expect("size"), split);
        let main = self.placed.main;
        rect(t.low(), main.x, main.y, main.w, main.h, SEA, 1.0);
        for f in std::iter::once(&self.main).chain(&self.insets) {
            if let Some(((x, y, w, h), _)) = f.inset_box() {
                rrect(t.low(), x - 1.0, y - 1.0, w + 2.0, h + 2.0, 4.0, INSET_LINE, 1.0);
                rect(t.low(), x, y, w, h, SEA, 1.0);
            }
            draw_land(t.low(), f, &self.neighbors, &self.prefs);
            if let Some(((x, y, _, _), title)) = f.inset_box() {
                self.text.draw(t.top(), title, x + 4.0, y + 12.0, 10.0, MUTED);
            }
        }
        panel::draw_frame(t.low(), &mut self.text, &self.placed);
        if let Some(legend) = self.placed.legend {
            panel::draw_legend(t.top(), &mut self.text, legend);
        }
        t
    }

    /// 1 コマ描く (premultiplied RGBA。全面が不透明)。地震波は含まない。
    /// 文字や塗りは重いので、波が動く間も 1 秒ごとにここで描き、コマごとには draw_waves だけを重ねる
    pub fn render(&mut self, scene: &Scene) -> Pixmap {
        self.render_into(scene, false).flat()
    }

    /// render と同じ絵を、波の円を挟めるよう 2 枚に分けて描く (下: 地図・震度の塗り・海岸線、上: 札・時計・凡例)
    pub fn render_layers(&mut self, scene: &Scene) -> Layers {
        let Target { mut under, over } = self.render_into(scene, true);
        let over = over.expect("split");
        // 波の円が出る面: 地図の枠とサブの地図
        let mut covered = covered_runs(&over, self.placed.main);
        if let Some(sub) = self.placed.sub.filter(|_| self.sub.is_some()) {
            covered.extend(covered_runs(&over, sub));
        }
        under.draw_pixmap(
            0,
            0,
            over.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
        Layers { full: under, covered }
    }

    fn render_into(&mut self, scene: &Scene, split: bool) -> Target {
        let mut t = self.base_target(split);
        // サブの地図を描く地震の画面は、右の列を broadcast-quake の矩形で描く (描かないときは平時の矩形のまま)
        let placed = self
            .placed
            .for_screen(self.sub.is_some() && (scene.quake.is_some() || scene.eew.is_some()));
        let main = placed.main;
        if let Some(z) = &self.zoomed {
            // 日本全体の地図 (と別枠) を海で隠し、寄った地図を描き直す (隠した凡例と別枠の題は上の絵からも消す)
            rect(t.low(), main.x, main.y, main.w, main.h, SEA, 1.0);
            t.clear_over(main);
            draw_land(t.low(), z, &self.neighbors, &self.prefs);
            if let Some(legend) = placed.legend {
                panel::draw_legend(t.top(), &mut self.text, legend);
            }
        }
        for (slot, f) in frames(&self.main, &self.insets, &self.zoomed).into_iter().enumerate() {
            match Shake::of(scene) {
                Some(s) => draw_shake(&mut t, &mut self.text, &self.prefs, f, &s, true, main.right()),
                None => {
                    let (fixed, bounds) = card_room(f, &self.insets, self.zoomed.is_some(), scene.test, &placed);
                    let env = calm::CardEnv {
                        slot,
                        prefs: &self.prefs,
                        fixed: &fixed,
                        bounds,
                        info: calm::info_window(placed.main),
                    };
                    calm::draw(&mut t, &mut self.text, &self.areas, f, scene, &env, &mut self.cards)
                }
            }
            tsunami_coast::draw(t.low(), f, &self.coast, scene.tsunami);
            if let Some(h) = scene.hindsight {
                draw_hindsight(t.top(), &mut self.text, f, h);
            }
        }
        panel::draw_dynamic(t.top(), &mut self.text, scene, &placed);
        if let (Some(f), Some(at), true) = (&self.sub, placed.sub, scene.quake.is_some() || scene.eew.is_some()) {
            draw_sub_map(
                &mut t,
                &mut self.text,
                &self.neighbors,
                &self.prefs,
                &self.coast,
                f,
                scene,
                at,
            );
        }
        // お知らせは平時だけ (右パネルの下半分。地震の画面では出さない)
        // 気象警報が出ているあいだは、同じ矩形を県ごとの一覧に置き換える (告知は一覧の最後のページ)
        let calm = scene.quake.is_none() && scene.eew.is_none();
        let rows = scene.warnings.map(pref_list::pref_rows).unwrap_or_default();
        let list_shown = calm && !rows.is_empty() && placed.notice.is_some();
        if let (true, Some(area)) = (calm, placed.notice) {
            if list_shown {
                let n = scene.notices;
                pref_list::draw(
                    t.top(),
                    &mut self.text,
                    &mut self.notice_lines,
                    &rows,
                    n,
                    scene.now_ms,
                    area,
                );
            } else if let Some(n) = scene.notices {
                notice::draw(t.top(), &mut self.text, &mut self.notice_lines, n, scene.now_ms, area);
            }
        }
        // 警報の帯は定義の banners の矩形の中に描く。平時は気象警報・注意報、地震の画面は緊急性の高いものだけ
        if let Some(at) = placed.banners {
            if scene.quake.is_none() && scene.eew.is_none() {
                banner::draw(t.top(), &mut self.text, scene.warnings, at, scene.now_ms);
            } else {
                quake_band::draw(t.top(), &mut self.text, scene.tsunami, scene.warnings, at, scene.now_ms);
            }
        }
        if scene.test {
            test_mark::draw(t.top(), &mut self.text, &placed);
        }
        t
    }

    /// 描き始めの絵 (動かない部分)。split なら、波より上の札だけの絵も付く
    fn base_target(&mut self, split: bool) -> Target {
        if !split {
            return Target::new(self.base.clone(), false);
        }
        if self.base_split.is_none() {
            let t = self.draw_base(true);
            self.base_split = t.over.map(|o| (t.under, o));
        }
        let (under, over) = self.base_split.as_ref().expect("split");
        Target {
            under: under.clone(),
            over: Some(over.clone()),
        }
    }

    /// 地震波の円を重ねる。札などが塗っている画素は塗らない (波は札の下)。
    pub fn waved(&self, layers: &Layers, waves: &[Wave]) -> Pixmap {
        let mut pm = layers.full.clone();
        self.draw_waves(&mut pm, waves);
        let (now, before) = (pm.data_mut(), layers.full.data());
        for &(at, len) in &layers.covered {
            now[at..at + len].copy_from_slice(&before[at..at + len]);
        }
        pm
    }

    /// 地震波の円 (P 波は青、S 波は赤で、内側をうっすら塗る) を重ねる
    pub fn draw_waves(&self, pm: &mut Pixmap, waves: &[Wave]) {
        self.draw_rings(pm, waves, frames(&self.main, &self.insets, &self.zoomed));
    }

    /// 円の path は日本全体の本図の座標で作り、寄った本図・別枠・サブの地図は変換して映す
    fn draw_rings<'a>(&self, pm: &mut Pixmap, waves: &[Wave], faces: impl IntoIterator<Item = &'a Frame> + Clone) {
        for (p, s) in wave_paths(&self.main.view, waves) {
            for f in faces.clone() {
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
#[allow(clippy::too_many_arguments)]
fn draw_sub_map(
    t: &mut Target,
    text: &mut Text,
    neighbors: &[Shape],
    prefs: &[Shape],
    coast: &[Coast],
    f: &Frame,
    scene: &Scene,
    at: Rect,
) {
    let Rect { x, y, w, h } = at;
    rect(t.low(), x, y, w, h, SEA, 1.0);
    draw_land(t.low(), f, neighbors, prefs);
    if let Some(s) = Shake::of(scene) {
        draw_shake(t, text, prefs, f, &s, false, x + w);
    }
    tsunami_coast::draw(t.low(), f, coast, scene.tsunami);
    // 枠線は波より上 (波の円が枠の 1px を塗りつぶさない)
    let pm = t.top();
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
    t: &mut Target,
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
        frame.fill(t.low(), &s.path, scale_color(*sc), alpha);
        frame.stroke(t.low(), &s.path, edge, 1.0, if shake.forecast { 1.2 } else { 1.0 });
    }
    // ここから先は札と印: 波の円より上
    let pm = t.top();
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
