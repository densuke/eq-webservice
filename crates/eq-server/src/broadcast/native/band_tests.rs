//! 気象警報の帯の専用の場所 (定義の banners) の確認。ページ送りの規則と、確認用の PNG の書き出し。

use super::data::{Kind, Warnings};
use super::draw::Scene;
use super::model::QuakeSummary;
use super::tests::{quake, renderer, scene};
use super::text::Text;
use crate::quake::Scale;

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

/// 確認用の画面を PNG に書く (EQ_NATIVE_PNG_DIR があるときだけ。フォントは EQ_NATIVE_FONT)
#[test]
fn write_band_pngs_when_asked() {
    let Ok(out) = std::env::var("EQ_NATIVE_PNG_DIR") else {
        return;
    };
    let text = std::env::var("EQ_NATIVE_FONT")
        .ok()
        .and_then(|p| Text::load(&p, 0).ok())
        .unwrap_or_else(Text::none);
    let mut r = renderer(text);
    let save = |name: &str, pm: tiny_skia::Pixmap| {
        pm.save_png(std::path::Path::new(&out).join(name)).unwrap();
    };
    let one = warnings("レベル３大雨警報", 1, 2);
    let two = warnings("レベル３大雨警報", 6, 5);
    let many = warnings("レベル３大雨警報", 47, 3);
    let now = scene(None, &[], None, None).now_ms;
    let calm = Warnings::default();
    save("band_none.png", r.render(&scene(None, &[], Some(&calm), None)));
    save("band_1line.png", r.render(&scene(None, &[], Some(&one), None)));
    save("band_2lines.png", r.render(&scene(None, &[], Some(&two), None)));
    for (i, sec) in [0u64, 8, 16].into_iter().enumerate() {
        let sc = Scene {
            now_ms: now + sec * 1000,
            ..scene(None, &[], Some(&many), None)
        };
        save(&format!("band_many_{i}.png"), r.render(&sc));
    }
    let q = noto();
    save(
        "band_quake.png",
        r.render(&scene(Some(&q), std::slice::from_ref(&q), Some(&many), None)),
    );
    let test = Scene {
        test: true,
        ..scene(None, &[], Some(&two), None)
    };
    save("band_test.png", r.render(&test));
}
