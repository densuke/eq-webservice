//! 描画の確認 (実際の地図データで 1 コマ描き、決まった位置の画素を確かめる)。文字は描かない設定で行う (CI にフォントが無くてもよい)。
//! EQ_NATIVE_PNG_DIR を指定すると、確認用の画面を PNG に書き出す (フォントは EQ_NATIVE_FONT)。

use tiny_skia::Pixmap;

use super::data::{City, CityWeather, Kind, Tomorrow, Warnings};
use super::draw::{Renderer, Scene};
use super::eew::{EewSummary, Wave};
use super::frame::{Frame, OKINAWA};
use super::geo::{self, View};
use super::model::{scale_color, QuakeSummary};
use super::paint::SEA;
use super::placed::Placed;
use super::text::Text;
use super::*;
use crate::broadcast::status::{Notice, Outage};
use crate::quake::{Hypocenter, Scale};

const NOW: u64 = 1_790_000_000_000;
static NO_ICONS: std::sync::LazyLock<Icons> = std::sync::LazyLock::new(Icons::new);

/// 組み込みの定義 (broadcast) の地図の枠・別枠の矩形
fn map_rect() -> (f64, f64, f64, f64) {
    Placed::builtin().unwrap().main.tuple64()
}

fn inset_rect() -> super::layout_resolve::Rect {
    Placed::builtin().unwrap().inset.unwrap()
}

pub(super) fn renderer(text: Text) -> Renderer {
    renderer_with(text, Placed::builtin().unwrap())
}

fn renderer_with(text: Text, placed: Placed) -> Renderer {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let view = View::fit_home(placed.main.tuple64());
    let prefs = geo::load(&dir.join("japan.geojson"), "name", &view).unwrap();
    let areas = geo::load(&dir.join("warning-areas.geojson"), "code", &view).unwrap();
    let neighbors = geo::load(&dir.join("neighbors.geojson"), "name", &view).unwrap();
    Renderer::new(view, neighbors, prefs, areas, text, placed)
}

fn center_of(pref: &str) -> (u32, u32) {
    let view = View::fit_home(map_rect());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let shapes = geo::load(&dir.join("japan.geojson"), "name", &view).unwrap();
    let c = shapes.iter().find(|s| s.key == pref).unwrap().center;
    (c.0 as u32, c.1 as u32)
}

/// 日本海の画素 (陸から離れた海)
fn sea_px() -> (u32, u32) {
    let (x, y) = View::fit_home(map_rect()).px(134.0, 40.5);
    (x as u32, y as u32)
}

pub(super) fn rgb(pm: &Pixmap, (x, y): (u32, u32)) -> [u8; 3] {
    let p = pm.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue()]
}

pub(super) fn quake(max: Scale, prefs: &[(&str, Scale)], at: Option<(f64, f64)>) -> QuakeSummary {
    QuakeSummary {
        updated_ms: NOW,
        origin_time: "2026/09/30 12:00:00".into(),
        origin_ms: Some(NOW as i64),
        hypocenter: at.map(|(lat, lon)| Hypocenter {
            name: "石川県能登地方".into(),
            latitude: Some(lat),
            longitude: Some(lon),
            depth_km: Some(10),
            magnitude: Some(6.5),
        }),
        max_scale: max,
        tsunami: "None".into(),
        pref_scales: prefs.iter().map(|(p, s)| (p.to_string(), *s)).collect(),
        points: Vec::new(),
    }
}

/// 右パネルの見出しの札の左端の x (右パネルの左端 900 + 余白 16 + 70)
const SIDE: u32 = 900 + 16 + 70;

fn eew(warning: bool, prefs: &[(&str, Scale)], at: Option<(f64, f64)>) -> EewSummary {
    EewSummary {
        event_id: "e".into(),
        serial: "3".into(),
        received_ms: NOW,
        warning,
        test: false,
        origin_time: "2026/09/30 12:00:00".into(),
        origin_ms: Some(NOW as i64),
        hypocenter: at.map(|(lat, lon)| Hypocenter {
            name: "石川県能登地方".into(),
            latitude: Some(lat),
            longitude: Some(lon),
            depth_km: Some(10),
            magnitude: Some(6.5),
        }),
        max_scale: prefs.iter().map(|p| p.1).max().unwrap_or(Scale::UNKNOWN),
        pref_scales: prefs.iter().map(|(p, s)| (p.to_string(), *s)).collect(),
        area_scales: Vec::new(),
    }
}

pub(super) fn scene<'a>(
    quake: Option<&'a QuakeSummary>,
    history: &'a [QuakeSummary],
    warnings: Option<&'a Warnings>,
    weather: Option<&'a CityWeather>,
) -> Scene<'a> {
    Scene {
        icons: &NO_ICONS,
        flip_s: 0,
        quake,
        eew: None,
        history,
        warnings,
        weather,
        now_ms: NOW,
        connected: true,
        bgm_title: "テスト曲",
        label: "",
        test: false,
        hindsight: None,
        fast_forward: false,
        status: None,
        notices: None,
    }
}

