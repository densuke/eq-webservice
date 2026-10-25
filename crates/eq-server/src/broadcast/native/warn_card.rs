//! 平時に危険警報以上の区域へ寄せたときの札と矢印 (web/src/warn-card.ts と同じ置き方)。
//! 区域と重ならない側 (左・下・右・上の順) に札を置き、区域の中心へ矢印を引く

use tiny_skia::Pixmap;

use super::data::WarningLevel;
use super::frame::BoxRect;
use super::paint::{line, rrect, PANEL, TEXT};
use super::text::Text;
use super::warn_focus::WarnFocus;

/// 画面の座標の矩形 (x0, y0, x1, y1)
pub type Region = (f32, f32, f32, f32);

/// 札の左上と、矢印 (札の縁 from → 区域の中心の手前 to)
#[derive(Debug, PartialEq)]
pub struct CardPlace {
    pub x: f32,
    pub y: f32,
    pub from: (f32, f32),
    pub to: (f32, f32),
}

const GAP: f32 = 48.0;
const MARGIN: f32 = 8.0;

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.min(hi.max(lo)).max(lo)
}

/// region は区域の外接矩形、card は札の大きさ (幅, 高さ)、area は地図の枠 (x, y, 幅, 高さ)、avoid は凡例・時計など
/// 重ねたくない札 (x, y, 幅, 高さ)。すべて画面の座標。区域に重ならないことを先に、次に avoid に重ならないものを選ぶ
pub fn place_card(region: Region, card: (f32, f32), area: BoxRect, avoid: &[BoxRect]) -> CardPlace {
    let (x0, y0, x1, y1) = region;
    let (w, h) = card;
    let (ax, ay, aw, ah) = area;
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let fit = |(x, y): (f32, f32)| {
        (
            clamp(x, ax + MARGIN, ax + aw - w - MARGIN),
            clamp(y, ay + MARGIN, ay + ah - h - MARGIN),
        )
    };
    let over = |(x, y): (f32, f32), (rx0, ry0, rx1, ry1): Region| {
        ((x + w).min(rx1) - x.max(rx0)).max(0.0) * ((y + h).min(ry1) - y.max(ry0)).max(0.0)
    };
    let covers = |c: (f32, f32)| {
        over(c, region) * 1e6
            + avoid
                .iter()
                .map(|&(ax, ay, aw, ah)| over(c, (ax, ay, ax + aw, ay + ah)))
                .sum::<f32>()
    };
    let (x, y) = [
        (x0 - GAP - w, cy - h / 2.0 + GAP),
        (cx - w / 2.0, y1 + GAP),
        (x1 + GAP, cy - h / 2.0 + GAP),
        (cx - w / 2.0, y0 - GAP - h),
    ]
    .into_iter()
    .map(fit)
    .reduce(|best, c| if covers(c) < covers(best) { c } else { best })
    .expect("candidates");
    // 矢印の根元は札の縁で区域の中心に一番近い点。先は中心の手前で止める
    let from = (clamp(cx, x, x + w), clamp(cy, y, y + h));
    let d = (cx - from.0).hypot(cy - from.1).max(1.0);
    let stop = (d / 2.0).min(10.0);
    let to = (cx - (cx - from.0) / d * stop, cy - (cy - from.1) / d * stop);
    CardPlace { x, y, from, to }
}

/// 札の枠と矢印の色 (危険警報は紫、特別警報は白。web の .warn-card-layer)
fn level_color(l: WarningLevel) -> [u8; 3] {
    match l {
        WarningLevel::Emergency => [0xff, 0xff, 0xff],
        _ => [0xaa, 0x00, 0xff],
    }
}

const NAME_PX: f32 = 19.0;
const PLACES_PX: f32 = 16.0;
const PAD: f32 = 12.0;
const MAX_W: f32 = 400.0;
/// 1 種類の市町村の行数の上限 (種類と合わせて 1 種類 4 行)
const PLACE_LINES: usize = 3;
const MAX_ITEMS: usize = 2;

