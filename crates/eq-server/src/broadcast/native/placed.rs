//! 描画が使う、部品ごとの矩形 (layout_resolve で割り付けたもの)。
//! 描画は「矩形の原点 + 部品の中の相対オフセット」で描く。定義に置かれていない部品 (None) は描かない。
//!
//! 地震の画面でも、map-sub 以外の部品は平時の定義 (broadcast) の矩形を使う (今の描画は地震の画面でも
//! 履歴などを同じ場所に描き、サブの地図がその上を隠すため)。地震の画面の定義 (broadcast-quake) から使うのは
//! map-sub の矩形だけ。他の部品を地震の画面の矩形で描くのは、出力が変わる段 (Task 4) で行う。

use anyhow::{Context, Result};

use super::draw::{H, W};
use super::layout_def::{self, LayoutDef};
use super::layout_resolve::{resolve, Rect};
use super::notice;

#[derive(Debug, Clone, Copy)]
pub struct Placed {
    pub main: Rect,
    pub topbar: Option<Rect>,
    pub detail: Option<Rect>,
    pub history: Option<Rect>,
    pub notice: Option<Rect>,
    pub credit: Option<Rect>,
    pub inset: Option<Rect>,
    pub legend: Option<Rect>,
    pub clock: Option<Rect>,
    /// サブの地図 (地震の画面の定義から)
    pub sub: Option<Rect>,
}

impl Placed {
    /// 平時の定義と地震の画面の定義を、画面 (W×H) に割り付ける
    pub fn new(calm: &LayoutDef, quake: &LayoutDef) -> Result<Placed> {
        let (w, h) = (W as f32, H as f32);
        let c = resolve(calm, w, h)?;
        let q = resolve(quake, w, h)?;
        // お知らせの箱が最大の高さで収まらない矩形には描かない (今までのコンパイル時の検査の代わり)
        let notice = c.get("notice").copied().filter(|r| {
            let fits = r.h >= notice::BOX_MAX_H;
            if !fits {
                tracing::warn!(
                    "broadcast: layout のお知らせの矩形が低すぎる ({} < {}) ので、お知らせは描きません",
                    r.h,
                    notice::BOX_MAX_H
                );
            }
            fits
        });
        let placed = Placed {
            main: *c.get("main").context("部品 main の矩形が無い")?,
            topbar: c.get("topbar").copied(),
            detail: c.get("detail").copied(),
            history: c.get("history").copied(),
            notice,
            credit: c.get("credit").copied(),
            inset: c.get("inset").copied(),
            legend: c.get("legend").copied(),
            clock: c.get("clock").copied(),
            sub: q.get("map-sub").copied(),
        };
        placed.check_on_screen()?;
        Ok(placed)
    }

    /// 割り付けた矩形が画面 (W×H) の中にあり、大きさが 1px 以上か。外れていれば誤り (呼び出し側が組み込みの定義に戻る)。
    /// 固定の大きさの合計が画面を超えた・地図の幅が 0 になった、などの定義で、地図の無い絵を流し続けないため
    fn check_on_screen(&self) -> Result<()> {
        let (w, h) = (W as f32, H as f32);
        let named = [
            ("main", Some(self.main)),
            ("topbar", self.topbar),
            ("detail", self.detail),
            ("history", self.history),
            ("notice", self.notice),
            ("credit", self.credit),
            ("inset", self.inset),
            ("legend", self.legend),
            ("clock", self.clock),
            ("map-sub", self.sub),
        ];
        for (name, r) in named {
            let Some(r) = r else { continue };
            let finite = [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite());
            let inside = r.x >= -0.5 && r.y >= -0.5 && r.right() <= w + 0.5 && r.bottom() <= h + 0.5;
            anyhow::ensure!(
                finite && r.w >= 1.0 && r.h >= 1.0 && inside,
                "部品 {name} の矩形 ({}, {}, {}, {}) が画面 {w}x{h} の中に収まらない",
                r.x,
                r.y,
                r.w,
                r.h
            );
        }
        Ok(())
    }

    /// 組み込みの定義 (broadcast・broadcast-quake)。再現動画とテストが使う
    pub fn builtin() -> Result<Placed> {
        let (calm, quake) = layout_def::load(None, "broadcast", "broadcast-quake")?;
        Placed::new(&calm, &quake)
    }

    /// 右パネルの地 (地図の右から画面の右端まで)
    pub fn side(&self) -> Rect {
        Rect {
            x: self.main.right(),
            y: self.main.y,
            w: W as f32 - self.main.right(),
            h: H as f32 - self.main.y,
        }
    }

    /// 地図の枠の縦横比
    pub fn map_aspect(&self) -> f64 {
        self.main.w as f64 / self.main.h as f64
    }

    /// サブの地図の枠の縦横比 (置いていなければ None)
    pub fn sub_aspect(&self) -> Option<f64> {
        self.sub.map(|r| r.w as f64 / r.h as f64)
    }
}
