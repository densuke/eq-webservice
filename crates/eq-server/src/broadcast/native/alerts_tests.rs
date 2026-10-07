//! 重大な警報の枠と、津波予報区の海岸線の確認 (実際の地図データで 1 コマ描き、画素を確かめる)。文字は描かない設定。
//! EQ_NATIVE_PNG_DIR を指定すると、確認用の画面を PNG に書き出す (フォントは EQ_NATIVE_FONT)。

use super::banner::banner_color;
use super::camera::{fit_box, MapBox};
use super::data::{Kind, WarningLevel, Warnings};
use super::draw::Scene;
use super::geo::{project, View};
use super::model::QuakeSummary;
use super::placed::Placed;
use super::tests::{quake, rgb, scene};
use super::*;
use crate::quake::model::{TsunamiArea, TsunamiGrade};
use crate::quake::Scale;

fn config(font: &str, sub_map: bool) -> BroadcastConfig {
    BroadcastConfig {
        sub_map,
        zoom: true,
        map_dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/public")
            .display()
            .to_string(),
        font: font.into(),
        ..BroadcastConfig::default()
    }
}

fn area(name: &str, grade: TsunamiGrade) -> TsunamiArea {
    TsunamiArea {
        name: name.into(),
        grade,
        immediate: false,
        first_height: None,
        max_height: None,
    }
}

/// 県 prefs 個 (コード 01 から) に、それぞれ市町村 cities 個の警報を出す
fn warnings(kind: &str, prefs: usize, cities: usize) -> Warnings {
    let mut w = Warnings::default();
    for p in 1..=prefs {
        for c in 1..=cities {
            let code = format!("{p:02}{c:02}100");
            w.areas.insert(code.clone(), vec![Kind { name: kind.into() }]);
            w.names.insert(code, format!("第{c}市"));
        }
    }
    w
}

fn noto() -> QuakeSummary {
    quake(
        Scale::S5_UPPER,
        &[("石川県", Scale::S5_UPPER), ("富山県", Scale::S4)],
        Some((37.5, 137.2)),
    )
}

/// 名前の予報区の海岸線の最初の点 (経度, 緯度)
fn first_point(name: &str) -> (f64, f64) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("tsunami.geojson")).unwrap()).unwrap();
    let f = v["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["properties"]["name"] == name)
        .unwrap();
    let p = &f["geometry"]["coordinates"][0][0];
    (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())
}

const PURPLE: [u8; 3] = [0xc8, 0x00, 0xff];

/// 履歴の矩形の画素 (矩形の内側の左から 4px・上から dy px)
fn history_px(placed: &Placed, dy: f32) -> (u32, u32) {
    let h = placed.history.unwrap();
    ((h.x + 4.0) as u32, (h.y + dy) as u32)
}

#[test]
fn the_history_rect_becomes_the_alert_panel_only_while_something_major_is_out() {
    let mut r = load_renderer(&config("/nonexistent", false)).unwrap();
    let placed = Placed::builtin().unwrap();
    let major = [area("宮城県", TsunamiGrade::MajorWarning)];
    let special = warnings("大雨特別警報", 1, 1);
    let minor = warnings("大雨警報", 1, 1);
    let calm = scene(None, &[], None, None);
    let mut at = |s: &Scene| rgb(&r.render(s), history_px(&placed, 30.0));
    // 大津波警報: 帯の下の最初の行 (見出し) が大津波警報の色
    let with_major = Scene {
        tsunami: &major,
        ..scene(None, &[], None, None)
    };
    assert_eq!(at(&with_major), banner_color(WarningLevel::Danger));
    // 特別警報だけ
    let with_special = scene(None, &[], Some(&special), None);
    assert_eq!(at(&with_special), banner_color(WarningLevel::Emergency));
    // 通常の警報・津波注意報だけなら、履歴のまま (矩形の中は何も変わらない)
    let watch = [area("宮城県", TsunamiGrade::Watch)];
    let quiet = Scene {
        tsunami: &watch,
        ..scene(None, &[], Some(&minor), None)
    };
    let (a, b) = (r.render(&quiet), r.render(&calm));
    let h = placed.history.unwrap();
    for y in h.y as u32..h.bottom() as u32 {
        for x in h.x as u32..h.right() as u32 {
            assert_eq!(rgb(&a, (x, y)), rgb(&b, (x, y)), "{x},{y}");
        }
    }
}

#[test]
fn the_coast_line_is_painted_in_the_grade_color_on_the_main_map_and_not_without_a_forecast() {
    let mut r = load_renderer(&config("/nonexistent", false)).unwrap();
    let placed = Placed::builtin().unwrap();
    let view = View::fit_home(placed.main.tuple64());
    let (lon, lat) = first_point("宮城県");
    let (x, y) = view.px(lon, lat);
    let at = (x as u32, y as u32);
    let major = [area("宮城県", TsunamiGrade::MajorWarning)];
    let on = Scene {
        tsunami: &major,
        ..scene(None, &[], None, None)
    };
    assert_eq!(rgb(&r.render(&on), at), PURPLE);
    assert_ne!(rgb(&r.render(&scene(None, &[], None, None)), at), PURPLE);
    // 予報区が違えば塗らない
    let other = [area("岩手県", TsunamiGrade::MajorWarning)];
    let off = Scene {
        tsunami: &other,
        ..scene(None, &[], None, None)
    };
    assert_ne!(rgb(&r.render(&off), at), PURPLE);
}

