//! レイアウトの定義 (layout_def.rs) を、幅 w・高さ h の画面の矩形に割り付ける純粋な関数。
//! 規則は web の flex と同じ: 容器の向きに沿って固定の大きさ (px・%・vh・vw・auto) を先に引き、残りを fill の重みで分ける。
//! 交差方向は容器いっぱい。重ね物 (overlays) は隅に寄せ、積みの向きに gap で並べる。
//! auto は文字で測らず、部品ごとの固定の大きさを使う (フォントの無い CI でも結果が変わらないように)。
//! 配信で描けない部品 (native に無い部品) は場所も取らず、結果にも入れない。

use std::collections::BTreeMap;

use anyhow::Result;

use super::layout_def::{parse_length, parse_size, slots_of, Corner, Dir, LayoutDef, Node, Size, Stack, StackItem};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 配信が描ける部品の固定の大きさ (幅, 高さ)。auto と重ね物の大きさに使う。
/// 右の列の部品は panel.rs・notice.rs が決めている高さ、重ね物は frame.rs の OKINAWA (幅は 9:7 の範囲の縦横比で約 225.9)・
/// panel.rs の LEGEND_RECT・時計 (176x74) に合わせる。None なら配信は描かない
fn natural(slot: &str) -> Option<(f32, f32)> {
    Some(match slot {
        "main" => (0.0, 0.0),
        "topbar" => (1280.0, 36.0),
        "detail" => (380.0, 144.0),
        "history" => (380.0, 312.0),
        "notice" => (380.0, 138.0),
        "credit" => (380.0, 90.0),
        "map-sub" => (380.0, 300.0),
        "inset" => (226.0, 220.0),
        "legend" => (34.0, 147.0),
        "clock" => (176.0, 74.0),
        _ => return None,
    })
}

/// 定義に置かれていて、配信が描けない部品の名前 (出てきた順。呼び出し側がログに出す)
pub fn unsupported_slots(def: &LayoutDef) -> Vec<&str> {
    slots_of(&def.root)
        .into_iter()
        .filter(|s| natural(s).is_none())
        .collect()
}

/// 定義を幅 w・高さ h の画面に割り付け、部品の名前 → 矩形 を返す。重ね物の部品も入る
pub fn resolve(def: &LayoutDef, w: f32, h: f32) -> Result<BTreeMap<String, Rect>> {
    let mut out = BTreeMap::new();
    Screen { w, h }.place(&def.root, Rect { x: 0.0, y: 0.0, w, h }, &mut out)?;
    Ok(out)
}

#[derive(Clone, Copy)]
struct Screen {
    w: f32,
    h: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Start,
    Center,
    End,
}

/// 描く中身があるか (描けない部品だけの容器は場所を取らない)
fn has_content(node: &Node) -> bool {
    slots_of(node).iter().any(|s| natural(s).is_some())
}

fn stack_has_content(s: &Stack) -> bool {
    s.items.iter().any(item_has_content)
}

fn item_has_content(i: &StackItem) -> bool {
    match i {
        StackItem::Name(n) => natural(n).is_some(),
        StackItem::Part(p) => natural(&p.slot).is_some(),
        StackItem::Stack(s) => stack_has_content(s),
    }
}

impl Screen {
    /// 長さ (px・%・vw・vh) を px にする。% は base に対する割合
    fn px(self, size: Size, base: f32) -> f32 {
        match size {
            Size::Px(v) => v,
            Size::Pct(p) => base * p / 100.0,
            Size::Vw(p) => self.w * p / 100.0,
            Size::Vh(p) => self.h * p / 100.0,
            Size::Fill(_) | Size::Auto => 0.0,
        }
    }

