# レイアウト段 B (右上のサブの地図) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** JDQ の右上の「最近の地震」の枠のように、表示中の地震 (`currentGroup()`) へ寄った 2 枚目の地図 (サブの地図) を、試験定義 trial に置く。地震を出し続ける時間 (保持時間) は最大震度で変え、定義ファイルで変えられるようにする。

**Architecture:** `JapanMap` に設定 (別枠・手の操作・画面外の矢印・帯の見張り・常に細かく描く) を足して 2 つ目を作れるようにする。塗り分け (`paintMap`) に描き先を渡す。保持の判断は純粋な関数 (`hold.ts`)。サブの地図の描き直しは、表示する地震か保持の状態が変わったときだけ。左の地図と既存の 3 定義の動きは変えない。

**Tech Stack:** TypeScript、`node --test`、SVG。

**Spec:** 2026-10-06 の会話での合意:
- サブの地図は `currentGroup()` の地震を映す (EEW 中は予想、震度速報は区域の札、確定は観測点の点)。履歴の選択・巡回にも追従する。
- 保持時間 (最後の情報からの時間): 最大震度 2 以下 1 分、3〜4 3 分、5弱・5強 10 分、6弱以上 15 分。最大震度が分からないとき 3 分。定義ファイルで変えられること。
- 保持時間の間ははっきり描く。過ぎたら同じ地震を薄く描いたまま残す (空にも日本全体にもしない)。
- 「警報・注意報」ボタン (calmSince) で左の地図を平時に戻しても、サブの地図はそのまま (上の規則どおり)。
- サブの地図は揺れた範囲が広くても常に区域と観測点で細かく描く。
- 左の地図の戻り方 (web/src/priority.ts の settleMs) は変えない (配信の native も同じ値を写しているため)。
- 調査メモ: 2026-10-06 の段 B の下調べ (このファイルの「背景」)。

## 背景 (下調べで分かったこと)

- 左の地図の塗り分けは view.ts の `paintMap(g)` だけが書き、`renderDetail()` が `paintMap(g && relatedQuake(g))` で呼ぶ。サブの地図に要るデータの入力は左と同じ。
- map.ts のコンストラクタは `<g id="map-base">` を作り、別枠は `<use href="#map-base">` で複製する。2 枚目でも同じ id を作ると、別枠が違う地図を映しうる。
- map.ts:215-216 の帯の ResizeObserver は、横向きのときの天気の札の置き直し用 (サブには要らない)。
- `setTarget()` は指数的に近づける動き (毎フレーム applyView)。サブは一度に移す方が軽い。
- `applyView()` の `zoomed` の判定 (`v.h < DETAIL_MAX_H`) で区域・点を出すかが決まる。

## Global Constraints

- web のコマンドは `web/` で: `npm test` / `npm run typecheck` / `npm run build`。
- 既存の 3 定義 (landscape / regular / compact) の見た目と、左の地図の動きは変えない。
- 日本語の conventional commit、署名なし。push とマージは親。
- 余計な抽象化をしない。サブの地図の新しい部品は SHIPPED_PARTS に足さない (省いてよい部品)。

## Review Focus

1. 2 枚目の地図を作っても、左の地図の南西諸島・小笠原の別枠が左の地図を映し続けること (id の重複)。→ Task 1 のテスト + Task 5 の目視。
2. サブの地図が隠し置き場にいる間 (trial 以外の定義) に、毎 tick の重い処理が走らないこと。→ Task 4 の `renderSubMap` は署名が同じなら何もしない。Task 5 で regular の性能が変わらないことを見る。
3. 定義ファイルの `subMap` が壊れているとき、ファイル全体を捨てて組み込みに戻ること (既存の振る舞い) と、正しい値なら保持時間が変わること。→ Task 2 のテスト。
4. 保持時間の境目 (ちょうど 60 秒、震度 2 と 3、4 と 5弱、5強 と 6弱) で正しい側に入ること。→ Task 2 のテスト。
5. 地震が 1 つも無いとき・取り消されたとき・震源が無い EEW のとき、サブの地図が空 (塗りなし・印なし・日本全体) になり、エラーを出さないこと。→ Task 2 のテスト (null を返す) と Task 5 の目視。

