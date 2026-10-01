//! 1 コマ分の判断と描画 (時計に触れない)。ライブ (render_loop) は今の時刻、記録からの描き直し (replay) は仮の時計で呼ぶ。
//! 前に描いたコマと同じなら描き直さず、None を返す。

use tiny_skia::Pixmap;

use super::camera::{fit_box, map_aspect, target_box, Aim, Camera, Epicenter};
use super::data::{CityWeather, Warnings};
use super::draw::{self, Renderer, Scene};
use super::hindsight::{self, Hindsight};
use super::icon::Icons;
use super::{eew, model, yuv, HISTORY};
use crate::broadcast::record::Shown;
use crate::quake::{Event, Hypocenter};

/// 1 コマの入力 (時刻はサーバの時計。epoch ミリ秒)
pub struct Input<'a> {
    pub events: &'a [Event],
    pub now: u64,
    /// 地震以外のデータが変わるたびに進む数 (描き直しの合図)
    pub rev: u64,
    pub warnings: Option<&'a Warnings>,
    pub weather: Option<&'a CityWeather>,
    pub icons: &'a Icons,
    pub bgm_title: &'a str,
    pub connected: bool,
    pub label: &'a str,
    pub test: bool,
    /// 地震波の描き直しの間隔 (ミリ秒。地震の画面の fps)
    pub check_ms: u64,
    /// 天気の札を今と明日で切り替える間隔 (秒。0 は今だけ)
    pub flip_s: u64,
    /// 記録から描き直すとき、のちの報で分かった震源 (ライブは None)
    pub hindsight: Option<&'a Hindsight>,
    /// 記録から描き直すとき、時計を飛ばした直後か (ライブは false)
    pub fast_forward: bool,
}

/// 描かずに分かる、ある時刻の画面の様子 (記録から描き直すときの、音の判断と終わりの判断に使う)
pub struct Look {
    /// 平時か (地震の画面ではない)
    pub calm: bool,
    /// 地震波を描いているか
    pub waving: bool,
}

/// 描く地震波。地震の画面のときの地震波と、のちに分かった震源の波 (その間は平時の画面でも描く)
fn frame_waves(
    groups: &[model::QuakeSummary],
    eews: &[eew::EewSummary],
    in_quake_screen: bool,
    hindsight: Option<&Hindsight>,
    now: u64,
) -> Vec<eew::Wave> {
    let mut waves = if in_quake_screen {
        eew::waves(groups, eews, now)
    } else {
        Vec::new()
    };
    waves.extend(hindsight::pending(hindsight, groups, eews).and_then(|h| hindsight::wave(h, now)));
    waves
}

pub fn look(events: &[Event], now: u64, hindsight: Option<&Hindsight>) -> Look {
    let groups = model::group_quakes(events);
    let eews = eew::latest_eews(events);
    let current = eew::current(&groups, &eews, now);
    Look {
        calm: current.is_none(),
        waving: !frame_waves(&groups, &eews, current.is_some(), hindsight, now).is_empty(),
    }
}

/// 1 コマの出力
pub struct Output {
    pub i420: Vec<u8>,
    /// 平時か
    pub calm: bool,
    /// 出している地震の最大震度・警報か
    pub shown: Shown,
}

/// 寄った表示範囲の同一判定 (日本全体は None)
type ViewKey = Option<(u64, u64, u64)>;

type StillKey = (u64, Option<(u64, i32)>, Option<u64>, u64, bool, ViewKey);

/// 地震の画面の地震から、寄りの目標の材料を作る。平時は None (日本全体)
fn aim_of(renderer: &Renderer, current: Option<eew::Current>) -> Option<Aim> {
    let make = |h: Option<&Hypocenter>, origin_ms, areas: &[&str], prefs: &[&str], forecast| Aim {
        epicenter: h.and_then(|h| {
            Some(Epicenter {
                lat: h.latitude?,
                lon: h.longitude?,
                depth_km: h.depth_km.map_or(eew::DEFAULT_DEPTH_KM, f64::from),
                origin_ms,
            })
        }),
        shaken: renderer.shaken_box(areas, prefs),
        forecast,
    };
    match current? {
        eew::Current::Quake(q) => {
            let prefs: Vec<&str> = q.pref_scales.iter().map(|(p, _)| p.as_str()).collect();
            Some(make(q.hypocenter.as_ref(), q.origin_ms, &[], &prefs, false))
        }
        eew::Current::Eew(e) => {
            let areas: Vec<&str> = e.area_scales.iter().map(|(a, _)| a.as_str()).collect();
            let prefs: Vec<&str> = e.pref_scales.iter().map(|(p, _)| p.as_str()).collect();
            Some(make(e.hypocenter.as_ref(), e.origin_ms, &areas, &prefs, true))
        }
    }
}