#[test]
fn the_coast_line_stays_on_the_coast_while_the_map_zooms() {
    let mut r = load_renderer(&config("/nonexistent", false)).unwrap();
    let placed = Placed::builtin().unwrap();
    let (lon, lat) = first_point("宮城県");
    let (px, py) = project(lon, lat);
    r.set_view(Some(fit_box(MapBox::around(px, py, 60.0), r.map_aspect())));
    let major = [area("宮城県", TsunamiGrade::MajorWarning)];
    let s = Scene {
        tsunami: &major,
        ..scene(None, &[], None, None)
    };
    // 寄った地図の中心が、その海岸線の点
    let c = placed.main;
    let center = ((c.x + c.w / 2.0) as u32, (c.y + c.h / 2.0) as u32);
    assert_eq!(rgb(&r.render(&s), center), PURPLE);
}

#[test]
fn the_sub_map_paints_the_coast_too() {
    let mut r = load_renderer(&config("/nonexistent", true)).unwrap();
    let (lon, lat) = first_point("宮城県");
    let (px, py) = project(lon, lat);
    r.set_sub_view(Some(fit_box(MapBox::around(px, py, 60.0), r.sub_aspect().unwrap())));
    let q = noto();
    let major = [area("宮城県", TsunamiGrade::MajorWarning)];
    let s = Scene {
        tsunami: &major,
        ..scene(Some(&q), std::slice::from_ref(&q), None, None)
    };
    let sub = Placed::builtin().unwrap().sub.unwrap();
    let center = ((sub.x + sub.w / 2.0) as u32, (sub.y + sub.h / 2.0) as u32);
    assert_eq!(rgb(&r.render(&s), center), PURPLE);
}

/// 確認用の画面を PNG に書く (EQ_NATIVE_PNG_DIR があるときだけ。dir/alerts_<name>.png)
#[test]
fn write_alert_panel_pngs_when_asked() {
    let Ok(out) = std::env::var("EQ_NATIVE_PNG_DIR") else {
        return;
    };
    let font = std::env::var("EQ_NATIVE_FONT").unwrap_or_else(|_| "/nonexistent".into());
    let save = |name: &str, pm: tiny_skia::Pixmap| {
        std::fs::create_dir_all(&out).unwrap();
        pm.save_png(std::path::Path::new(&out).join(format!("alerts_{name}.png")))
            .unwrap();
    };
    let mut calm_r = load_renderer(&config(&font, false)).unwrap();
    let major = [
        area("岩手県", TsunamiGrade::MajorWarning),
        area("宮城県", TsunamiGrade::MajorWarning),
        area("青森県太平洋沿岸", TsunamiGrade::Warning),
        area("福島県", TsunamiGrade::Warning),
        area("茨城県", TsunamiGrade::Watch),
        area("千葉県九十九里・外房", TsunamiGrade::Watch),
        area("北海道太平洋沿岸東部", TsunamiGrade::Watch),
    ];
    let special = warnings("暴風特別警報", 2, 2);
    let many = warnings("大雨特別警報", 12, 4);
    let minor = warnings("大雨警報", 3, 2);
    let now = scene(None, &[], None, None).now_ms / 48000 * 48000;
    let at = |dt: u64| now + dt;
    let calm = |tsu: &'static [TsunamiArea], w: Option<&'static Warnings>, dt: u64| Scene {
        tsunami: tsu,
        now_ms: at(dt),
        ..scene(None, &[], w, None)
    };
    let major: &'static [TsunamiArea] = Box::leak(major.to_vec().into_boxed_slice());
    let special: &'static Warnings = Box::leak(Box::new(special));
    let many: &'static Warnings = Box::leak(Box::new(many));
    let minor: &'static Warnings = Box::leak(Box::new(minor));
    for p in 0..4u64 {
        save(
            &format!("calm_major_p{}", p + 1),
            calm_r.render(&calm(major, Some(many), p * 8000)),
        );
    }
    save("calm_special", calm_r.render(&calm(&[], Some(special), 0)));
    save("calm_none", calm_r.render(&calm(&[], Some(minor), 0)));
    // 地震の画面 (サブの地図あり)
    let mut quake_r = load_renderer(&config(&font, true)).unwrap();
    let q = noto();
    let (lon, lat) = (141.0, 38.5);
    let (px, py) = project(lon, lat);
    quake_r.set_sub_view(Some(fit_box(
        MapBox::around(px, py, 330.0),
        quake_r.sub_aspect().unwrap(),
    )));
    let hist = [q.clone()];
    let qs = |tsu: &'static [TsunamiArea], w: Option<&'static Warnings>, dt: u64| Scene {
        tsunami: tsu,
        now_ms: at(dt),
        ..scene(Some(&q), &hist, w, None)
    };
    let warn = &major[..4];
    for p in 0..2u64 {
        save(
            &format!("quake_tsunami_p{}", p + 1),
            quake_r.render(&qs(warn, Some(minor), p * 8000)),
        );
    }
    save("quake_none", quake_r.render(&qs(&[], Some(minor), 0)));
}
