# レイアウト段 A (試験定義 trial を JDQ の形の並びにする) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 地図 1 枚のまま、試験定義 trial (`?layout=trial`) を JDQ の画面の形 (左半分 = 全国の地図 + 下の帯、右半分 = 寄り図の場所 + パネル群) に並べ直す。段 B (2 地図) で何を作るかの目標にする。

**Architecture:** 変えるのは web/src/layout.json の trial と、trial だけに効く CSS (`body[data-layout="trial"]`)。既存の 3 定義 (landscape / regular / compact) は変えない。

**Tech Stack:** TypeScript、`node --test`、CSS。

**Spec:** docs/ui-spec/layout-system.js の `LS_PROFILES` の `jdq` (概算: 上段 74% = 左の全国図 49% + 右の最近の地震 51% の高さ、右の下にパネル群、左の下に時計とカメラ 22%)。docs/superpowers/plans/2026-10-06-layout-phase1.md (前の段) の Global Constraints を引き継ぐ。

## Global Constraints

- web のコマンドは `web/` で実行: `npm test` / `npm run typecheck` / `npm run build`。
- 既存の 3 定義の見た目は変えない (layout.test.ts の SHIPPED_PARTS の検査が通ること)。CSS は `body[data-layout="trial"]` の下だけに足す。例外は下の Task 1 の `.ep-rows dd` の 1 行 (パネルは trial にしか置かれない)。
- 日本語の conventional commit、署名なし。push とマージは親。

## Review Focus

1. 時計 (`clock`) を地図の重ね物から外して下の帯に置いても、天気の札の避け先 (map.ts の thinCities) と、接続状態の枠の色が今までどおり働くこと。→ Task 2 の目視。
2. 下の帯の高さが低い画面 (1280×720) で、時計とお知らせが切れないこと。→ Task 2 の目視。
3. 帯 (banners) を地図の左上に compact で置いたとき、EEW の札の巡回と、津波・気象警報の札が出ること (いまの trial は帯を省いていて、津波と気象警報が出ない)。→ Task 2 の目視。

---

### Task 1: trial の並びを JDQ の形にする

**Files:**
- Modify: `web/src/layout.json` (trial の定義だけ)
- Modify: `web/public/style.css` (末尾に trial 用の数行と `.ep-rows dd` の 1 行)
- Test: `web/src/layout.test.ts` (trial のテスト)

**Interfaces:**
- Consumes: 前の段の書式 (部品の省略・積みの gap/pad/minHeight・`manual`・`variant`)。

trial の定義 (`note` は「試験中。?layout=trial で選ぶ。JDQ の画面の形 (左: 全国の地図 + 下の帯 / 右: 寄り図の場所 + パネル群)。地図は 1 枚で、右上の寄り図の場所はいまは地震の詳細で代用 (段 B で 2 枚目の地図にする)」):

```
root: column
├ topbar (auto)
└ box "layout": row (fill)
   ├ div: column (fill)                       … 左半分
   │  ├ main (fill)  overlays:
   │  │    top-left:    column [ {banners, variant compact}, row [inset, caption], countdown ]
   │  │    bottom-left: column [ legend ]
   │  │    bottom-right: column [ ogasawara ]
   │  │    bottom:      column [ toast, hint ]
   │  └ div: row (22%)                         … 左下の帯 (JDQ の時計とカメラの場所)
   │     ├ clock (auto)
   │     └ notice (fill)
   └ box "side": column (fill)                 … 右半分
      ├ settings (auto)
      ├ detail (50%)                           … 寄り図の場所 (代用)
      ├ eew-panel (auto)
      ├ history-head (auto)
      ├ history (fill)
      └ credit (auto)
```

CSS (style.css の末尾、既存の `body[data-layout="landscape"]` の並びの後):
- `body[data-layout="trial"] .ld-box:has(> #clock)` … 下の帯: `border-top: 1px solid var(--line); background: var(--panel); align-items: center; gap: 12px; padding: 0 12px;` (セレクタは帯の div を指せればよい。`:has` が使いにくければ、時計とお知らせの側に余白を付けてもよい)
- `body[data-layout="trial"] .banner { border-top: none; }` (帯の上の線と重ねない)
- `.ep-rows dd { word-break: keep-all; overflow-wrap: anywhere; }` (「最後の発表」の値を空白の位置で折り返す。空白の無い長い震源名は文字の途中で折る)

- [ ] **Step 1: 失敗するテストを書く**

layout.test.ts の既存の trial のテストを、次の検査に置き換える (名前は `trial (manual) is the JDQ-shaped layout`):
- `chooseLayout(LAYOUTS, 1440, 900, "trial").name === "trial"`、`pickLayout(LAYOUTS, 1440, 900).name === "regular"`
- `slotsOf(trial.root)` が `["topbar", "main", "banners", "inset", "caption", "countdown", "legend", "ogasawara", "toast", "hint", "clock", "notice", "settings", "detail", "eew-panel", "history-head", "history", "credit"]` と同じ集合 (順は問わない: 両方を sort して比べる)
- `checkLayouts(shipped(), ...)` が空 (既存のテストで済んでいればそのまま)

- [ ] **Step 2: 落ちることを確かめる** — Run: `cd web && npm test` / Expected: FAIL (trial の部品の集合が違う)
- [ ] **Step 3: layout.json の trial と style.css を変える**
- [ ] **Step 4: 通ることを確かめる** — Run: `cd web && npm test && npm run typecheck && npm run build` / Expected: PASS
- [ ] **Step 5: Commit**

```bash
git add web/src/layout.json web/src/layout.test.ts web/public/style.css
git commit -m "feat: 試験定義 trial を JDQ の画面の形に並べる (地図 1 枚、右上は詳細で代用)"
```

---

### Task 2: 検証 (Opus)

実装はしない。

- [ ] `cd web && npm test && npm run typecheck && npm run build`
- [ ] web/dist を静的配信し、Playwright で `?layout=trial&demo=standard` を 1440×900・1280×720・1920×1080 で開く。平時 (デモ開始前)・EEW の発表中 (開いて約 13 秒後)・津波注意報が出ている時点のスクリーンショット。Review Focus 1〜3 を確かめる (時計の枠の色、天気の札が時計の位置を避ける必要がなくなったこと、帯の札が地図の左上に出ること、下の帯で時計とお知らせが切れないこと、「最後の発表」が空白の位置で折り返すこと)。
- [ ] 既存の 3 定義で、前の段と同じ 5 画面の比較 (origin/main と bounding rect ±1px)。差が出たのが `.ep-rows dd` 以外ならば報告。
- [ ] 指摘は重大度つきで報告。