## 並列

- Task 1 (map.ts) と Task 2 (hold.ts・layout.ts・layout-dom.ts・layout.json) は触るファイルが重ならない → 並列。
- Task 3 は無し (paintMap の変更は Task 4 に含める)。
- Task 4 (統合) は 1・2 のあと。Task 5 (Opus の検証)、Task 6 (文書) はその後。

---

### Task 1: JapanMap を 2 つ作れるようにする (設定・id・一度に移すカメラ)

**Files:**
- Modify: `web/src/map.ts`
- Test: `web/src/map-options.test.ts` (新規。純粋な部分だけ)

**Interfaces:**
- Produces:

```ts
export interface MapOptions {
  /** 南西諸島・小笠原の別枠 (既定 true) */
  insets?: boolean;
  /** 手でのパン・ズーム (既定 true) */
  panZoom?: boolean;
  /** 画面外の地震の矢印 (既定 true) */
  offscreen?: boolean;
  /** 横向きの帯の大きさを見張って天気の札を置き直す (既定 true) */
  bandWatch?: boolean;
  /** 表示範囲の大きさによらず、区域と観測点で細かく描く (既定 false) */
  alwaysDetail?: boolean;
}
constructor(container: HTMLElement, opts?: MapOptions)
/** 自動カメラの目標へ、動きを付けずに一度に移す。null は日本全体 */
jumpTo(box: Box | null): void
/** 下地の <g> の id: 1 つ目は "map-base" (いまと同じ)、2 つ目からは "map-base-2"、"map-base-3"… */
export function baseId(n: number): string
```

仕様:
- `opts` を省くと、いまとまったく同じ動き (既存の呼び出し `new JapanMap($("#map"))` はそのまま)。
- `insets: false` なら別枠の div を作らない。`offscreen: false` なら矢印の層を作らず、`renderOffscreen` の類は何もしない。`panZoom: false` なら `installPanZoom()` を呼ばない。`bandWatch: false` なら map.ts:215-216 の帯の ResizeObserver を作らない。
- `alwaysDetail: true` なら `applyView()` の `zoomed` を `this.areas.size > 0` だけで決める。
- 下地の id はモジュール内の連番で `baseId(n)` から。別枠の `<use>` の href も同じ id にする。
- `jumpTo(box)`: `this.target` と `this.view` を目標にして `applyView()` を 1 回呼ぶ (アニメーションしない)。`userMoved` は見ない (サブは手で動かさない)。

- [ ] **Step 1: 失敗するテストを書く** — `map-options.test.ts`: `baseId(1) === "map-base"`、`baseId(2) === "map-base-2"`、`baseId(3) === "map-base-3"`。(map.ts を node から import できない場合は、`baseId` を小さな純粋モジュール web/src/map-ids.ts に置き、map.ts から使う。どちらにしたか報告)
- [ ] **Step 2: 落ちることを確かめる** — Run: `cd web && npm test` / Expected: FAIL
- [ ] **Step 3: 実装する** (上の仕様)
- [ ] **Step 4: 通ることを確かめる** — Run: `cd web && npm test && npm run typecheck && npm run build` / Expected: PASS
- [ ] **Step 5: Commit** — `git commit -m "feat: 地図に設定 (別枠・手の操作・矢印・帯の見張り・常に細かく) と一度に移すカメラを足し、下地の id を地図ごとに分ける"`

---

### Task 2: 保持時間の判断 (純粋な関数) と定義ファイルの設定

