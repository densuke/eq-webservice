//! 描画が使う、部品ごとの矩形 (layout_resolve で割り付けたもの)。
//! 描画は「矩形の原点 + 部品の中の相対オフセット」で描く。定義に置かれていない部品 (None) は描かない。
//!
//! 地震の画面 (サブの地図を描くとき) は、右の列の詳細・履歴・サブの地図を地震の画面の定義 (broadcast-quake) の
//! 矩形で描く (for_screen)。main・topbar・出典・時計・凡例・寄り図は平時と同じ矩形のまま (base が共通なので、
//! 定義の側でも同じにしておく。出典は動かない地の側に描かれる)。

use std::collections::BTreeMap;

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

/// 地震の画面の右の列 (詳細・サブの地図・履歴・出典) が互いに重なる定義は誤り (重ねて描くと配信が崩れる)
fn check_quake_column(q: &BTreeMap<String, Rect>) -> Result<()> {
    let col: Vec<(&str, Rect)> = ["detail", "map-sub", "history", "credit"]
        .into_iter()
        .filter_map(|n| Some((n, *q.get(n)?)))
        .collect();
    for (i, (a, ra)) in col.iter().enumerate() {
        for (b, rb) in &col[i + 1..] {
            let overlap = ra.x < rb.right() && rb.x < ra.right() && ra.y < rb.bottom() && rb.y < ra.bottom();
            anyhow::ensure!(!overlap, "地震の画面の部品 {a} と {b} の矩形が重なっている");
        }
    }
    Ok(())
}

impl Placed {
    /// 平時の定義と地震の画面の定義を、画面 (W×H) に割り付ける
    pub fn new(calm: &LayoutDef, quake: &LayoutDef) -> Result<Placed> {
        let (w, h) = (W as f32, H as f32);
        let c = resolve(calm, w, h)?;
        let q = resolve(quake, w, h)?;
        check_quake_column(&q)?;
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
            quake_detail: q.get("detail").copied(),
            quake_history: q.get("history").copied(),
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
            ("detail (地震)", self.quake_detail),
            ("history (地震)", self.quake_history),
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