    fn place(self, node: &Node, rect: Rect, out: &mut BTreeMap<String, Rect>) -> Result<()> {
        if let Some(slot) = node.slot.as_deref().filter(|s| natural(s).is_some()) {
            out.insert(slot.to_string(), rect);
        }
        let dir = node.dir.unwrap_or(Dir::Column);
        let kids: Vec<&Node> = node.children.iter().flatten().filter(|c| has_content(c)).collect();
        let extent = if dir == Dir::Column { rect.h } else { rect.w };
        let sizes = self.main_sizes(&kids, dir, extent)?;
        let mut at = if dir == Dir::Column { rect.y } else { rect.x };
        for (kid, size) in kids.iter().zip(sizes) {
            let r = match dir {
                Dir::Column => Rect {
                    x: rect.x,
                    y: at,
                    w: rect.w,
                    h: size,
                },
                Dir::Row => Rect {
                    x: at,
                    y: rect.y,
                    w: size,
                    h: rect.h,
                },
            };
            self.place(kid, r, out)?;
            at += size;
        }
        for (corner, stack) in node.overlays.iter().flatten() {
            self.place_corner(*corner, stack, rect, out)?;
        }
        Ok(())
    }

    /// 子の、容器の向きに沿った大きさ。固定を先に引いて、残りを fill の重みで分ける (残りが無ければ 0)
    fn main_sizes(self, kids: &[&Node], dir: Dir, extent: f32) -> Result<Vec<f32>> {
        let sizes = kids
            .iter()
            .map(|k| parse_size(k.size.as_deref().unwrap_or("fill")))
            .collect::<Result<Vec<_>>>()?;
        let fixed = |k: &Node, s: Size| match s {
            Size::Auto => self.natural_main(k, dir),
            s => self.px(s, extent),
        };
        let used: f32 = kids
            .iter()
            .zip(&sizes)
            .filter(|(_, s)| !matches!(s, Size::Fill(_)))
            .map(|(k, s)| fixed(k, *s))
            .sum();
        let weights: f32 = sizes.iter().map(|s| if let Size::Fill(w) = s { *w } else { 0.0 }).sum();
        let rest = (extent - used).max(0.0);
        Ok(kids
            .iter()
            .zip(&sizes)
            .map(|(k, s)| match s {
                Size::Fill(w) => rest * w / weights,
                s => fixed(k, *s),
            })
            .collect())
    }

    /// auto のときの、向きに沿った大きさ。部品は固定の大きさ、容器は中身の合計 (向きが違えば一番大きい子)
    fn natural_main(self, node: &Node, axis: Dir) -> f32 {
        let pick = |(w, h): (f32, f32)| if axis == Dir::Column { h } else { w };
        if let Some(sz) = node.slot.as_deref().and_then(natural) {
            return pick(sz);
        }
        let kids = node.children.iter().flatten().filter(|c| has_content(c));
        let each = kids.map(|k| self.natural_main(k, axis));
        if node.dir.unwrap_or(Dir::Column) == axis {
            each.sum()
        } else {
            each.fold(0.0, f32::max)
        }
    }

    fn gap(self, s: &Stack) -> Result<f32> {
        // web の .ld-stack の既定 (gap: 10px)
        Ok(s.gap
            .as_deref()
            .map(parse_length)
            .transpose()?
            .map_or(10.0, |g| self.px(g, 0.0)))
    }

    /// 積みの大きさ (幅, 高さ)。描く中身が無ければ (0, 0)
    fn stack_size(self, s: &Stack) -> Result<(f32, f32)> {
        let items: Vec<(f32, f32)> = s
            .items
            .iter()
            .filter(|i| item_has_content(i))
            .map(|i| self.item_size(i))
            .collect::<Result<_>>()?;
        if items.is_empty() {
            return Ok((0.0, 0.0));
        }
        let gaps = self.gap(s)? * (items.len() - 1) as f32;
        let (sum_w, sum_h) = items.iter().fold((0.0, 0.0), |a, i| (a.0 + i.0, a.1 + i.1));
        let (max_w, max_h) = items
            .iter()
            .fold((0.0, 0.0), |a, i| (f32::max(a.0, i.0), f32::max(a.1, i.1)));
        Ok(match s.flow {
            Dir::Column => (max_w, sum_h + gaps),
            Dir::Row => (sum_w + gaps, max_h),
        })
    }

