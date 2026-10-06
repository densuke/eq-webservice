//! 画面の並びの定義 (web/src/layout.json と同じ書式) の型・読み込み・検査。
//! 配信 (native) は起動時に GET /api/layout を 1 回読み、平時用と地震の画面用の 2 つの定義を決めて持つ。
//! 矩形への割り付けは layout_resolve.rs。書式の説明は docs/ui-spec/layout-system.html の「定義ファイル」。
//! 知らないキーは誤り (書き間違いに気づけるように)。ただし note は web と同じくどこでも書けて、読んで捨てる。

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;

use super::layout_resolve::resolve;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutFile {
    pub version: u32,
    pub layouts: Vec<LayoutDef>,
    /// サブの地図の保持時間 (web が使う。配信は読んで捨てる)
    #[serde(rename = "subMap", default)]
    _sub_map: Option<serde_json::Value>,
    #[serde(default, rename = "note")]
    _note: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutDef {
    pub name: String,
    pub when: Option<When>,
    pub manual: Option<bool>,
    pub scroll: Option<String>,
    pub root: Node,
    #[serde(default, rename = "note")]
    _note: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct When {
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Row,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub slot: Option<String>,
    pub variant: Option<String>,
    pub dir: Option<Dir>,
    pub r#box: Option<String>,
    pub size: Option<String>,
    pub children: Option<Vec<Node>>,
    pub overlays: Option<BTreeMap<Corner, Stack>>,
    #[serde(default, rename = "note")]
    _note: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Stack {
    pub flow: Dir,
    pub items: Vec<StackItem>,
    pub gap: Option<String>,
    pub pad: Option<String>,
    pub min_height: Option<String>,
    #[serde(default, rename = "note")]
    _note: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum StackItem {
    Name(String),
    Part(PartItem),
    Stack(Stack),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartItem {
    pub slot: String,
    pub variant: Option<String>,
    #[serde(default, rename = "note")]
    _note: Option<serde_json::Value>,
}

/// 大きさの指定。web の flexOf と同じ読み方 (配信では em・rem・calc() などは使えない)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    /// 残りを重みで分ける
    Fill(f32),
    /// 部品の固定の大きさ (文字では測らない)
    Auto,
    Px(f32),
    /// 親の大きさに対する割合 (0〜100)
    Pct(f32),
    /// 画面の幅・高さに対する割合 (0〜100)
    Vw(f32),
    Vh(f32),
}

pub fn parse_size(s: &str) -> anyhow::Result<Size> {
    let num = |t: &str| -> anyhow::Result<f32> {
        if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == '.') {
            bail!("大きさ「{s}」は使えない");
        }
        t.parse::<f32>().map_err(|_| anyhow!("大きさ「{s}」は使えない"))
    };
    if s == "fill" {
        return Ok(Size::Fill(1.0));
    }
    if let Some(w) = s.strip_prefix("fill:") {
        return Ok(Size::Fill(num(w)?));
    }
    if s == "auto" {
        return Ok(Size::Auto);
    }
    if s == "0" {
        return Ok(Size::Px(0.0));
    }
    let at = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (n, unit) = s.split_at(at);
    let n = num(n)?;
    match unit {
        "px" => Ok(Size::Px(n)),
        "%" => Ok(Size::Pct(n)),
        "vw" | "svw" | "dvw" | "lvw" => Ok(Size::Vw(n)),
        "vh" | "svh" | "dvh" | "lvh" => Ok(Size::Vh(n)),
        _ => bail!("大きさ「{s}」は使えない (配信では px・%・vw・vh・fill・auto だけ)"),
    }
}

/// 間隔・余白・最小の高さ (固定の長さだけ)
pub(super) fn parse_length(s: &str) -> anyhow::Result<Size> {
    match parse_size(s)? {
        Size::Fill(_) | Size::Auto => bail!("長さ「{s}」は使えない"),
        l => Ok(l),
    }
}

pub fn parse(json: &str) -> anyhow::Result<LayoutFile> {
    let file: LayoutFile = serde_json::from_str(json).context("レイアウトの定義を読めない")?;
    if file.version != 1 {
        bail!("レイアウトの定義の version は 1 (いまは {})", file.version);
    }
    Ok(file)
}

fn stack_slots(s: &Stack) -> Vec<&str> {
    s.items
        .iter()
        .flat_map(|i| match i {
            StackItem::Name(n) => vec![n.as_str()],
            StackItem::Part(p) => vec![p.slot.as_str()],
            StackItem::Stack(s) => stack_slots(s),
        })
        .collect()
}

/// 定義の中に置かれた部品の名前を、重ね物と入れ子の積みを含めて出てきた順に返す (重複もそのまま)
pub fn slots_of(node: &Node) -> Vec<&str> {
    node.slot
        .as_deref()
        .into_iter()
        .chain(node.overlays.iter().flat_map(|o| o.values().flat_map(stack_slots)))
        .chain(node.children.iter().flatten().flat_map(slots_of))
        .collect()
}

fn check_stack(s: &Stack) -> anyhow::Result<()> {
    for l in [&s.gap, &s.min_height].into_iter().flatten() {
        parse_length(l)?;
    }
    for l in s.pad.iter().flat_map(|p| p.split(' ')) {
        parse_length(l)?;
    }
    s.items.iter().try_for_each(|i| match i {
        StackItem::Stack(s) => check_stack(s),
        _ => Ok(()),
    })
}

fn check_lengths(node: &Node) -> anyhow::Result<()> {
    if let Some(s) = &node.size {
        parse_size(s)?;
    }
    node.overlays
        .iter()
        .flat_map(|o| o.values())
        .try_for_each(check_stack)?;
    node.children.iter().flatten().try_for_each(check_lengths)
}

/// 配信で使う定義を名前で取り出して検査する (main を 1 回置く・同じ部品を 2 回置かない・大きさが配信で扱える)
pub fn pick<'a>(file: &'a LayoutFile, name: &str) -> anyhow::Result<&'a LayoutDef> {
    let def = file
        .layouts
        .iter()
        .find(|l| l.name == name)
        .ok_or_else(|| anyhow!("定義「{name}」が無い"))?;
    let slots = slots_of(&def.root);
    if let Some(dup) = slots.iter().enumerate().find(|(i, s)| slots[..*i].contains(s)) {
        bail!("定義「{name}」: 部品「{}」を 2 回置いている", dup.1);
    }
    if !slots.contains(&"main") {
        bail!("定義「{name}」: 部品「main」を置いていない");
    }
    check_lengths(&def.root).with_context(|| format!("定義「{name}」"))?;
    Ok(def)
}

fn pick_both(json: &str, name: &str, quake: &str) -> anyhow::Result<(LayoutDef, LayoutDef)> {
    let file = parse(json)?;
    let defs = (pick(&file, name)?.clone(), pick(&file, quake)?.clone());
    // 平時と地震の画面で main と topbar の矩形が違うと、地図の絵や投影を作り直すことになる
    let (w, h) = (super::draw::W as f32, super::draw::H as f32);
    let (a, b) = (resolve(&defs.0, w, h)?, resolve(&defs.1, w, h)?);
    for part in ["main", "topbar"] {
        if a.get(part) != b.get(part) {
            bail!(
                "定義「{name}」と「{quake}」で部品「{part}」の矩形が違う ({:?} と {:?})",
                a.get(part),
                b.get(part)
            );
        }
    }
    Ok(defs)
}

/// 起動時: 取れた JSON (無ければ None) から平時用と地震の画面用の 2 つの定義を決める。
/// 取れない・壊れている・名前が無い・使えない大きさがあるときは、警告を出して組み込みの定義。組み込みもだめならエラー
pub fn load(json: Option<&str>, name: &str, quake: &str) -> anyhow::Result<(LayoutDef, LayoutDef)> {
    load_with(json, crate::layout::BUILTIN, name, quake)
}

pub(super) fn load_with(
    json: Option<&str>,
    builtin: &str,
    name: &str,
    quake: &str,
) -> anyhow::Result<(LayoutDef, LayoutDef)> {
    if let Some(j) = json {
        match pick_both(j, name, quake) {
            Ok(defs) => return Ok(defs),
            Err(e) => {
                tracing::warn!("broadcast: サーバのレイアウトの定義を使えないので、組み込みの定義にします: {e:#}")
            }
        }
    }
    pick_both(builtin, name, quake).context("組み込みのレイアウトの定義が使えない")
}