**Files:**
- Create: `web/src/hold.ts`、`web/src/hold.test.ts`
- Modify: `web/src/layout.ts` (型・`checkLayouts` の最上位のキー・組み込みの既定値)、`web/src/layout.json` (最上位に `subMap`)、`web/src/layout.test.ts`、`web/src/layout-dom.ts` (`loadLayoutFile` で設定も入れ替える)

**Interfaces:**
- Produces:

```ts
// layout.ts
export interface HoldRule { /** この震度以下に当てはまる (無ければ残り全部) */ maxScale?: number; sec: number }
export interface SubMapConfig {
  /** 上から順に、最大震度が maxScale 以下の最初の規則。最後は maxScale なし */
  hold: HoldRule[];
  /** 最大震度が分からない (0 以下) とき */
  unknownSec: number;
  /** 保持時間を過ぎたあとの濃さ (0〜1) */
  fadedAlpha: number;
}
/** 組み込みの設定 (layout.json の subMap) */
export const SUB_MAP: SubMapConfig;

// layout-dom.ts
/** いま使う設定 (はじめは組み込み。定義ファイルが読めて正しければそれ) */
export function subMapConfig(): SubMapConfig;

// hold.ts
export function holdMs(scale: number, cfg: SubMapConfig): number;
export interface SubMapState { key: string; faded: boolean }
/** サブの地図に何を出すか。g が無い・地震でない (kind が quake でも eew でもない) なら null。
 *  保持時間内なら faded=false、過ぎたら faded=true (同じ地震を薄く) */
export function subMapState(now: number, g: { key: string; kind: string; updatedAt: number } | undefined, scale: number, cfg: SubMapConfig): SubMapState | null;
```

layout.json の最上位に足す値 (組み込みの既定):

```json
"subMap": {
  "note": "右上のサブの地図 (試験定義 trial)。最後の情報から hold の時間ははっきり、過ぎたら fadedAlpha の濃さで残す。震度は 10=1, 20=2, 30=3, 40=4, 45=5弱, 50=5強, 55=6弱, 60=6強, 70=7",
  "hold": [
    {"maxScale": 20, "sec": 60},
    {"maxScale": 40, "sec": 180},
    {"maxScale": 50, "sec": 600},
    {"sec": 900}
  ],
  "unknownSec": 180,
  "fadedAlpha": 0.4
}
```

仕様:
- `checkLayouts`: 最上位のキーに `subMap` を許す (任意。無ければ組み込みの既定を使う)。中身の検査: キーは `hold`・`unknownSec`・`fadedAlpha` (と `note`)。`hold` は空でない配列、各要素のキーは `maxScale`・`sec` (と `note`)、`sec` は 0 より大きい数、`maxScale` は数、最後の要素だけ `maxScale` なし、`maxScale` は上から増える。`unknownSec` は 0 より大きい数。`fadedAlpha` は 0〜1 の数。文言は既存に合わせる (例 `subMap.hold[1]: sec は 0 より大きい数`)。
- `holdMs`: scale が 0 以下なら `unknownSec * 1000`。それ以外は上から最初の `scale <= maxScale` (maxScale なしは常に当てはまる) の `sec * 1000`。
- `subMapState`: `g` が undefined、または kind が "quake" でも "eew" でもないなら null。`now - g.updatedAt <= holdMs(scale, cfg)` なら faded=false、超えたら faded=true。key は g.key。
- `loadLayoutFile`: 定義ファイルが正しければ、`layouts` と一緒に `subMap` (無ければ組み込み) も入れ替える。

