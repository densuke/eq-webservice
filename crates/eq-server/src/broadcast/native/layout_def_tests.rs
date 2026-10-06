//! layout_def (定義の型・読み込み・検査) のテスト。出荷の layout.json (crate::layout::BUILTIN) を実物として使う。

use super::layout_def::load_with;
use super::layout_def::*;
use crate::layout::BUILTIN;

const NAMES: [&str; 6] = [
    "landscape",
    "regular",
    "compact",
    "trial",
    "broadcast",
    "broadcast-quake",
];

/// main を 1 つ置いただけの最小の定義 (name・size だけ差し替える)
fn json_with(root: &str) -> String {
    format!(r#"{{"version":1,"layouts":[{{"name":"a","root":{root}}},{{"name":"b","root":{root}}}]}}"#)
}

#[test]
fn every_shipped_layout_is_readable() {
    let file = parse(BUILTIN).unwrap();
    let names: Vec<_> = file.layouts.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, NAMES);
}

#[test]
fn the_broadcast_layouts_are_manual_and_pass_the_checks() {
    let file = parse(BUILTIN).unwrap();
    for name in ["broadcast", "broadcast-quake"] {
        let def = pick(&file, name).unwrap();
        assert_eq!(def.manual, Some(true), "{name}");
        assert!(def
            .when
            .as_ref()
            .is_some_and(|w| w.min_width == Some(801.0) && w.min_height == Some(481.0)));
    }
    // 平時には notice があり、地震の画面にはサブの地図があって notice は無い
    let calm = slots_of(&pick(&file, "broadcast").unwrap().root);
    let quake = slots_of(&pick(&file, "broadcast-quake").unwrap().root);
    assert!(calm.contains(&"notice") && !calm.contains(&"map-sub"));
    assert!(quake.contains(&"map-sub") && !quake.contains(&"notice"));
}

#[test]
fn a_missing_name_is_an_error() {
    assert!(pick(&parse(BUILTIN).unwrap(), "nothing").is_err());
}

#[test]
fn a_layout_without_main_is_an_error() {
    let file = parse(&json_with(
        r#"{"dir":"column","children":[{"slot":"topbar","size":"36px"}]}"#,
    ))
    .unwrap();
    assert!(pick(&file, "a").is_err());
}

#[test]
fn a_part_placed_twice_is_an_error_even_inside_an_overlay() {
    let root = r#"{"dir":"column","children":[{"slot":"main","size":"fill","overlays":{"top-left":{"flow":"column","items":["clock",{"flow":"row","items":["clock"]}]}}}]}"#;
    let err = pick(&parse(&json_with(root)).unwrap(), "a").unwrap_err().to_string();
    assert!(err.contains("clock"), "{err}");
}

#[test]
fn sizes_the_broadcast_cannot_compute_are_errors() {
    for bad in [
        "calc(100% - 10px)",
        "clamp(1px,2px,3px)",
        "2em",
        "1rem",
        "min(1px,2px)",
        "-3px",
        "ten",
    ] {
        let root = format!(r#"{{"dir":"column","children":[{{"slot":"main","size":"{bad}"}}]}}"#);
        assert!(pick(&parse(&json_with(&root)).unwrap(), "a").is_err(), "{bad}");
    }
    for good in ["fill", "fill:2", "auto", "380px", "70%", "60svh", "10vw", "0"] {
        let root = format!(r#"{{"dir":"column","children":[{{"slot":"main","size":"{good}"}}]}}"#);
        assert!(pick(&parse(&json_with(&root)).unwrap(), "a").is_ok(), "{good}");
    }
}

#[test]
fn notes_are_ignored_everywhere_but_unknown_keys_are_errors() {
    let with_notes = r#"{"version":1,"note":"x","layouts":[{"name":"a","note":"y","root":{"note":"z","dir":"column","children":[
        {"slot":"main","note":"w","overlays":{"top-left":{"flow":"column","note":"v","items":[{"slot":"clock","variant":"v","note":"u"}]}}}]}}]}"#;
    assert!(parse(with_notes).is_ok());
    assert!(parse(&with_notes.replace(r#""note":"w""#, r#""colour":"red""#)).is_err());
    assert!(parse(&with_notes.replace(r#""note":"u""#, r#""colour":"red""#)).is_err());
}

#[test]
fn a_broken_or_missing_definition_falls_back_to_the_built_in() {
    let builtin = parse(BUILTIN).unwrap();
    let want = (
        pick(&builtin, "broadcast").unwrap().clone(),
        pick(&builtin, "broadcast-quake").unwrap().clone(),
    );
    assert_eq!(load(None, "broadcast", "broadcast-quake").unwrap(), want);
    assert_eq!(load(Some("{ broken"), "broadcast", "broadcast-quake").unwrap(), want);
    // 読めても、配信用の名前が無い・使えない大きさがあるときも組み込みへ
    assert_eq!(
        load(Some(&json_with(r#"{"slot":"main"}"#)), "broadcast", "broadcast-quake").unwrap(),
        want
    );
    let calc = r#"{"dir":"column","children":[{"slot":"main","size":"calc(1px + 2px)"}]}"#;
    let named = json_with(calc)
        .replace(r#""a""#, r#""broadcast""#)
        .replace(r#""b""#, r#""broadcast-quake""#);
    assert_eq!(load(Some(&named), "broadcast", "broadcast-quake").unwrap(), want);
}

#[test]
fn a_good_server_definition_is_used_as_is() {
    let json = json_with(r#"{"slot":"main"}"#);
    let (a, b) = load(Some(&json), "a", "b").unwrap();
    assert_eq!((a.name.as_str(), b.name.as_str()), ("a", "b"));
}

#[test]
fn a_broken_built_in_fails_the_start() {
    assert!(load_with(None, "{ broken", "broadcast", "broadcast-quake").is_err());
    assert!(load_with(Some("{ broken"), "{ broken", "broadcast", "broadcast-quake").is_err());
    // 組み込みに配信用の名前が無くても同じ
    assert!(load_with(None, &json_with(r#"{"slot":"main"}"#), "broadcast", "broadcast-quake").is_err());
}

#[test]
fn only_version_1_is_accepted() {
    assert!(parse(&json_with(r#"{"slot":"main"}"#).replace(r#""version":1"#, r#""version":2"#)).is_err());
}
