//! 描画の確認 (実際の地図データで 1 コマ描き、決まった位置の画素を確かめる)。文字は描かない設定で行う (CI にフォントが無くてもよい)。
//! EQ_NATIVE_PNG_DIR を指定すると、確認用の画面を PNG に書き出す (フォントは EQ_NATIVE_FONT)。

use tiny_skia::Pixmap;

use super::data::{City, CityWeather, Kind, Warnings};
use super::draw::{Renderer, Scene, MAP_RECT};
use super::eew::{EewSummary, Wave};
use super::geo::{self, View};
use super::model::{scale_color, QuakeSummary};
use super::paint::SEA;
use super::text::Text;
use super::*;
use crate::quake::{Hypocenter, Scale};

const NOW: u64 = 1_790_000_000_000;
static NO_ICONS: std::sync::LazyLock<Icons> = std::sync::LazyLock::new(Icons::new);

fn renderer(text: Text) -> Renderer {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let view = View::fit_home(MAP_RECT);
    let prefs = geo::load(&dir.join("japan.geojson"), "name", &view).unwrap();
    let areas = geo::load(&dir.join("warning-areas.geojson"), "code", &view).unwrap();
    let neighbors = geo::load(&dir.join("neighbors.geojson"), "name", &view).unwrap();
    Renderer::new(view, neighbors, prefs, areas, text)
}

fn center_of(pref: &str) -> (u32, u32) {
    let view = View::fit_home(MAP_RECT);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let shapes = geo::load(&dir.join("japan.geojson"), "name", &view).unwrap();
    let c = shapes.iter().find(|s| s.key == pref).unwrap().center;
    (c.0 as u32, c.1 as u32)
}

/// 日本海の画素 (陸から離れた海)
fn sea_px() -> (u32, u32) {
    let (x, y) = View::fit_home(MAP_RECT).px(134.0, 40.5);
    (x as u32, y as u32)
}

fn rgb(pm: &Pixmap, (x, y): (u32, u32)) -> [u8; 3] {
    let p = pm.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue()]
}

fn quake(max: Scale, prefs: &[(&str, Scale)], at: Option<(f64, f64)>) -> QuakeSummary {
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
        has_areas: false,
    }
}

fn scene<'a>(
    quake: Option<&'a QuakeSummary>,
    history: &'a [QuakeSummary],
    warnings: Option<&'a Warnings>,
    weather: Option<&'a CityWeather>,
) -> Scene<'a> {
    Scene {
        icons: &NO_ICONS,
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
    }
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
    let view = View::fit_home(MAP_RECT);
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
    let (x, y) = View::fit_home(MAP_RECT).px(137.2, 37.5);
    assert!(near(&pm, (x as u32, y as u32), [0xe0, 0x1e, 0x1e]));
    // 右パネルの見出しの札: 警報は赤、予報は橙 (文字の左の余白)
    assert_eq!(rgb(&pm, (SIDE + 3, 62)), [0xd7, 0x26, 0x3d]);
    let forecast = eew(false, &[("石川県", Scale::S4)], Some((37.5, 137.2)));
    sc.eew = Some(&forecast);
    assert_eq!(rgb(&r.render(&sc), (SIDE + 3, 62)), [0xb3, 0x59, 0x00]);
    // 地震情報があれば、そちらを出す (観測の色、札は無い)
    let q = quake(Scale::S4, &[("石川県", Scale::S4)], Some((37.5, 137.2)));
    sc.quake = Some(&q);
    let pm = r.render(&sc);
    assert_eq!(rgb(&pm, center_of("石川県")), [0xfa, 0xf5, 0x00]);
    assert_ne!(rgb(&pm, (SIDE + 3, 62)), [0xb3, 0x59, 0x00]);
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
    let view = View::fit_home(MAP_RECT);
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
    let (x, y) = View::fit_home(MAP_RECT).px(126.98, 37.57); // ソウル
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
    let main = View::fit_home(MAP_RECT);
    let inset = frame::Frame::inset(&main, &frame::OKINAWA).unwrap();
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
    assert!(near(&pm, (18, 258), [0xe0, 0x1e, 0x1e])); // 枠 (10,46 から高さ 220) の左下の隅
                                                       // 遠い震央 (台湾の西) は、どこにも印を置かない
    let far = eew(false, &[], Some((23.6, 118.0)));
    sc.eew = Some(&far);
    assert!(!near(&r.render(&sc), (18, 258), [0xe0, 0x1e, 0x1e]));
}

#[test]
fn the_okinawa_inset_is_drawn_with_land_and_the_scale_color() {
    let mut r = renderer(Text::none());
    let calm = r.render(&scene(None, &[], None, None));
    assert!(near(&calm, okinawa_px(), [0x3a, 0x42, 0x50])); // 別枠の中の沖縄本島は陸 (細い島なので、まわりも見る)
    assert_eq!(rgb(&calm, (50, 60)), SEA); // 枠の中の海 (枠は 10,46 から)
    let line = rgb(&calm, (9, 100)); // 枠線 (角の丸めで少しにじむ)
    assert!(line[2] > 60 && line[2] < 0x53, "{line:?}");
    let q = quake(Scale::S3, &[("沖縄県", Scale::S3)], None);
    let shaken = r.render(&scene(Some(&q), &[], None, None));
    assert!(near(&shaken, okinawa_px(), [0x00, 0x41, 0xff])); // 震度 3
}

#[test]
fn warnings_in_okinawa_are_drawn_in_the_inset_too() {
    let mut r = renderer(Text::none());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let view = View::fit_home(MAP_RECT);
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
    let test_png = r.render(&test_scene(None, &history[1..]));
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
    // 上部バーのすぐ下と最下部の帯 (文字の無い端の画素)
    assert_eq!(rgb(&marked, (5, 40)), red);
    assert_eq!(rgb(&marked, (5, 715)), red);
    assert_ne!(rgb(&plain, (5, 40)), red);
    // 地図の中央 (E の縦棒の上) に、うすい白が乗る
    let (mx, my) = (450 - 256 + 134 + 5, 378 - 95 + 90);
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
    let mut r = renderer(Text::none());
    let at = |pm: &Pixmap| rgb(pm, (640, 50));
    let warn = hachijo("レベル３土砂災害警報");
    assert_eq!(at(&r.render(&scene(None, &[], Some(&warn), None))), [0xb3, 0x26, 0x1e]);
    let danger = hachijo("レベル４土砂災害危険警報");
    assert_eq!(
        at(&r.render(&scene(None, &[], Some(&danger), None))),
        [0x7a, 0x1f, 0xa2]
    );
    // 注意報だけ・警報が無い・地震の画面のときは出ない
    let adv = hachijo("レベル２大雨注意報");
    assert_eq!(at(&r.render(&scene(None, &[], Some(&adv), None))), SEA);
    assert_eq!(at(&r.render(&scene(None, &[], None, None))), SEA);
    let q = quake(Scale::S5_LOWER, &[("東京都", Scale::S5_LOWER)], None);
    assert_ne!(
        at(&r.render(&scene(Some(&q), &[], Some(&warn), None))),
        [0xb3, 0x26, 0x1e]
    );
}

#[test]
fn the_warning_banner_sits_below_the_test_band() {
    let mut r = renderer(Text::none());
    let warn = hachijo("レベル３土砂災害警報");
    let mut sc = scene(None, &[], Some(&warn), None);
    sc.test = true;
    let pm = r.render(&sc);
    assert_eq!(rgb(&pm, (640, 38)), super::test_mark::BAND);
    assert_eq!(rgb(&pm, (640, 36 + 20 + 14)), [0xb3, 0x26, 0x1e]);
}