- [ ] **Step 1: 失敗するテストを書く**
  - hold.test.ts (組み込みの `SUB_MAP` を使う): `holdMs(10)`・`holdMs(20)` = 60000、`holdMs(30)`・`holdMs(40)` = 180000、`holdMs(45)`・`holdMs(50)` = 600000、`holdMs(55)`・`holdMs(60)`・`holdMs(70)` = 900000、`holdMs(-1)`・`holdMs(0)` = 180000。`subMapState`: updatedAt から 60000 ちょうど (震度 20) は faded=false、60001 は faded=true。g undefined → null、kind "tsunami" → null。
  - layout.test.ts の `the check names what is wrong in a layout file` に: subMap なしでも通る / 組み込みの subMap が通る / `hold: []` → 誤り / `hold[1].sec = 0` → 誤り / 最後の要素に maxScale → 誤り / maxScale が減る → 誤り / `fadedAlpha: 1.5` → 誤り / `subMap.extra` → 知らないキー。`SUB_MAP` が layout.json の subMap と同じ。
- [ ] **Step 2: 落ちることを確かめる** — Run: `cd web && npm test` / Expected: FAIL
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる** — Run: `cd web && npm test && npm run typecheck && npm run build`、リポジトリ直下で `cargo test -p eq-server layout` / Expected: PASS
- [ ] **Step 5: Commit** — `git commit -m "feat: サブの地図の保持時間を最大震度で決める関数と、定義ファイルの設定 (subMap) を足す"`

---

### Task 3: (欠番。paintMap の変更は Task 4 に含める)

---

### Task 4: サブの地図を足し、trial の右上に置く

**Files:**
- Modify: `web/public/index.html` (要素)、`web/public/style.css` (枠)、`web/src/dom.ts` (`subMap`)、`web/src/view.ts` (`paintMap` に描き先)、`web/src/main.ts` (読み込み・tick)、`web/src/layout-dom.ts` (SLOTS)、`web/src/layout.json` (trial)、`web/src/layout.test.ts` (trial の部品の集合)
- Create: `web/src/sub-map.ts`

**Interfaces:**
- Consumes: Task 1 の `MapOptions`・`jumpTo`、Task 2 の `subMapConfig()`・`holdMs`・`subMapState`。既存の `currentGroup()`・`relatedQuake()`・`geoOf()`・`shakenGeo()`・`groupScale()` (quakes.ts)、`pad`・`union`・`pointBox` (camera.ts)、`project` (map.ts で使っている投影)。
- Produces: `export function paintMap(target: JapanMap, g: Group | undefined): void` (view.ts。いまの paintMap に描き先を足して export)、`export function renderSubMap(now: number): void` (sub-map.ts)、`export const subMap: JapanMap` (dom.ts)。

仕様:
- index.html: `#map` の直後に `<section id="map-sub" class="map-wrap map-sub" aria-label="最近の地震"></section>`。
- dom.ts: `export const subMap = new JapanMap($("#map-sub"), { insets: false, panZoom: false, offscreen: false, bandWatch: false, alwaysDetail: true })`。
- main.ts の初期化の Promise.all に、サブの地図の `load("japan.geojson")`・`loadAreas("areas.geojson").catch(() => {})`・`loadNeighbors("neighbors.geojson").catch(() => {})` を足す (津波・警報・天気は読まない)。tick の `renderBanner(now)` の近くで `renderSubMap(now)`。
- view.ts: `paintMap(target, g)` にして、中の `map.` を `target.` に。`renderDetail` は `paintMap(map, g && relatedQuake(g))`。
- sub-map.ts の `renderSubMap(now)`:
  1. `g = currentGroup()`、`q = g && relatedQuake(g)` (左と同じ選び方)、`state = subMapState(now, q, q ? groupScale(q) : -1, subMapConfig())`。
  2. 署名 `state ? state.key + "|" + q.updatedAt + "|" + state.faded : ""` が前回と同じなら何もしない (毎 tick の処理はここまで)。
  3. state が null: `paintMap(subMap, undefined)`、`subMap.setEpicenters([])`、`subMap.setFade(1)`、`subMap.jumpTo(null)`。
  4. それ以外: `paintMap(subMap, q)`、震央があれば 1 点だけ `subMap.setEpicenters([{ key: q.key, lat, lon, label: null, primary: true, scale: groupScale(q) }])` (無ければ空)、`subMap.setFade(state.faded ? cfg.fadedAlpha : 1)`、カメラは揺れた範囲 (`subMap.areaBox(shakenGeo(q).areas) ?? subMap.prefBox(shakenGeo(q).prefs)`) と震央の小さな箱 (`pointBox(x, y, 80)`、x,y は震央の投影) の `union` を `pad` して `subMap.jumpTo(...)`。どちらも無ければ `jumpTo(null)`。
  5. 定義が切り替わって大きさが変わったときの置き直しは、JapanMap 自身の ResizeObserver に任せる (applyView が縦横比を合わせる)。ただし jumpTo の目標の縦横比は、見えた時点の大きさで合わせ直す必要があるので、`renderSubMap` は前回の署名に加えて `#map-sub` の幅と高さ (整数) も署名に入れる。
