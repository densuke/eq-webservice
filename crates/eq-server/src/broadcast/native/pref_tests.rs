//! 県ごとの警報の一覧と上の帯の要約の確認 (1 コマ描いて画素を確かめる)。
//! EQ_NATIVE_PNG_DIR を指定すると、確認用の画面を dir/<name>.png に書き出す (フォントは EQ_NATIVE_FONT)。

use super::data::{Kind, Warnings};
use super::draw::Scene;
use super::notice::Notices;
use super::placed::Placed;
use super::tests::{quake, rgb, scene};
use super::*;
use crate::quake::Scale;

fn config(font: &str) -> BroadcastConfig {
    BroadcastConfig {
        sub_map: true,
        zoom: true,
        map_dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/public")
            .display()
            .to_string(),
        font: font.into(),
        ..BroadcastConfig::default()
    }
}

/// (県コード 2 桁, 市町村の数, 警報名...) から Warnings
fn warnings(items: &[(u32, usize, &[&str])]) -> Warnings {
    let mut w = Warnings::default();
    for (p, cities, kinds) in items {
        for c in 1..=*cities {
            let code = format!("{p:02}{c:02}100");
            w.areas.insert(
                code.clone(),
                kinds.iter().map(|k| Kind { name: k.to_string() }).collect(),
            );
            w.names.insert(code, format!("第{c}市"));
        }
    }
    w
}

fn notice() -> Notices {
    Notices {
        interval_s: 20,
        texts: vec!["検証中: 試験の画面です。JDQ 様・JQuake 様の画面を参考にしています".into()],
    }
}

const WARN: &str = "レベル３大雨警報";

fn many() -> Warnings {
    warnings(&[
        (1, 3, &[WARN]),
        (2, 1, &["レベル３洪水警報"]),
        (3, 12, &[WARN, "レベル３洪水警報"]),
        (4, 2, &["大雨特別警報", "レベル３暴風警報"]),
        (5, 5, &[WARN]),
        (6, 1, &["レベル３暴風警報"]),
        (7, 1, &["レベル４土砂災害危険警報"]),
        (
            13,
            4,
            &[
                "大雨特別警報",
                "レベル３洪水警報",
                "レベル３暴風警報",
                "レベル３波浪警報",
                "レベル３高潮警報",
            ],
        ),
        (14, 123, &[WARN]),
        (30, 2, &[WARN]),
        (46, 7, &["レベル３暴風警報", "レベル３波浪警報"]),
        (47, 1, &["レベル３高潮警報"]),
    ])
}

/// 平時の帯の中央の画素 (帯の色を見る)
fn band_px(placed: &Placed) -> [u32; 2] {
    let b = placed.banners.unwrap();
    [(b.x + 6.0) as u32, (b.y + b.h / 2.0) as u32]
}

#[test]
fn the_list_replaces_the_notice_only_while_a_warning_is_out_and_the_notice_returns_as_the_last_page() {
    let mut r = load_renderer(&config("/nonexistent")).unwrap();
    let placed = Placed::builtin().unwrap();
    let n = notice();
    let area = placed.notice.unwrap();
    // 箱の内側 (左から 20px・上から 10px) の画素: お知らせの箱は BG、一覧の箱も BG だが、特別警報の行は黒
    let mut at = |w: &Warnings, now_ms: u64| {
        let s = Scene {
            now_ms,
            notices: Some(&n),
            ..scene(None, &[], Some(w), None)
        };
        let pm = r.render(&s);
        rgb(&pm, ((area.x + 20.0) as u32, (area.y + 4.0 + 20.0 * 3.0 + 10.0) as u32))
    };
    // 特別警報の県が 4 番目の行 (ページ 0) にある: 黒い行。告知のページ (最後) は箱の地のまま
    let special = warnings(&[
        (1, 1, &[WARN]),
        (2, 1, &[WARN]),
        (3, 1, &[WARN]),
        (4, 1, &["大雨特別警報"]),
    ]);
    let (list_page, notice_page) = (at(&special, 0), at(&special, 8_000));
    assert_eq!(list_page, [0x0c, 0x00, 0x0c]);
    assert_ne!(notice_page, list_page);
    // 警報が無ければ告知のまま (ページ送りなし)
    let none = Warnings::default();
    assert_eq!(at(&none, 0), at(&none, 8_000));
    assert_ne!(at(&none, 0), list_page);
    // 帯は要約。特別警報なので黒
    let pm = r.render(&Scene {
        now_ms: 0,
        ..scene(None, &[], Some(&special), None)
    });
    assert_eq!(rgb(&pm, (band_px(&placed)[0], band_px(&placed)[1])), [0x0c, 0x00, 0x0c]);
}

/// 確認用の画面を PNG に書く (EQ_NATIVE_PNG_DIR があるときだけ)
#[test]
fn write_pref_list_pngs_when_asked() {
    let Ok(out) = std::env::var("EQ_NATIVE_PNG_DIR") else {
        return;
    };
    let font = std::env::var("EQ_NATIVE_FONT").unwrap_or_else(|_| "/nonexistent".into());
    let mut r = load_renderer(&config(&font)).unwrap();
    let n = notice();
    let save = |r: &mut Renderer, name: &str, s: &Scene| {
        r.render(s)
            .save_png(std::path::Path::new(&out).join(format!("{name}.png")))
            .unwrap();
    };
    let many = many();
    for k in 0..4u64 {
        let s = Scene {
            now_ms: k * 8_000,
            notices: Some(&n),
            ..scene(None, &[], Some(&many), None)
        };
        save(&mut r, &format!("many_p{k}"), &s);
    }
    let two = warnings(&[(3, 12, &[WARN, "レベル３洪水警報"]), (4, 3, &["レベル３暴風警報"])]);
    let adv = warnings(&[(3, 2, &["レベル２大雨注意報"]), (4, 1, &["レベル２強風注意報"])]);
    for (name, w, k) in [
        ("two_p0", Some(&two), 0u64),
        ("two_p1", Some(&two), 1),
        ("advisory", Some(&adv), 0),
        ("none", Some(&Warnings::default()), 0),
        ("unknown", None, 0),
    ] {
        let s = Scene {
            now_ms: k * 8_000,
            notices: Some(&n),
            ..scene(None, &[], w, None)
        };
        save(&mut r, name, &s);
    }
    let q = quake(Scale::S5_LOWER, &[("東京都", Scale::S5_LOWER)], None);
    let s = Scene {
        notices: Some(&n),
        ..scene(Some(&q), &[], Some(&many), None)
    };
    save(&mut r, "quake", &s);
}
