//! 描画の確認 (実際の地図データで 1 コマ描き、決まった位置の画素を確かめる)。文字は描かない設定で行う (CI にフォントが無くてもよい)。
//! EQ_NATIVE_PNG_DIR を指定すると、確認用の画面を PNG に書き出す (フォントは EQ_NATIVE_FONT)。

use tiny_skia::Pixmap;

use super::data::{City, CityWeather, Kind, Warnings};
use super::draw::{Renderer, Scene, MAP_RECT};
use super::geo::{self, View};
use super::model::QuakeSummary;
use super::paint::SEA;
use super::text::Text;
use super::*;
use crate::quake::{Hypocenter, Scale};

const NOW: u64 = 1_790_000_000_000;

fn renderer(text: Text) -> Renderer {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let view = View::fit_home(MAP_RECT);
    let prefs = geo::load(&dir.join("japan.geojson"), "name", &view).unwrap();
    let areas = geo::load(&dir.join("warning-areas.geojson"), "code", &view).unwrap();
    Renderer::new(view, prefs, areas, text)
}

fn center_of(pref: &str) -> (u32, u32) {
    let view = View::fit_home(MAP_RECT);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let shapes = geo::load(&dir.join("japan.geojson"), "name", &view).unwrap();
    let c = shapes.iter().find(|s| s.key == pref).unwrap().center;
    (c.0 as u32, c.1 as u32)
}

fn rgb(pm: &Pixmap, (x, y): (u32, u32)) -> [u8; 3] {
    let p = pm.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue()]
}

fn quake(max: Scale, prefs: &[(&str, Scale)], at: Option<(f64, f64)>) -> QuakeSummary {
    QuakeSummary {
        updated_ms: NOW,
        origin_time: "2026/09/30 12:00:00".into(),
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

fn scene<'a>(
    quake: Option<&'a QuakeSummary>,
    history: &'a [QuakeSummary],
    warnings: Option<&'a Warnings>,
    weather: Option<&'a CityWeather>,
) -> Scene<'a> {
    Scene {
        quake,
        history,
        warnings,
        weather,
        now_ms: NOW,
        connected: true,
        bgm_title: "テスト曲",
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
    };
    let pm = r.render(&scene(None, &[], Some(&warnings), None));
    assert_eq!((pm.width(), pm.height()), (1280, 720));
    assert_eq!(rgb(&pm, (100, 100)), SEA); // 日本海
                                           // 警報の赤 (陸に半透明で重なる) が地図に出る。県の全体を塗るのではなく、市町村等の区域だけを塗る
                                           // (左下の凡例にも赤があるので、数えるのは凡例より右)
    let red = |pm: &Pixmap| {
        let w = pm.width() as usize;
        let hit = |(i, p): (usize, &tiny_skia::PremultipliedColorU8)| {
            i % w >= 100 && p.red() > 150 && p.green() < 80 && p.blue() < 80
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
    assert_eq!(rgb(&pm, (100, 100)), SEA);
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
}