#[test]
fn parts_missing_from_the_layout_are_not_drawn() {
    use super::layout_def::{parse, pick};
    let def = pick(
        &parse(r#"{"version":1,"layouts":[{"name":"t","root":{"dir":"column","children":[{"slot":"main"}]}}]}"#)
            .unwrap(),
        "t",
    )
    .unwrap()
    .clone();
    let mut r = renderer_with(Text::none(), Placed::new(&def, &def).unwrap());
    let g = quake(Scale(40), &[("石川県", Scale(40))], Some((37.5, 137.2)));
    let pm = r.render(&scene(Some(&g), &[], None, None));
    // 上部バーが無いので上端は地図 (バーの地の色ではない)。時計 (緑の枠) も描かれない
    assert_ne!(rgb(&pm, (640, 5)), super::paint::PANEL);
    assert!(!near(&pm, (716, 670), [0x3f, 0xb9, 0x50]));
}

#[test]
fn calm_frame_shows_the_sea_and_a_warned_prefecture() {
    let mut r = renderer(Text::none());
    let kind = Kind {
        name: "レベル３大雨警報".into(),
    };
    let warnings = Warnings {
        areas: r_areas_of_tokyo()
            .into_iter()
            .map(|c| (c, vec![kind.clone()]))
            .collect(),
        ..Default::default()
    };
    let pm = r.render(&scene(None, &[], Some(&warnings), None));
    assert_eq!((pm.width(), pm.height()), (1280, 720));
    assert_eq!(rgb(&pm, sea_px()), SEA); // 日本海
                                         // 警報の赤 (陸に半透明で重なる) が地図に出る。県の全体を塗るのではなく、市町村等の区域だけを塗る
                                         // (左下の凡例と上の帯にも赤があるので、数えるのは凡例より右・帯より下)
    let red = |pm: &Pixmap| {
        let w = pm.width() as usize;
        let hit = |(i, p): (usize, &tiny_skia::PremultipliedColorU8)| {
            i % w >= 100 && i / w >= 100 && p.red() > 150 && p.green() < 80 && p.blue() < 80
        };
        pm.pixels().iter().enumerate().filter(|&e| hit(e)).count()
    };
    assert!(red(&pm) > 50, "{}", red(&pm));
    // 警報の無い県は陸の色のまま
    assert_eq!(rgb(&pm, center_of("長野県")), [0x3a, 0x42, 0x50]);
    // 警報が無ければ、赤は出ない
    let calm = r.render(&scene(None, &[], None, None));
    assert_eq!(red(&calm), 0);
}

/// 東京都の市町村等のコード (警報の区域のデータから)
fn r_areas_of_tokyo() -> Vec<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let view = View::fit_home(map_rect());
    let areas = geo::load(&dir.join("warning-areas.geojson"), "code", &view).unwrap();
    areas
        .into_iter()
        .map(|s| s.key)
        .filter(|k| k.starts_with("13"))
        .collect()
}

#[test]
fn quake_frame_paints_the_prefecture_with_its_scale_color() {
    let mut r = renderer(Text::none());
    let q = quake(
        Scale::S4,
        &[("東京都", Scale::S4), ("長野県", Scale::S2)],
        Some((35.7, 139.7)),
    );
    let pm = r.render(&scene(Some(&q), &[], None, None));
    assert_eq!(rgb(&pm, center_of("東京都")), [0xfa, 0xf5, 0x00]); // 震度 4
    assert_eq!(rgb(&pm, center_of("長野県")), [0x00, 0xaa, 0xff]); // 震度 2
    assert_eq!(rgb(&pm, center_of("愛知県")), [0x3a, 0x42, 0x50]); // 揺れていない県
    assert_eq!(rgb(&pm, sea_px()), SEA);
}

#[test]
fn an_eew_frame_paints_the_forecast_translucent_with_an_epicenter_and_a_colored_band() {
    let mut r = renderer(Text::none());
    let warning = eew(true, &[("石川県", Scale::S5_LOWER)], Some((37.5, 137.2)));
    let mut sc = scene(None, &[], None, None);
    sc.eew = Some(&warning);
    let pm = r.render(&sc);
    // 予測の県は、震度の色が陸の色に半透明で重なる (観測のような真っ直ぐの色にはならない)
    let painted = rgb(&pm, center_of("石川県"));
    assert_ne!(painted, scale_color(Scale::S5_LOWER));
    assert_ne!(painted, [0x3a, 0x42, 0x50]);
    assert!(
        painted[0] > 0x3a && painted[1] > 0x42 && painted[2] < 0x50,
        "{painted:?}"
    );
    assert_eq!(rgb(&pm, center_of("愛知県")), [0x3a, 0x42, 0x50]);
    // 震源の ✕ (中心は赤)
    let (x, y) = View::fit_home(map_rect()).px(137.2, 37.5);
    assert!(near(&pm, (x as u32, y as u32), [0xe0, 0x1e, 0x1e]));
    // 右パネルの見出しの札: 警報は赤、予報は橙 (文字の左の余白)
    assert_eq!(rgb(&pm, (SIDE + 3, 112)), [0xd7, 0x26, 0x3d]);
    let forecast = eew(false, &[("石川県", Scale::S4)], Some((37.5, 137.2)));
    sc.eew = Some(&forecast);
    assert_eq!(rgb(&r.render(&sc), (SIDE + 3, 112)), [0xb3, 0x59, 0x00]);
    // 地震情報があれば、そちらを出す (観測の色、札は無い)
    let q = quake(Scale::S4, &[("石川県", Scale::S4)], Some((37.5, 137.2)));
    sc.quake = Some(&q);
    let pm = r.render(&sc);
    assert_eq!(rgb(&pm, center_of("石川県")), [0xfa, 0xf5, 0x00]);
    assert_ne!(rgb(&pm, (SIDE + 3, 112)), [0xb3, 0x59, 0x00]);
}

#[test]
fn a_wave_is_drawn_as_a_ring_that_grows_with_the_radius() {
    let mut r = renderer(Text::none());
    let (lat, lon) = (37.5, 137.2);
    let q = quake(Scale::S4, &[], Some((lat, lon)));
    let wave = |km: f64| Wave {
        lat,
        lon,
        p_km: None,
        s_km: Some(km),
    };
    let view = View::fit_home(map_rect());
    // 円周の、真東・真北の点 (緯度 1 度 = 111.19km)
    let east = |km: f64| view.px(lon + km / (111.19 * lat.to_radians().cos()), lat);
    let north = |km: f64| view.px(lon, lat + km / 111.19);
    let px = |p: (f32, f32)| (p.0 as u32, p.1 as u32);
    let s_red = [0xff, 0x52, 0x52];
    let sc = scene(Some(&q), &[], None, None);
    let still = r.render(&sc);
    let with = |r: &Renderer, waves: &[Wave]| {
        let mut pm = still.clone();
        r.draw_waves(&mut pm, waves);
        pm
    };
    let ring = with(&r, &[wave(200.0)]);
    assert!(near(&ring, px(east(200.0)), s_red));
    assert!(near(&ring, px(north(200.0)), s_red));
    assert!(!near(&ring, px(east(100.0)), s_red)); // 内側には線が無い
                                                   // 半径が変われば、線の場所も動く (コマごとに描き直す)
    let small = with(&r, &[wave(100.0)]);
    assert!(near(&small, px(east(100.0)), s_red));
    assert!(!near(&small, px(east(200.0)), s_red));
    // P 波は青。波が無ければ、どちらも出ない
    let p_only = Wave {
        p_km: Some(200.0),
        s_km: None,
        ..wave(0.0)
    };
    assert!(near(&with(&r, &[p_only]), px(east(200.0)), [0x4f, 0xc3, 0xf7]));
    assert!(!near(&with(&r, &[]), px(east(200.0)), s_red));
}