/// 描き直す必要があるかの覚えと、1 秒ごとに描く文字・塗りの絵
pub struct Stepper {
    renderer: Renderer,
    /// 震源へ寄るカメラ (コマの時刻で進める。寄りが無効なら動かない)
    camera: Camera,
    last_key: Option<(StillKey, u64)>,
    still: Option<(StillKey, Pixmap)>,
}

impl Stepper {
    pub fn new(renderer: Renderer) -> Self {
        Stepper {
            renderer,
            camera: Camera::new(),
            last_key: None,
            still: None,
        }
    }

    /// 日本全体の表示の幅 / いまの表示の幅 (1 なら日本全体)
    #[cfg(test)]
    pub fn zoom_ratio(&self) -> f64 {
        super::camera::home_fit().w / self.camera.fit().w
    }

    /// コマの時刻 now で寄りを進め、描く面に渡す。寄っているときの表示範囲の判定を返す
    fn aim_camera(&mut self, current: Option<eew::Current>, now: u64, snap: bool) -> ViewKey {
        if !self.renderer.zoom_enabled() {
            return None;
        }
        let target = aim_of(&self.renderer, current)
            .and_then(|a| target_box(&a, now))
            .map(|b| fit_box(b, map_aspect()));
        self.camera.advance(target, now, snap);
        let zoomed = (!self.camera.is_home()).then(|| self.camera.fit());
        self.renderer.set_view(zoomed);
        zoomed.map(|f| (f.x.to_bits(), f.y.to_bits(), f.w.to_bits()))
    }

    /// 前のコマから変わっていなければ None
    pub fn step(&mut self, i: &Input) -> Option<Output> {
        let now = i.now;
        let groups = model::group_quakes(i.events);
        let eews = eew::latest_eews(i.events);
        let current = eew::current(&groups, &eews, now);
        let (quake, shown_eew) = match current {
            Some(eew::Current::Quake(q)) => (Some(q), None),
            Some(eew::Current::Eew(e)) => (None, Some(e)),
            None => (None, None),
        };
        let view = self.aim_camera(current, now, i.fast_forward);
        // 地震波は地震の画面のときだけ描く (のちに分かった震源の波は、平時の画面でも描く)
        let waves = frame_waves(&groups, &eews, current.is_some(), i.hindsight, now);
        // 地震波以外を描き直すのは、データが変わったとき・平時と地震が切り替わったとき・秒が進んだときだけ。
        // 地震波が動いている間は、その上にコマごとに波だけを重ねる
        let still_key = (
            i.rev,
            quake.map(|q| (q.updated_ms, q.max_scale.0)),
            shown_eew.map(|e| e.received_ms),
            now / 1000,
            i.fast_forward,
            view,
        );
        let key = (still_key, if waves.is_empty() { 0 } else { now / i.check_ms });
        if self.last_key == Some(key) {
            return None;
        }
        self.last_key = Some(key);
        if self.still.as_ref().is_none_or(|(k, _)| *k != still_key) {
            let scene = Scene {
                quake,
                eew: shown_eew,
                history: &groups[..groups.len().min(HISTORY)],
                warnings: i.warnings,
                weather: i.weather,
                icons: i.icons,
                flip_s: i.flip_s,
                now_ms: now,
                connected: i.connected,
                bgm_title: i.bgm_title,
                label: i.label,
                test: i.test,
                hindsight: hindsight::pending(i.hindsight, &groups, &eews),
                fast_forward: i.fast_forward,
            };
            self.still = Some((still_key, self.renderer.render(&scene)));
        }
        let (_, base) = self.still.as_ref()?;
        let with_waves;
        let pm = if waves.is_empty() {
            base
        } else {
            let mut pm = base.clone();
            self.renderer.draw_waves(&mut pm, &waves);
            with_waves = pm;
            &with_waves
        };
        Some(Output {
            i420: yuv::rgba_to_i420(pm.data(), draw::W as usize, draw::H as usize),
            calm: current.is_none(),
            shown: eew::shown(current),
        })
    }
}
