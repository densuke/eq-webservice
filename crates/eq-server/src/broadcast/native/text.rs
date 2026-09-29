//! 文字を描く (fontdue)。字形は一度描いたらキャッシュする。
//! フォントが読めないとき (CI など) は、文字を描かずに済ませる (Text::none)。

#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};
use tiny_skia::Pixmap;

pub struct Text {
    font: Option<Font>,
    cache: HashMap<(char, u32), (Metrics, Vec<u8>)>,
}

impl Text {
    pub fn enabled(&self) -> bool {
        self.font.is_some()
    }

    /// 文字を描かない
    pub fn none() -> Text {
        Text {
            font: None,
            cache: HashMap::new(),
        }
    }

    /// フォントを読む (.ttc は index 番目)。読めなければ文字を描かない
    pub fn load(path: &str, index: u32) -> anyhow::Result<Text> {
        let bytes = std::fs::read(path)?;
        let font = Font::from_bytes(
            bytes,
            FontSettings {
                collection_index: index,
                ..FontSettings::default()
            },
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Text {
            font: Some(font),
            cache: HashMap::new(),
        })
    }

    fn glyph(&mut self, c: char, px: f32) -> Option<&(Metrics, Vec<u8>)> {
        let font = self.font.as_ref()?;
        let key = (c, px.round() as u32);
        Some(self.cache.entry(key).or_insert_with(|| font.rasterize(c, px)))
    }

    /// 1 行の幅
    pub fn width(&mut self, s: &str, px: f32) -> f32 {
        s.chars()
            .map(|c| self.glyph(c, px).map_or(0.0, |(m, _)| m.advance_width))
            .sum()
    }

    /// x から右へ、baseline を基準に 1 行を描く。幅を返す
    pub fn draw(&mut self, pm: &mut Pixmap, s: &str, x: f32, baseline: f32, px: f32, color: [u8; 3]) -> f32 {
        let mut pen = x;
        for c in s.chars() {
            let Some((m, bitmap)) = self.glyph(c, px) else { break };
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            blend(pm, bitmap, m.width, gx, gy, color);
            pen += m.advance_width;
        }
        pen - x
    }

    /// 中央そろえ
    pub fn draw_center(&mut self, pm: &mut Pixmap, s: &str, cx: f32, baseline: f32, px: f32, color: [u8; 3]) {
        let w = self.width(s, px);
        self.draw(pm, s, cx - w / 2.0, baseline, px, color);
    }

    /// 右そろえ
    pub fn draw_right(&mut self, pm: &mut Pixmap, s: &str, right: f32, baseline: f32, px: f32, color: [u8; 3]) {
        let w = self.width(s, px);
        self.draw(pm, s, right - w, baseline, px, color);
    }

    /// 幅 max に収まるところまでを描く (収まらなければ「…」で切る)
    pub fn draw_fit(&mut self, pm: &mut Pixmap, s: &str, x: f32, baseline: f32, px: f32, color: [u8; 3], max: f32) {
        let mut text = s.to_string();
        while self.width(&text, px) > max && text.chars().count() > 1 {
            text.pop();
            text.pop();
            text.push('…');
        }
        self.draw(pm, &text, x, baseline, px, color);
    }
}

/// 文字の濃さ (0..255) の画像を、色 color で pm に重ねる (pm は不透明の背景に描く前提の premultiplied RGBA)
fn blend(pm: &mut Pixmap, coverage: &[u8], w: usize, gx: i32, gy: i32, color: [u8; 3]) {
    if w == 0 {
        return;
    }
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    for (i, &cov) in coverage.iter().enumerate() {
        let (x, y) = (gx + (i % w) as i32, gy + (i / w) as i32);
        if cov == 0 || x < 0 || y < 0 || x >= pw || y >= ph {
            continue;
        }
        let a = cov as u32;
        let o = ((y * pw + x) * 4) as usize;
        for k in 0..3 {
            data[o + k] = ((color[k] as u32 * a + data[o + k] as u32 * (255 - a) + 127) / 255) as u8;
        }
        data[o + 3] = (a + (data[o + 3] as u32 * (255 - a) + 127) / 255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_font_nothing_is_drawn() {
        let mut t = Text::none();
        let mut pm = Pixmap::new(4, 4).unwrap();
        assert_eq!(t.draw(&mut pm, "震度", 0.0, 3.0, 12.0, [255, 255, 255]), 0.0);
        assert!(pm.data().iter().all(|&b| b == 0));
        assert_eq!(t.width("震度", 12.0), 0.0);
    }

    #[test]
    fn a_glyph_is_mixed_by_its_coverage_and_clipped_at_the_edge() {
        let mut pm = Pixmap::new(2, 1).unwrap();
        pm.data_mut().copy_from_slice(&[0, 0, 0, 255, 0, 0, 0, 255]);
        // 2 画素の字形を x=1 に置く (右の 1 画素は画面の外)
        blend(&mut pm, &[255, 128], 2, 1, 0, [200, 100, 0]);
        assert_eq!(&pm.data()[..4], &[0, 0, 0, 255]);
        assert_eq!(&pm.data()[4..], &[200, 100, 0, 255]);
        let mut pm = Pixmap::new(1, 1).unwrap();
        pm.data_mut().copy_from_slice(&[0, 0, 0, 255]);
        blend(&mut pm, &[128], 1, 0, 0, [200, 100, 0]);
        assert_eq!(&pm.data()[..3], &[100, 50, 0]);
    }
}