#[test]
fn neighbor_countries_are_drawn_under_japan_and_a_missing_file_is_fine() {
    let mut r = renderer(Text::none());
    let pm = r.render(&scene(None, &[], None, None));
    let (x, y) = View::fit_home(map_rect()).px(126.98, 37.57); // ソウル
    assert_eq!(rgb(&pm, (x as u32, y as u32)), paint::NEIGHBOR);
    assert_eq!(rgb(&pm, center_of("長野県")), [0x3a, 0x42, 0x50]); // 日本の県はその上
                                                                   // load_renderer は neighbors.geojson が無くても動く
    let dir = tempfile::tempdir().unwrap();
    let web = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    for f in ["japan.geojson", "warning-areas.geojson"] {
        std::fs::copy(web.join(f), dir.path().join(f)).unwrap();
    }
    let cfg = BroadcastConfig {
        map_dir: dir.path().display().to_string(),
        font: "/nonexistent".into(),
        ..BroadcastConfig::default()
    };
    let mut bare = load_renderer(&cfg).unwrap();
    let pm = bare.render(&scene(None, &[], None, None));
    assert_eq!(rgb(&pm, (x as u32, y as u32)), SEA);
}

/// 中心から 3 画素以内に、その色 (各成分の差が 3 以内) の画素があるか
fn near(pm: &Pixmap, (cx, cy): (u32, u32), want: [u8; 3]) -> bool {
    (cy - 3..=cy + 3)
        .any(|y| (cx - 3..=cx + 3).any(|x| rgb(pm, (x, y)).iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 3)))
}

/// 別枠の中の画素 (那覇のあたり)
fn okinawa_px() -> (u32, u32) {
    let main = View::fit_home(map_rect());
    let inset = frame::Frame::inset(&main, &frame::OKINAWA, inset_rect()).unwrap();
    let (x, y) = inset.view.px(127.95, 26.5); // 沖縄本島
    (x as u32, y as u32)
}

#[test]
fn an_epicenter_just_west_of_the_inset_is_pinned_to_its_corner() {
    let mut r = renderer(Text::none());
    // 台湾の東の海 (23.6N 121.5E) は、本図にも南西諸島の枠にも入らない
    let e = eew(false, &[], Some((23.6, 121.5)));
    let mut sc = scene(None, &[], None, None);
    sc.eew = Some(&e);
    let pm = r.render(&sc);
    assert!(near(&pm, (18, 308), [0xe0, 0x1e, 0x1e])); // 枠 (10,96 から高さ 220) の左下の隅
                                                       // 遠い震央 (台湾の西) は、どこにも印を置かない
    let far = eew(false, &[], Some((23.6, 118.0)));
    sc.eew = Some(&far);
    assert!(!near(&r.render(&sc), (18, 308), [0xe0, 0x1e, 0x1e]));
}

#[test]
fn the_okinawa_inset_is_drawn_with_land_and_the_scale_color() {
    let mut r = renderer(Text::none());
    let calm = r.render(&scene(None, &[], None, None));
    assert!(near(&calm, okinawa_px(), [0x3a, 0x42, 0x50])); // 別枠の中の沖縄本島は陸 (細い島なので、まわりも見る)
    assert_eq!(rgb(&calm, (50, 110)), SEA); // 枠の中の海 (枠は 10,96 から)
    let line = rgb(&calm, (9, 150)); // 枠線 (角の丸めで少しにじむ)
    assert!(line[2] > 60 && line[2] < 0x53, "{line:?}");
    let q = quake(Scale::S3, &[("沖縄県", Scale::S3)], None);
    let shaken = r.render(&scene(Some(&q), &[], None, None));
    assert!(near(&shaken, okinawa_px(), [0x00, 0x41, 0xff])); // 震度 3
}

#[test]
fn warnings_in_okinawa_are_drawn_in_the_inset_too() {
    let mut r = renderer(Text::none());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let view = View::fit_home(map_rect());
    let areas = geo::load(&dir.join("warning-areas.geojson"), "code", &view).unwrap();
    let kind = vec![Kind {
        name: "特別警報".into(),
    }];
    let warnings = Warnings {
        areas: areas
            .into_iter()
            .filter(|s| s.key.starts_with("47"))
            .map(|s| (s.key, kind.clone()))
            .collect(),
        ..Default::default()
    };
    let pm = r.render(&scene(None, &[], Some(&warnings), None));
    assert!(near(&pm, okinawa_px(), [0x13, 0x0a, 0x16])); // 特別警報の暗い色 (85%) が陸に重なる
}

#[test]
fn the_weather_icon_replaces_the_kanji_and_is_shown_at_night_too() {
    let mut r = renderer(Text::none());
    let weather = CityWeather {
        cities: vec![City {
            name: "東京".into(),
            lat: 35.69,
            lon: 139.69,
            code: "100".into(),
            temp: Some(24.0),
            tomorrow: None,
        }],
        rain: vec![],
    };
    let orange = |pm: &Pixmap| {
        pm.pixels()
            .iter()
            .filter(|p| p.red() > 230 && (90..115).contains(&p.green()) && p.blue() < 20)
            .count()
    };
    let none = r.render(&scene(None, &[], None, Some(&weather)));
    assert_eq!(orange(&none), 0);
    // アイコンが取れていれば札の中に出る (昼と夜で別のファイル)
    let day = icon::name_for("100", NOW - NOW % 86_400_000 + 3 * 3_600_000).unwrap(); // 12 時 JST
    let night = icon::name_for("100", NOW - NOW % 86_400_000 + 12 * 3_600_000).unwrap(); // 21 時 JST
    assert_ne!(day, night);
    let sun = icon::rasterize(include_bytes!("testdata/sun.svg")).unwrap();
    let icons: Icons = [(day.to_string(), sun)].into();
    let mut with = scene(None, &[], None, Some(&weather));
    with.icons = &icons;
    with.now_ms = NOW - NOW % 86_400_000 + 3 * 3_600_000;
    assert!(orange(&r.render(&with)) > 100);
    // 夜のアイコンはまだ取れていないので、漢字 (文字なしの設定なので何も出ない) に戻る
    with.now_ms = NOW - NOW % 86_400_000 + 12 * 3_600_000;
    assert_eq!(orange(&r.render(&with)), 0);
}