/// s を幅 max に収まる行に分ける (文字単位)。lines 行を超える分は最後の行を「…」で切る
pub fn wrap(text: &mut Text, s: &str, px: f32, max: f32, lines: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        let next = format!("{cur}{ch}");
        if !cur.is_empty() && text.width(&next, px) > max {
            out.push(std::mem::take(&mut cur));
            if out.len() == lines {
                let last = out.last_mut().expect("line");
                while !last.is_empty() && text.width(&format!("{last}…"), px) > max {
                    last.pop();
                }
                last.push('…');
                return out;
            }
            cur = ch.to_string();
        } else {
            cur = next;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 寄せた区域 region (画面の座標) に札と矢印を描く。area は地図の枠、avoid は重ねたくない札
pub fn draw(pm: &mut Pixmap, text: &mut Text, f: &WarnFocus, region: Region, area: BoxRect, avoid: &[BoxRect]) {
    let inner = MAX_W.min(area.2 * 0.75) - PAD * 2.0;
    let mut rows: Vec<(String, f32, [u8; 3])> = Vec::new();
    for (i, item) in f.items.iter().take(MAX_ITEMS).enumerate() {
        if i > 0 {
            rows.push((String::new(), 8.0, TEXT));
        }
        rows.push((item.name.clone(), NAME_PX, TEXT));
        for l in wrap(text, &item.places(usize::MAX), PLACES_PX, inner, PLACE_LINES) {
            rows.push((l, PLACES_PX, [0xd0, 0xd7, 0xde]));
        }
    }
    let rest = f.items.len().saturating_sub(MAX_ITEMS);
    if rest > 0 {
        rows.push((format!("ほか {rest} 件"), PLACES_PX - 2.0, [0x8b, 0x94, 0x9e]));
    }
    let line_h = |px: f32| if px < 10.0 { px } else { px * 1.4 };
    let w = rows
        .iter()
        .map(|(s, px, _)| text.width(s, *px))
        .fold(0.0, f32::max)
        .min(inner)
        + PAD * 2.0;
    let h = rows.iter().map(|(_, px, _)| line_h(*px)).sum::<f32>() + PAD * 2.0 - 4.0;
    let p = place_card(region, (w, h), area, avoid);
    let color = level_color(f.top);
    line(pm, p.from, p.to, 3.0, color, 1.0);
    arrow_head(pm, p.from, p.to, color);
    rrect(pm, p.x, p.y, w, h, 8.0, color, 1.0);
    rrect(pm, p.x + 2.0, p.y + 2.0, w - 4.0, h - 4.0, 6.0, PANEL, 0.95);
    let mut y = p.y + PAD;
    for (s, px, c) in &rows {
        let lh = line_h(*px);
        if !s.is_empty() {
            text.draw(pm, s, p.x + PAD, y + lh * 0.75, *px, *c);
        }
        y += lh;
    }
}

/// 矢印の先の三角 (線の太さの 3 倍ほど)
fn arrow_head(pm: &mut Pixmap, from: (f32, f32), to: (f32, f32), color: [u8; 3]) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let d = dx.hypot(dy).max(1.0);
    let (ux, uy) = (dx / d, dy / d);
    let back = (to.0 - ux * 10.0, to.1 - uy * 10.0);
    for side in [-1.0, 1.0] {
        let wing = (back.0 - uy * 6.0 * side, back.1 + ux * 6.0 * side);
        line(pm, wing, to, 3.0, color, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlap((x, y, w, h): (f32, f32, f32, f32), r: Region) -> f32 {
        ((x + w).min(r.2) - x.max(r.0)).max(0.0) * ((y + h).min(r.3) - y.max(r.1)).max(0.0)
    }

    #[test]
    fn the_card_goes_beside_the_area_without_covering_it_and_points_at_it() {
        let region = (400.0, 250.0, 500.0, 330.0);
        let p = place_card(region, (260.0, 70.0), (0.0, 0.0, 900.0, 600.0), &[]);
        assert_eq!(overlap((p.x, p.y, 260.0, 70.0), region), 0.0);
        assert!(p.x >= 0.0 && p.y >= 0.0 && p.x + 260.0 <= 900.0 && p.y + 70.0 <= 600.0);
        let c = (450.0f32, 290.0f32);
        assert!((p.to.0 - c.0).hypot(p.to.1 - c.1) < (p.from.0 - c.0).hypot(p.from.1 - c.1));
    }

    #[test]
    fn the_card_stays_inside_the_map_frame() {
        let area = (0.0, 86.0, 900.0, 634.0);
        let p = place_card((10.0, 100.0, 120.0, 200.0), (300.0, 90.0), area, &[]);
        assert!(p.x >= 0.0 && p.x + 300.0 <= 900.0 && p.y >= 86.0 && p.y + 90.0 <= 720.0);
        assert_eq!(overlap((p.x, p.y, 300.0, 90.0), (10.0, 100.0, 120.0, 200.0)), 0.0);
    }

    #[test]
    fn the_card_avoids_the_legend_when_another_side_is_free() {
        // 左に置くと凡例 (左下) に重なるので、下か右へ
        let region = (430.0, 390.0, 480.0, 420.0);
        let legend = (8.0, 400.0, 70.0, 310.0);
        let p = place_card(region, (390.0, 90.0), (0.0, 86.0, 900.0, 634.0), &[legend]);
        assert_eq!(overlap((p.x, p.y, 390.0, 90.0), (8.0, 400.0, 78.0, 710.0)), 0.0);
        assert_eq!(overlap((p.x, p.y, 390.0, 90.0), region), 0.0);
    }
}
