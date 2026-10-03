//! 平時の右パネルの下半分に出すお知らせ (web の「バナー」。docs/broadcast-native.md)。
//! GET /api/banners の文字の項目だけを、interval_sec ごとに順に出す (画像だけの項目は出さない)。
//! 並べた行はキャッシュして、コマごとに字形を測り直さない。

use serde::Deserialize;
use tiny_skia::Pixmap;

use super::draw::{H, MAP_W, W};
use super::paint::{rrect, BG, LINE, TEXT};
use super::text::Text;

/// 箱の左上と幅 (右パネルの左右の余白は panel.rs の PAD と同じ 16)。履歴の下、出典の上に収まる
const BOX_X: f32 = MAP_W + 16.0;
const BOX_Y: f32 = 492.0;
const BOX_W: f32 = W as f32 - 16.0 - BOX_X;
const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 10.0;
const PX: f32 = 14.0;
const LINE_H: f32 = 21.0;
const MAX_LINES: usize = 4;
const ELLIPSIS: char = '…';
/// 右パネルの出典の一番上 (panel.rs の CREDIT は 6 行。1 行目の字の上端より少し上)
const CREDIT_TOP: f32 = H as f32 - 5.0 - 15.0 * 5.0 - 10.0;
/// 履歴の最後の行の下 (panel.rs の draw_history: 216 から 52 ずつ 5 件)
const HISTORY_BOTTOM: f32 = 216.0 + 5.0 * 52.0;
// 4 行の箱でも、履歴と出典に重ならない
const _: () = assert!(BOX_Y > HISTORY_BOTTOM && BOX_Y + PAD_Y * 2.0 + LINE_H * (MAX_LINES as f32) < CREDIT_TOP);

/// GET /api/banners
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Banners {
    #[serde(default)]
    pub interval_sec: u64,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub text: Option<String>,
}

/// 出すお知らせ (文字だけ。リンクの行は除いてある)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notices {
    pub interval_s: u64,
    pub texts: Vec<String>,
}

impl Banners {
    pub fn notices(&self) -> Notices {
        Notices {
            interval_s: self.interval_sec,
            texts: self
                .items
                .iter()
                .filter_map(|i| shown_text(i.text.as_deref()?))
                .collect(),
        }
    }
}

/// http:// か https:// で始まる行 (リンク先) を除き、前後の空白を落とす。空になれば None (サーバも同じ規則で分けている)
fn shown_text(raw: &str) -> Option<String> {
    let kept: Vec<&str> = raw
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("http://") || t.starts_with("https://"))
        })
        .collect();
    let text = kept.join("\n").trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// 時刻 now_ms に出す項目の番号。interval_s ごとに次へ進み、一巡したら戻る (時計ではなくコマの時刻で決める)
pub fn index_at(now_ms: u64, interval_s: u64, count: usize) -> Option<usize> {
    let slot = now_ms / (interval_s.max(1) * 1000);
    (count > 0).then(|| (slot % count as u64) as usize)
}

/// 文を幅 max_w の行に割る (改行は守る)。max_lines 行に収まらないときは、最後の行を削って「…」を付ける。
/// adv は 1 字の幅
pub fn layout(s: &str, max_w: f32, max_lines: usize, adv: &mut impl FnMut(char) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for para in s.lines() {
        let (mut cur, mut w) = (String::new(), 0.0);
        for c in para.chars() {
            let a = adv(c);
            if w + a > max_w && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                w = 0.0;
            }
            cur.push(c);
            w += a;
        }
        lines.push(cur);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            let ell = adv(ELLIPSIS);
            let mut w: f32 = last.chars().map(&mut *adv).sum();
            while w + ell > max_w {
                let Some(c) = last.pop() else { break };
                w -= adv(c);
            }
            last.push(ELLIPSIS);
        }
    }
    lines
}

/// 並べた行の覚え (文が変わったときだけ並べ直す)
#[derive(Default)]
pub struct LayoutCache(Option<(String, Vec<String>)>);