#[test]
fn the_card_shows_tomorrow_every_other_interval_and_only_when_asked() {
    let mut r = renderer(Text::none());
    let city = City {
        name: "東京".into(),
        lat: 35.69,
        lon: 139.69,
        code: "100".into(),
        temp: Some(24.0),
        tomorrow: Some(Tomorrow {
            code: "101".into(),
            temp_min: Some(17.0),
            temp_max: Some(24.0),
            pop: Some(30),
        }),
    };
    let weather = CityWeather {
        cities: vec![city],
        rain: vec![],
    };
    let orange = |pm: &Pixmap| {
        pm.pixels()
            .iter()
            .filter(|p| p.red() > 230 && (90..115).contains(&p.green()) && p.blue() < 20)
            .count()
    };
    // 今の天気 (100) のアイコンは取れていない。明日の天気 (101) のアイコンだけ取れている
    let sun = icon::rasterize(include_bytes!("testdata/sun.svg")).unwrap();
    let icons: Icons = [("101.svg".to_string(), sun)].into();
    let mut sc = scene(None, &[], None, Some(&weather));
    sc.icons = &icons;
    let noon = NOW - NOW % 86_400_000 + 3 * 3_600_000; // 12 時 JST (20 秒の区切りの頭)
    sc.flip_s = 20;
    sc.now_ms = noon + 5_000;
    assert_eq!(orange(&r.render(&sc)), 0); // 今の札
    sc.now_ms = noon + 25_000;
    assert!(orange(&r.render(&sc)) > 100); // 明日の札
    sc.now_ms = noon + 45_000;
    assert_eq!(orange(&r.render(&sc)), 0);
    // 0 なら切り替えない
    sc.flip_s = 0;
    sc.now_ms = noon + 25_000;
    assert_eq!(orange(&r.render(&sc)), 0);
}

/// 情報の窓の中で、2 つの画面の画素が違うところの数 (縁を除く)
fn window_diff(a: &Pixmap, b: &Pixmap) -> usize {
    let (x, y, w, h) = super::calm::info_window(Placed::builtin().unwrap().main);
    (y as u32 + 4..(y + h) as u32 - 4)
        .flat_map(|py| (x as u32 + 4..(x + w) as u32 - 4).map(move |px| (px, py)))
        .filter(|&p| rgb(a, p) != rgb(b, p))
        .count()
}

#[test]
fn the_info_window_sits_in_the_sea_clear_of_land_the_inset_and_the_banners() {
    let (x, y, w, h) = super::calm::info_window(Placed::builtin().unwrap().main);
    let inset = Frame::inset(&View::fit_home(map_rect()), &OKINAWA, inset_rect()).unwrap();
    let ((ix, _, iw, _), _) = inset.inset_box().unwrap();
    assert!(x > ix + iw); // 南西諸島の別枠の右
    assert!(y > map_rect().1 as f32 + super::test_mark::BAND_H); // 地図の上端・テスト配信の帯の下 (警報の帯は地図の外)
    assert!(x + w < 900.0 && y + h < 720.0 - 10.0 - 147.0); // 地図の中。左下の凡例より上
                                                            // 陸 (周辺国と日本) を描いた画面で、窓とその周り 6px が全部海の色
    let mut r = renderer(Text::none());
    let plain = r.render(&scene(None, &[], None, None));
    for py in (y as u32 - 6)..(y + h) as u32 + 6 {
        for px in (x as u32 - 6)..(x + w) as u32 + 6 {
            assert_eq!(rgb(&plain, (px, py)), SEA, "({px}, {py}) が陸にかかっている");
        }
    }
}

#[test]
fn the_info_window_names_what_the_cards_show_in_the_same_frame() {
    let weather = CityWeather {
        cities: vec![City {
            name: "東京".into(),
            lat: 35.69,
            lon: 139.69,
            code: "100".into(),
            temp: Some(24.0),
            tomorrow: Some(Tomorrow {
                code: "101".into(),
                temp_min: Some(17.0),
                temp_max: Some(24.0),
                pop: Some(30),
            }),
        }],
        rain: vec![],
    };
    let noon = NOW - NOW % 86_400_000 + 3 * 3_600_000; // 12 時 JST (20 秒の区切りの頭)
    let frame = |r: &mut Renderer, weather: Option<&CityWeather>, quake: Option<&QuakeSummary>, at: u64| {
        let mut sc = scene(quake, &[], None, weather);
        sc.flip_s = 20;
        sc.now_ms = at;
        r.render(&sc)
    };
    let font = BroadcastConfig::default().font;
    let mut r = renderer(Text::load(&font, 0).unwrap_or_else(|_| Text::none()));
    let plain = frame(&mut r, None, None, noon + 5_000);
    let now = frame(&mut r, Some(&weather), None, noon + 5_000);
    let tomorrow = frame(&mut r, Some(&weather), None, noon + 25_000);
    // 窓の色が、窓の中にある (窓の枠の画素が海の色でなくなる)
    let (x, y, _, _) = super::calm::info_window(Placed::builtin().unwrap().main);
    assert_eq!(rgb(&plain, (x as u32 + 30, y as u32)), SEA);
    assert_ne!(rgb(&now, (x as u32 + 30, y as u32)), SEA);
    assert_ne!(rgb(&tomorrow, (x as u32 + 30, y as u32)), SEA);
    // 地震の画面では出さない
    let q = quake(Scale::S3, &[("東京都", Scale::S3)], None);
    let shake = frame(&mut r, Some(&weather), Some(&q), noon + 5_000);
    assert_eq!(rgb(&shake, (x as u32 + 30, y as u32)), SEA);
    // 文字 (フォント) が読める環境でだけ、案内の文字が出て、切り替えで変わる。読めなければ窓の枠だけで同じ
    if r.text.enabled() {
        assert!(window_diff(&now, &tomorrow) > 100, "案内の文が変わる");
    } else {
        assert_eq!(window_diff(&now, &tomorrow), 0);
    }
}

#[test]
fn the_same_state_gives_the_same_frame() {
    let mut r = renderer(Text::none());
    let a = r.render(&scene(None, &[], None, None));
    let b = r.render(&scene(None, &[], None, None));
    assert_eq!(a.data(), b.data()); // 同じ状態なら同じ画面
}

