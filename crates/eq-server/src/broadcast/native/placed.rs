//! 描画が使う、部品ごとの矩形 (layout_resolve で割り付けたもの)。
//! 描画は「矩形の原点 + 部品の中の相対オフセット」で描く。定義に置かれていない部品 (None) は描かない。
//!
//! 地震の画面 (サブの地図を描くとき) は、右の列の詳細・履歴・サブの地図を地震の画面の定義 (broadcast-quake) の
//! 矩形で描く (for_screen)。main・topbar・出典・時計・凡例・寄り図は平時と同じ矩形のまま (base が共通なので、
//! 定義の側でも同じにしておく。出典は動かない地の側に描かれる)。

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
    /// 地震の画面の定義での詳細・履歴の矩形 (for_screen で detail・history と入れ替える)
    quake_detail: Option<Rect>,
    quake_history: Option<Rect>,
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
        Ok(Placed {
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
            quake_detail: q.get("detail").copied(),
            quake_history: q.get("history").copied(),
        })
    }

    /// 地震の画面 (サブの地図が出る) なら、右の列を地震の画面の矩形にした Placed。平時はそのまま
    pub fn for_screen(&self, quake: bool) -> Placed {
        if quake {
            Placed {
                detail: self.quake_detail,
                history: self.quake_history,
                ..*self
            }
        } else {
            *self
        }
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