- layout-dom.ts の SLOTS: `"map-sub": "#map-sub"`。
- layout.json の trial: 右列を `settings (auto) / map-sub (fill) / detail (auto) / eew-panel (auto) / history-head (auto) / history (fill) / credit (auto)` にする (詳細は寄り図の下に戻す)。note の「右上の寄り図の場所はいまは地震の詳細で代用」を「右上はサブの地図 (最近の地震)」に直す。
- layout.test.ts: trial の部品の集合に `map-sub` を足す。
- style.css: `.map-sub { border-bottom: 1px solid var(--line); min-height: 160px; }` 程度 (trial 以外では隠し置き場なので見えない)。

- [ ] **Step 1: 失敗するテストを書く** (trial の部品の集合に map-sub)
- [ ] **Step 2: 落ちることを確かめる** — Run: `cd web && npm test` / Expected: FAIL
- [ ] **Step 3: index.html・SLOTS・layout.json・dom.ts・view.ts を変える**
- [ ] **Step 4: sub-map.ts・main.ts・style.css を変える**
- [ ] **Step 5: 通ることを確かめる** — Run: `cd web && npm test && npm run typecheck && npm run build` / Expected: PASS
- [ ] **Step 6: Commit** — 2 つに分けてよい: `feat: 塗り分けに描き先を渡し、サブの地図 (最近の地震) を足す` / `feat: 試験定義 trial の右上にサブの地図を置く`

---

### Task 5: 検証 (Opus)

- [ ] `npm test` / `npm run typecheck` / `npm run build` / `cargo test -p eq-server`
- [ ] 既存 3 定義の 5 画面の比較 (origin/main と bounding rect ±1px、`?demo=standard` と平時)。左の地図の別枠が左を映していること (Review Focus 1)。
- [ ] `?layout=trial&demo=standard` (1440×900・1280×720): EEW → 震度速報 → 確定の流れで、サブの地図が予想 → 区域の札 → 観測点の点に変わり、震源へ寄ること。左の地図の動きと独立していること。デモを ×8 で進めて保持時間 (3 分) を過ぎたら薄くなること。スクリーンショット。
- [ ] `?layout=trial&demo=nationwide` など揺れた範囲の広い場面で、サブの地図が区域・観測点で描くこと (alwaysDetail)。
- [ ] `?layout=trial` (デモなし・地震なし) でサブの地図が日本全体で空、エラーなし。
- [ ] regular で `#map-sub` が隠し置き場にあり、tick ごとの処理時間が main と大差ないこと (Performance の簡単な計測でよい)。
- [ ] Opus のコードレビュー (差分全体)。

### Task 6: 文書

- docs/ui-spec/layout-system.html の「定義ファイル」の表に `subMap` (hold・unknownSec・fadedAlpha) を足す。docs/ui-layout-backlog.md の「4.」のサブの地図を済みへ、残り (サブの地図に重ねる詳細・凡例・出典、P 波・S 波の円、native への持ち込み) を残す。
- Commit: `docs: サブの地図と保持時間の設定 (subMap) を記す`