    fn item_size(self, i: &StackItem) -> Result<(f32, f32)> {
        match i {
            StackItem::Name(n) => Ok(natural(n).unwrap_or_default()),
            StackItem::Part(p) => Ok(natural(&p.slot).unwrap_or_default()),
            StackItem::Stack(s) => self.stack_size(s),
        }
    }

    /// pad (CSS の 1〜4 個の書き方) を 上・右・下・左 にする。既定は 10px (web の .ld-corner)
    fn pad(self, s: &Stack, rect: Rect) -> Result<[f32; 4]> {
        let Some(p) = &s.pad else { return Ok([10.0; 4]) };
        let v = p.split(' ').map(parse_length).collect::<Result<Vec<_>>>()?;
        let at = |i: usize, base: f32| self.px(v[i], base);
        let (t, r, b, l) = match v.len() {
            1 => (0, 0, 0, 0),
            2 => (0, 1, 0, 1),
            3 => (0, 1, 2, 1),
            _ => (0, 1, 2, 3),
        };
        Ok([at(t, rect.h), at(r, rect.w), at(b, rect.h), at(l, rect.w)])
    }

    fn place_corner(self, corner: Corner, s: &Stack, rect: Rect, out: &mut BTreeMap<String, Rect>) -> Result<()> {
        if !stack_has_content(s) {
            return Ok(());
        }
        let (ha, va) = match corner {
            Corner::TopLeft => (Align::Start, Align::Start),
            Corner::TopRight => (Align::End, Align::Start),
            Corner::BottomLeft => (Align::Start, Align::End),
            Corner::BottomRight => (Align::End, Align::End),
            Corner::Top => (Align::Center, Align::Start),
            Corner::Bottom => (Align::Center, Align::End),
        };
        let (w, h) = self.stack_size(s)?;
        let [pt, pr, pb, pl] = self.pad(s, rect)?;
        let x = match ha {
            Align::Start => rect.x + pl,
            Align::End => rect.x + rect.w - pr - w,
            Align::Center => rect.x + (rect.w - w) / 2.0,
        };
        let y = match va {
            Align::Start => rect.y + pt,
            Align::End => rect.y + rect.h - pb - h,
            Align::Center => rect.y + (rect.h - h) / 2.0,
        };
        self.place_stack(s, Rect { x, y, w, h }, (ha, va), out)
    }

    /// 積みの箱の中に、積みの向きに沿って gap をあけて並べる。交差方向は隅に寄せる側にそろえる
    fn place_stack(self, s: &Stack, b: Rect, align: (Align, Align), out: &mut BTreeMap<String, Rect>) -> Result<()> {
        let gap = self.gap(s)?;
        let (mut x, mut y) = (b.x, b.y);
        for item in s.items.iter().filter(|i| item_has_content(i)) {
            let (w, h) = self.item_size(item)?;
            let r = match s.flow {
                Dir::Column => Rect {
                    x: aligned(align.0, b.x, b.w, w),
                    y,
                    w,
                    h,
                },
                Dir::Row => Rect {
                    x,
                    y: aligned(align.1, b.y, b.h, h),
                    w,
                    h,
                },
            };
            match item {
                StackItem::Name(n) => drop(out.insert(n.clone(), r)),
                StackItem::Part(p) => drop(out.insert(p.slot.clone(), r)),
                StackItem::Stack(inner) => self.place_stack(inner, r, align, out)?,
            }
            match s.flow {
                Dir::Column => y += h + gap,
                Dir::Row => x += w + gap,
            }
        }
        Ok(())
    }
}

/// 長さ len のものを、origin から size の幅の中に寄せたときの位置
fn aligned(a: Align, origin: f32, size: f32, len: f32) -> f32 {
    match a {
        Align::Start => origin,
        Align::Center => origin + (size - len) / 2.0,
        Align::End => origin + size - len,
    }
}