#[test]
fn ws_messages_update_the_state_and_dedupe_events() {
    let ev = |id: &str, t: u64| {
        serde_json::json!({"id": id, "source": "t", "received_at_ms": t, "kind": "quake",
            "info_type": "destination", "origin_time": "2026/09/30 12:00:00", "origin_time_ms": t,
            "issued_at": "", "hypocenter": null, "max_scale": 30, "domestic_tsunami": "None",
            "points": [], "pref_max": [], "comment": ""})
    };
    let mut s = State::default();
    let hello = serde_json::json!({"type": "hello", "server_time_ms": 1_000, "events": [ev("a", 1), {"bad": 1}]});
    apply(&mut s, serde_json::from_value(hello).unwrap());
    let again = serde_json::json!({"type": "event", "server_time_ms": 2_000, "event": ev("a", 1)});
    apply(&mut s, serde_json::from_value(again).unwrap());
    let next = serde_json::json!({"type": "event", "server_time_ms": 2_000, "event": ev("b", 2)});
    apply(&mut s, serde_json::from_value(next).unwrap());
    assert_eq!(s.events.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    // 時計のずれ: 直近のメッセージの server_time_ms と今の差 (テストの実行時刻が入るので、向きだけ確かめる)
    assert!(s.offset < 0);
}

#[test]
fn server_url_becomes_a_websocket_url() {
    assert_eq!(ws_url("https://eq.fuga.jp"), "wss://eq.fuga.jp/ws");
    assert_eq!(ws_url("http://localhost:8080"), "ws://localhost:8080/ws");
}

/// 札の右端 (文字の無いテストでは、曲名の幅 0 + 余白 16 のぶん、配信元の名前が無い右端 1264 から左)
const CHIP_X: u32 = 1264 - 16 - 4;

#[test]
fn the_status_chip_is_drawn_in_the_top_bar_only_when_asked() {
    let mut r = renderer(Text::none());
    let plain = r.render(&scene(None, &[], None, None));
    let mut with = |n| {
        r.render(&Scene {
            status: n,
            ..scene(None, &[], None, None)
        })
    };
    let busy = with(Some(Notice::Busy));
    let outage = with(Some(Notice::Outage(Outage {
        from_ms: NOW,
        to_ms: NOW + 60_000,
    })));
    let at = (CHIP_X, 20);
    assert_ne!(rgb(&plain, at), chip::BUSY_BG);
    assert_eq!(rgb(&busy, at), chip::BUSY_BG);
    assert_eq!(rgb(&outage, at), chip::OUTAGE_BG);
    // 上部バーの下 (地図・パネル・帯) は変わらない
    let below = |pm: &Pixmap| pm.data()[(36 * 1280 * 4)..].to_vec();
    assert_eq!(below(&busy), below(&plain));
    assert_eq!(below(&outage), below(&plain));
}

#[test]
fn the_stepper_redraws_when_the_status_changes() {
    let mut s = Stepper::new(renderer(Text::none()));
    let plain_in = input(&[], NOW, None);
    let busy_in = Input {
        status: Some(Notice::Busy),
        ..input(&[], NOW, None)
    };
    let plain = s.step(&plain_in).unwrap();
    assert!(s.step(&plain_in).is_none());
    let busy = s.step(&busy_in).unwrap();
    assert_ne!(busy.i420, plain.i420);
    assert!(s.step(&busy_in).is_none());
    assert_eq!(s.step(&plain_in).unwrap().i420, plain.i420);
}

/// 確認用の画面を PNG に書く (EQ_NATIVE_PNG_DIR があるときだけ)
#[test]
fn write_fixture_pngs_when_asked() {
    let Ok(out) = std::env::var("EQ_NATIVE_PNG_DIR") else {
        return;
    };
    let text = std::env::var("EQ_NATIVE_FONT")
        .ok()
        .and_then(|p| Text::load(&p, 0).ok())
        .unwrap_or_else(Text::none);
    let mut r = renderer(text);
    let noto = quake(
        Scale::S7,
        &[
            ("石川県", Scale::S7),
            ("富山県", Scale::S5_UPPER),
            ("新潟県", Scale::S5_LOWER),
            ("福井県", Scale::S4),
            ("長野県", Scale::S3),
            ("岐阜県", Scale::S3),
            ("山形県", Scale::S2),
            ("京都府", Scale::S1),
        ],
        Some((37.5, 137.2)),
    );
    let old = quake(Scale::S3, &[("千葉県", Scale::S3)], Some((35.3, 140.3)));
    let history = [noto.clone(), old];
    let quake_png = r.render(&scene(Some(&noto), &history, None, None));
    quake_png
        .save_png(std::path::Path::new(&out).join("quake.png"))
        .unwrap();
    let kind = |n: &str| vec![Kind { name: n.into() }];
    let warnings = Warnings {
        areas: r_areas_of_tokyo()
            .into_iter()
            .map(|c| (c, kind("レベル３大雨警報")))
            .chain([("2810000".to_string(), kind("レベル２大雨注意報"))])
            .collect(),
        ..Default::default()
    };
    let city = |name: &str, lat, lon, code: &str, t| City {
        name: name.into(),
        lat,
        lon,
        code: code.into(),
        temp: Some(t),
        tomorrow: Some(Tomorrow {
            code: "101".into(),
            temp_min: Some(t - 7.0),
            temp_max: Some(t + 1.0),
            pop: Some(30),
        }),
    };
    let weather = CityWeather {
        cities: vec![
            city("東京", 35.69, 139.69, "100", 24.0),
            city("大阪", 34.69, 135.5, "200", 26.0),
            city("札幌", 43.06, 141.35, "300", 15.0),
        ],
        rain: vec![[35.0, 137.0, 12.0], [34.0, 135.0, 32.0]],
    };
    let calm_png = r.render(&scene(None, &history[1..], Some(&warnings), Some(&weather)));
    calm_png.save_png(std::path::Path::new(&out).join("calm.png")).unwrap();
    let notices = Notices {
        interval_s: 20,
        texts: vec!["毎日 4:00〜4:15 ごろ、システムメンテナンスのため数分配信が途切れることがあります".into()],
    };
    let notice_png = r.render(&Scene {
        notices: Some(&notices),
        ..scene(None, &history[1..], Some(&warnings), Some(&weather))
    });
    notice_png
        .save_png(std::path::Path::new(&out).join("notice.png"))
        .unwrap();
    // 石狩市に警報: 札幌の札が石狩の塗りを隠さず、引き出し線で海へ逃げる
    let ishikari = Warnings {
        areas: [("0123500".to_string(), kind("レベル３大雨警報"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    r.render(&scene(None, &[], Some(&ishikari), Some(&weather)))
        .save_png(std::path::Path::new(&out).join("ishikari.png"))
        .unwrap();
    // 状態の札: 混雑中 (警報の帯が出ている平時)・途切れた (地震の画面)・曲名が長いとき
    let busy = Some(Notice::Busy);
    let outage = Some(Notice::Outage(Outage {
        from_ms: NOW - 25 * 60_000,
        to_ms: NOW,
    }));
    r.render(&Scene {
        status: busy,
        ..scene(None, &history[1..], Some(&ishikari), Some(&weather))
    })
    .save_png(std::path::Path::new(&out).join("status_busy.png"))
    .unwrap();
    r.render(&Scene {
        status: outage,
        ..scene(Some(&noto), &history, None, None)
    })
    .save_png(std::path::Path::new(&out).join("status_outage.png"))
    .unwrap();
    r.render(&Scene {
        status: busy,
        bgm_title: "とても長い曲名のBGM とても長い曲名のBGM とても長い曲名のBGM とても長い曲名のBGM",
        label: "配信元の名前",
        test: true,
        ..scene(None, &history[1..], Some(&ishikari), Some(&weather))
    })
    .save_png(std::path::Path::new(&out).join("status_busy_long_bgm.png"))
    .unwrap();
    // テスト配信の帯と警報の帯 (2 行) の下にも、情報の窓が重ならない
    let test_png = r.render(&Scene {
        test: true,
        ..scene(None, &history[1..], Some(&warnings), Some(&weather))
    });
    test_png.save_png(std::path::Path::new(&out).join("test.png")).unwrap();
}

fn test_scene<'a>(quake: Option<&'a QuakeSummary>, history: &'a [QuakeSummary]) -> Scene<'a> {
    Scene {
        test: true,
        ..scene(quake, history, None, None)
    }
}

#[test]
fn test_broadcast_draws_red_bands_and_a_watermark_even_without_a_font() {
    let mut r = renderer(Text::none());
    let plain = r.render(&scene(None, &[], None, None));
    let marked = r.render(&test_scene(None, &[]));
    let red = super::test_mark::BAND;
    // 地図の上端 (警報の帯の下) と最下部の帯 (文字の無い端の画素)
    assert_eq!(rgb(&marked, (5, 90)), red);
    assert_eq!(rgb(&marked, (5, 715)), red);
    assert_ne!(rgb(&plain, (5, 90)), red);
    // 地図の中央 (E の縦棒の上) に、うすい白が乗る
    let (mx, my) = (450 - 256 + 134 + 5, 403 - 95 + 90);
    assert_ne!(rgb(&marked, (mx, my)), rgb(&plain, (mx, my)));
    // 海の画素は、透かしの外なら変わらない
    assert_eq!(rgb(&marked, sea_px()), rgb(&plain, sea_px()));
}

#[test]
fn replay_without_test_is_refused_but_unknown_or_real_sources_are_not() {
    assert!(check_replay(Some("replay"), false).is_err());
    assert!(check_replay(Some("replay"), true).is_ok());
    assert!(check_replay(Some("p2pquake"), false).is_ok());
    assert!(check_replay(None, false).is_ok()); // 取れない (古いサーバ) ときは replay ではないとみなす
    let e = check_replay(Some("replay"), false).unwrap_err();
    assert!(e.is::<crate::broadcast::Refused>());
}

/// 警報 1 件だけの入力 (八丈町の土砂災害警報)
fn hachijo(name: &str) -> Warnings {
    let mut w = Warnings::default();
    w.areas.insert("1340100".into(), vec![Kind { name: name.into() }]);
    w.names.insert("1340100".into(), "八丈町".into());
    w
}

#[test]
fn the_warning_banner_is_drawn_only_in_calm_and_only_for_warnings_and_above() {
    use super::paint::PANEL;
    let mut r = renderer(Text::none());
    let at = |pm: &Pixmap| rgb(pm, (640, 50));
    let warn = hachijo("レベル３土砂災害警報");
    assert_eq!(at(&r.render(&scene(None, &[], Some(&warn), None))), [0xb3, 0x26, 0x1e]);
    let danger = hachijo("レベル４土砂災害危険警報");
    assert_eq!(
        at(&r.render(&scene(None, &[], Some(&danger), None))),
        [0x7a, 0x1f, 0xa2]
    );
    // 注意報だけ・警報が無い・まだ取れていないときは、帯の矩形は落ち着いた地のまま
    let adv = hachijo("レベル２大雨注意報");
    assert_eq!(at(&r.render(&scene(None, &[], Some(&adv), None))), PANEL);
    assert_eq!(
        at(&r.render(&scene(None, &[], Some(&Warnings::default()), None))),
        PANEL
    );
    assert_eq!(at(&r.render(&scene(None, &[], None, None))), PANEL);
    // 地震の画面のときは、警報があっても矩形は空 (地のまま)
    let q = quake(Scale::S5_LOWER, &[("東京都", Scale::S5_LOWER)], None);
    assert_eq!(at(&r.render(&scene(Some(&q), &[], Some(&warn), None))), PANEL);
}

#[test]
fn the_warning_banner_has_its_own_rect_and_does_not_cover_the_map() {
    let mut r = renderer(Text::none());
    let warn = hachijo("レベル３土砂災害警報");
    let pm = r.render(&scene(None, &[], Some(&warn), None));
    let banner = [0xb3, 0x26, 0x1e];
    // 帯は 36 から 86 (上部バーの下) まで。地図は 86 から始まり、帯の色は入らない
    assert_eq!((rgb(&pm, (5, 36)), rgb(&pm, (5, 85))), (banner, banner));
    assert_ne!(rgb(&pm, (5, 35)), banner);
    assert_ne!(rgb(&pm, (5, 86)), banner); // 地図の上端 (陸の色) に帯の色は入らない
                                           // 右の列も帯の下から始まる (詳細の区切り線の上に帯の色が無い)
    assert_ne!(rgb(&pm, (1000, 90)), banner);
}

#[test]
fn the_idle_banner_message_is_drawn_when_a_font_is_available() {
    let font = BroadcastConfig::default().font;
    let mut r = renderer(Text::load(&font, 0).unwrap_or_else(|_| Text::none()));
    if !r.text.enabled() {
        return;
    }
    let unknown = r.render(&scene(None, &[], None, None));
    let none = r.render(&scene(None, &[], Some(&Warnings::default()), None));
    let differs = (36..86)
        .flat_map(|y| (16..300).map(move |x| (x, y)))
        .any(|p| rgb(&unknown, p) != rgb(&none, p));
    assert!(differs);
}

#[test]
fn the_test_band_is_at_the_top_of_the_map_below_the_warning_banner() {
    let mut r = renderer(Text::none());
    let warn = hachijo("レベル３土砂災害警報");
    let mut sc = scene(None, &[], Some(&warn), None);
    sc.test = true;
    let pm = r.render(&sc);
    assert_eq!(rgb(&pm, (640, 50)), [0xb3, 0x26, 0x1e]); // 警報の帯 (36〜86)
    assert_eq!(rgb(&pm, (5, 90)), super::test_mark::BAND); // 赤い帯は地図の上端 (86〜106)
}

fn hindsight_at(lat: f64, lon: f64) -> Hindsight {
    Hindsight {
        lat,
        lon,
        depth_km: 10.0,
        origin_ms: NOW as i64,
    }
}

#[test]
fn a_hindsight_epicenter_is_a_faint_cross_that_is_drawn_only_when_given() {
    let mut r = renderer(Text::none());
    let h = hindsight_at(36.0, 138.0);
    let plain = r.render(&scene(None, &[], None, None));
    let ghost = r.render(&Scene {
        hindsight: Some(&h),
        ..scene(None, &[], None, None)
    });
    let (x, y) = View::fit_home(map_rect()).px(138.0, 36.0);
    let at = (x as u32, y as u32);
    assert_ne!(rgb(&ghost, at), rgb(&plain, at));
    // 本物の ✕ (中心は赤) ほど濃くない
    let solid = {
        let e = eew(false, &[], Some((36.0, 138.0)));
        let mut sc = scene(None, &[], None, None);
        sc.eew = Some(&e);
        rgb(&r.render(&sc), at)
    };
    assert_eq!(solid, [0xe0, 0x1e, 0x1e]);
    assert_ne!(rgb(&ghost, at), solid);
    // 離れたところは変わらない
    assert_eq!(rgb(&ghost, sea_px()), rgb(&plain, sea_px()));
}

#[test]
fn the_clock_shows_fast_forward_only_when_asked() {
    let mut r = renderer(Text::none());
    let a = r.render(&scene(None, &[], None, None));
    let b = r.render(&Scene {
        fast_forward: true,
        ..scene(None, &[], None, None)
    });
    // 文字を描かない設定では、見た目は同じ (落ちないことと、既定の false が画面を変えないことの確認)
    assert_eq!(a.data(), b.data());
}

fn eew_event(origin_ms: u64, at: (f64, f64)) -> Event {
    use crate::quake::{Eew, EventBody};
    Event {
        id: "e1".into(),
        source: "wolfx".into(),
        received_at_ms: origin_ms + 5_000,
        body: EventBody::Eew(Eew {
            event_id: "E".into(),
            serial: "1".into(),
            cancelled: false,
            test: false,
            warning: false,
            issued_at: String::new(),
            origin_time: None,
            origin_time_ms: Some(origin_ms as i64),
            hypocenter: Some(Hypocenter {
                name: "x".into(),
                latitude: Some(at.0),
                longitude: Some(at.1),
                depth_km: Some(10),
                magnitude: Some(5.0),
            }),
            areas: vec![],
            pref_max: vec![],
            max_scale: Scale::S3,
        }),
    }
}

fn input<'a>(events: &'a [Event], now: u64, h: Option<&'a Hindsight>) -> Input<'a> {
    Input {
        events,
        now,
        rev: events.len() as u64,
        warnings: None,
        weather: None,
        icons: &NO_ICONS,
        bgm_title: "",
        connected: true,
        label: "記録から再現",
        test: false,
        check_ms: 200,
        flip_s: 0,
        hindsight: h,
        fast_forward: false,
        status: None,
        notices: None,
    }
}

#[test]
fn a_pending_hindsight_draws_waves_in_the_calm_screen_until_a_located_report_arrives() {
    let h = hindsight_at(36.0, 138.0);
    let t = NOW + 10_000;
    // 報がまだ無い: 平時の画面のまま、波は出る。ライブ (hindsight なし) は出ない
    let look_h = look(&[], t, Some(&h));
    assert!(look_h.calm && look_h.waving);
    assert!(!look(&[], t, None).waving);
    // 発生前・180 秒を過ぎたあとは出ない
    assert!(!look(&[], NOW - 1, Some(&h)).waving);
    assert!(!look(&[], NOW + 181_000, Some(&h)).waving);
    // 震源の付いた緊急地震速報が届いたら、のちに判明する震源は退き、波はその報から描く (二重にならない)
    let events = [eew_event(NOW, (36.0, 138.0))];
    let waves_ms = t + 1_000;
    assert!(super::eew::waves(&group_quakes(&events), &latest_eews(&events), waves_ms).len() == 1);
    let l = look(&events, waves_ms, Some(&h));
    assert!(!l.calm && l.waving);
    assert!(hindsight::pending(Some(&h), &group_quakes(&events), &latest_eews(&events)).is_none());
}

#[test]
fn the_stepper_skips_an_unchanged_frame_and_redraws_when_the_wave_moves() {
    let mut s = Stepper::new(renderer(Text::none()));
    let h = hindsight_at(36.0, 138.0);
    let t = NOW + 10_000;
    let first = s.step(&input(&[], t, Some(&h))).unwrap();
    assert!(first.calm);
    assert_eq!(first.i420.len(), 1280 * 720 * 3 / 2);
    // 同じ入力なら描き直さない
    assert!(s.step(&input(&[], t, Some(&h))).is_none());
    // 波が動けば (check_ms 進めば) 描き直す。波が無ければ同じ秒の間は描き直さない
    let moved = s.step(&input(&[], t + 200, Some(&h))).unwrap();
    assert_ne!(first.i420, moved.i420);
    let mut plain = Stepper::new(renderer(Text::none()));
    assert!(plain.step(&input(&[], t, None)).is_some());
    assert!(plain.step(&input(&[], t + 200, None)).is_none());
    assert!(plain.step(&input(&[], t + 1_000, None)).is_some());
}

/// 石狩市の区域の内側にある、札の色 (明るい) の画素の数
fn card_pixels_over_ishikari(pm: &Pixmap) -> usize {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let view = View::fit_home(map_rect());
    let areas = geo::load(&dir.join("warning-areas.geojson"), "code", &view).unwrap();
    let path = &areas.iter().find(|s| s.key == "0123500").unwrap().path;
    let frame = Frame::main(view);
    let (x, y, w, h) = frame.screen_bounds(path).unwrap();
    let mut n = 0;
    for py in y as u32..(y + h) as u32 + 1 {
        for px in x as u32..(x + w) as u32 + 1 {
            let [r, g, b] = rgb(pm, (px, py));
            if r > 215 && g > 220 && b > 225 && frame.path_contains(path, (px as f32 + 0.5, py as f32 + 0.5)) {
                n += 1;
            }
        }
    }
    n
}

fn sapporo_weather() -> CityWeather {
    CityWeather {
        cities: vec![City {
            name: "札幌".into(),
            lat: 43.06,
            lon: 141.35,
            code: "300".into(),
            temp: Some(15.0),
            tomorrow: None,
        }],
        rain: Vec::new(),
    }
}

#[test]
fn a_warning_on_ishikari_is_not_hidden_by_the_sapporo_card() {
    let mut r = renderer(Text::none());
    let weather = sapporo_weather();
    let kind = |n: &str| vec![Kind { name: n.into() }];
    let warned = |name: &str| Warnings {
        areas: [("0123500".to_string(), kind(name))].into_iter().collect(),
        ..Default::default()
    };
    let none = r.render(&scene(None, &[], None, Some(&weather)));
    let covered = card_pixels_over_ishikari(&none);
    assert!(covered > 75, "札幌の札は石狩の上にかかっているはず: {covered}");
    // 警報なら札は石狩を避ける (線の分だけは残る)
    let w = warned("レベル３大雨警報");
    let moved = r.render(&scene(None, &[], Some(&w), Some(&weather)));
    let left = card_pixels_over_ishikari(&moved);
    assert!(left < covered / 10, "札が石狩に残っている: {left} (避ける前 {covered})");
    // 注意報なら今までどおり (札は動かない)
    let adv = warned("レベル２大雨注意報");
    let stay = r.render(&scene(None, &[], Some(&adv), Some(&weather)));
    assert!(card_pixels_over_ishikari(&stay) > covered / 2);
}

fn maintenance_notices() -> Notices {
    Notices {
        interval_s: 20,
        texts: vec!["システムメンテナンスのお知らせ".into(), "二つ目のお知らせ".into()],
    }
}

/// お知らせの箱の中の画素 (右パネルの下半分。文字の無いテストでは 1 行ぶんの箱)
const NOTICE_PX: (u32, u32) = (1000, 500);

#[test]
fn the_notice_box_is_drawn_in_calm_only() {
    let mut r = renderer(Text::none());
    let n = maintenance_notices();
    let mut with = |quake| {
        r.render(&Scene {
            notices: Some(&n),
            ..scene(quake, &[], None, None)
        })
    };
    let calm = with(None);
    let q = quake(Scale::S4, &[("千葉県", Scale::S4)], Some((35.3, 140.3)));
    let shaking = with(Some(&q));
    assert_eq!(rgb(&calm, NOTICE_PX), super::paint::BG);
    // 地震の画面では出さない
    assert_eq!(rgb(&shaking, NOTICE_PX), super::paint::PANEL);
    let plain = r.render(&scene(None, &[], None, None));
    assert_eq!(rgb(&plain, NOTICE_PX), super::paint::PANEL);
}

#[test]
fn the_stepper_shows_the_notice_in_calm() {
    let mut s = Stepper::new(renderer(Text::none()));
    let n = maintenance_notices();
    let plain = s.step(&input(&[], NOW, None)).unwrap();
    let with = Input {
        notices: Some(&n),
        rev: 1, // 取れたときは rev が進む
        ..input(&[], NOW, None)
    };
    assert_ne!(s.step(&with).unwrap().i420, plain.i420);
}

/// 履歴は見出しの下に 52px の行が入る件数だけ描き、矩形の下へはみ出さない (平時の 312px では 5 件、地震の画面の 140px では 2 件)
#[test]
fn the_history_draws_as_many_rows_as_fit_in_its_rect() {
    use super::layout_resolve::Rect;
    let many: Vec<QuakeSummary> = (0..8).map(|_| quake(Scale::S7, &[], None)).collect();
    let s7 = scale_color(Scale::S7);
    for (h, rows) in [(312.0_f32, 5), (140.0, 2), (36.0, 0), (20.0, 0)] {
        let mut pm = Pixmap::new(draw::W, draw::H).unwrap();
        let area = Rect {
            x: 900.0,
            y: 100.0,
            w: 380.0,
            h,
        };
        panel::draw_history(&mut pm, &mut Text::none(), &many, area);
        let painted: Vec<u32> = (0..draw::H)
            .filter(|&y| (900..1280).any(|x| rgb(&pm, (x, y)) == s7))
            .collect();
        let count = (0..rows)
            .filter(|i| painted.contains(&(100 + 36 + i * 52 + 20)))
            .count();
        assert_eq!(count, rows as usize, "h={h}");
        // 次の行の札も、矩形の下も塗られない
        assert!(!painted.contains(&(100 + 36 + rows * 52 + 20)), "h={h}");
        assert!(painted.iter().all(|&y| (y as f32) < 100.0 + h.max(1.0)), "h={h}");
    }
}

/// サブの地図を描かない (sub_map = false) 設定の地震の画面は、今までどおり平時の矩形で履歴を 5 件描く
#[test]
fn the_quake_screen_without_the_sub_map_keeps_the_calm_history() {
    let q = quake(Scale::S7, &[("石川県", Scale::S7)], Some((37.5, 137.2)));
    let history: Vec<QuakeSummary> = (0..6).map(|_| quake(Scale::S7, &[], None)).collect();
    let pm = renderer(Text::none()).render(&scene(Some(&q), &history, None, None));
    // 履歴の 5 行目 (矩形 y180 + 見出し 36 + 4 行 + 札の中ほど)
    assert_eq!(rgb(&pm, (936, 180 + 36 + 4 * 52 + 20)), scale_color(Scale::S7));
}