impl LayoutCache {
    fn lines(&mut self, text: &mut Text, s: &str) -> &[String] {
        if self.0.as_ref().is_none_or(|(k, _)| k != s) {
            let max_w = BOX_W - PAD_X * 2.0;
            let lines = layout(s, max_w, MAX_LINES, &mut |c| text.width(c.encode_utf8(&mut [0; 4]), PX));
            self.0 = Some((s.to_string(), lines));
        }
        self.0.as_ref().map_or(&[], |(_, l)| l)
    }
}

/// 箱の高さ (行数で決まる)
fn box_height(lines: usize) -> f32 {
    PAD_Y * 2.0 + LINE_H * lines as f32
}

/// now_ms に出すお知らせを、右パネルの下半分に描く (平時だけ呼ぶ。出すものが無ければ何もしない)
pub fn draw(pm: &mut Pixmap, text: &mut Text, cache: &mut LayoutCache, n: &Notices, now_ms: u64) {
    let Some(i) = index_at(now_ms, n.interval_s, n.texts.len()) else {
        return;
    };
    let lines = cache.lines(text, &n.texts[i]);
    let h = box_height(lines.len().max(1));
    rrect(pm, BOX_X, BOX_Y, BOX_W, h, 8.0, LINE, 1.0);
    rrect(pm, BOX_X + 1.0, BOX_Y + 1.0, BOX_W - 2.0, h - 2.0, 7.0, BG, 1.0);
    for (k, line) in lines.iter().enumerate() {
        let y = BOX_Y + PAD_Y + LINE_H * k as f32 + 15.0;
        text.draw_center(pm, line, BOX_X + BOX_W / 2.0, y, PX, TEXT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Notices {
        serde_json::from_str::<Banners>(json).unwrap().notices()
    }

    #[test]
    fn only_text_items_are_kept_and_link_lines_are_dropped() {
        let n = parse(
            r#"{"interval_sec":20,"items":[
                {"image":"/b/a.png","text":null,"link":null},
                {"image":null,"text":"保守のお知らせ","link":null},
                {"image":"/b/c.png","text":"画像とあわせて\nhttps://example.com/x","link":"https://example.com/x"},
                {"image":null,"text":"  \nhttp://example.com\n","link":"http://example.com"}
            ]}"#,
        );
        assert_eq!(n.interval_s, 20);
        assert_eq!(n.texts, ["保守のお知らせ", "画像とあわせて"]);
    }

    #[test]
    fn a_missing_list_is_empty() {
        assert_eq!(parse("{}"), Notices::default());
    }

    #[test]
    fn rotation_follows_the_frame_time() {
        assert_eq!(index_at(0, 20, 3), Some(0));
        assert_eq!(index_at(19_999, 20, 3), Some(0));
        assert_eq!(index_at(20_000, 20, 3), Some(1));
        assert_eq!(index_at(60_000, 20, 3), Some(0));
        assert_eq!(index_at(5_000, 0, 2), Some(1)); // 0 秒は 1 秒に直す
        assert_eq!(index_at(5_000, 20, 0), None);
    }

    fn wide(_: char) -> f32 {
        10.0
    }

    #[test]
    fn long_text_wraps_to_the_width_and_keeps_newlines() {
        assert_eq!(
            layout("あいうえおかきく", 30.0, 4, &mut wide),
            ["あいう", "えおか", "きく"]
        );
        assert_eq!(layout("ab\ncd", 100.0, 4, &mut wide), ["ab", "cd"]);
    }

    #[test]
    fn overflow_is_cut_with_an_ellipsis_inside_the_width() {
        let lines = layout("あいうえおかきくけこさしすせそ", 30.0, 2, &mut wide);
        assert_eq!(lines, ["あいう", "えお…"]);
        // ちょうど収まるなら省かない
        assert_eq!(layout("あいうえおか", 30.0, 2, &mut wide), ["あいう", "えおか"]);
    }
}
