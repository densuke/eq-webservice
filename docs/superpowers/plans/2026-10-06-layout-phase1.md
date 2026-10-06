# レイアウト第 1 段 (部品の省略・余白の書式・試験定義・EEW の常設パネル) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** レイアウトの定義ファイル (web/src/layout.json) で、部品を省ける・余白と場所取りを書ける・URL で試験用の定義を選べるようにし、緊急地震速報 (EEW) を平時も出す常設パネルの部品を足す。既存の 3 定義 (landscape / regular / compact) の見た目は変えない。

**Architecture:** 定義の型・選び方・確かめ方は DOM に触らない web/src/layout.ts、並べるのは web/src/layout-dom.ts。EEW パネルは「見せ方を決める純粋関数」(新規 web/src/eew-panel.ts) と「描く関数」(web/src/view.ts) に分ける。新しい部品は既存 3 定義に置かず、手動でだけ選ばれる試験定義 "trial" に置く。

**Tech Stack:** TypeScript (tsc 7, esbuild)、テストは `node --test --experimental-strip-types`、Rust (eq-server は layout.json を include_str! で同梱)。

**Spec:** docs/ui-layout-backlog.md の「2. レイアウトの定義の書式を育てる」、docs/ui-spec/layout-system.html の「定義ファイル (段階 0.5)」、2026-10-06 の会話での合意 (EEW は案 (a) = 平時も枠を出し「発表なし」を表示)。

## Global Constraints

- web のコマンドは `web/` で実行する: `npm test` / `npm run typecheck` / `npm run build`。
- コードのコメントと文言は既存に合わせて日本語、です・ます調でなく常体。絵文字は使わない。
- 既存の 3 定義 (landscape / regular / compact) の要素の位置・大きさは変えない (±1px)。
- 定義ファイルの `version` は 1 のまま (足すキーはすべて任意で、古い定義ファイルもそのまま通る)。
- コミットは日本語の conventional commit (例: `feat: 定義で部品を省けるようにする`)。Co-Authored-By などの署名は付けない。
- 1 Task ごとに、テストが通った状態でコミットする。push とマージは親 (オーケストレータ) が行う。
- `checkLayouts` のエラー文は既存の言い回し (`${at}: 知らない部品 ...` など) に合わせる。

## Review Focus

1. 定義の切り替え (画面を回す・幅を変える) で、前の定義に置かれ次の定義に置かれない部品が文書から外れないこと。`#eew-banner` などを `document.querySelector` で探すコードと、map.ts:216 の ResizeObserver が対象を失う。→ Task 1 の隠し置き場で防ぐ。確認は Task 5 の実測 (Playwright)。
2. 場所取り (minHeight) を付けた積みが、中身が全部 hidden のときも高さを保つこと。style.css の `.ld-stack:not(:has(> :not([hidden]))) { display: none }` に負けない。→ Task 2 で inline の `display: flex` を付ける。確認は Task 5 で compact 390×844、別枠を隠した状態。
3. `?layout=` に存在しない名前・空文字が来たとき、自動の選び方に戻ること。→ Task 3 のテスト。
4. 試験定義 (manual) が画面の大きさによる自動選択で選ばれないこと (どれにも合わないときの「先頭」にも選ばれない)。→ Task 3 のテスト。
5. EEW パネルの時刻が、テストを動かす環境のタイムゾーンによらず日本時間で出ること。震源・規模・深さが null のときに "null" や "NaN" を出さないこと。→ Task 4 のテスト。

---

## ファイルの分担

- `web/src/layout.ts` — 型・`pickLayout`・`chooseLayout` (新規)・`slotsOf` (新規。テストから移す)・`checkLayouts`。DOM に触らない。
- `web/src/layout-dom.ts` — `SLOTS`・`BOXES`・`applyLayout`。隠し置き場と、積みの余白の反映。
- `web/src/layout.json` — 組み込みの定義。compact に余白を書き、試験定義 "trial" を足す。
- `web/src/layout.test.ts` — 上の 3 つのテスト。
- `web/public/style.css` — compact の補い 3 行 (331〜333 行目) を消す。EEW パネルの見た目を足す。
- `web/src/eew-panel.ts` (新規) / `web/src/eew-panel.test.ts` (新規) — EEW パネルの見せ方 (純粋関数)。
- `web/src/view.ts` — `renderEewPanel(now)`。`web/src/main.ts` の `tick()` から呼ぶ。
- `web/public/index.html` — `#eew-panel` の要素。

## 実行の順と並列

- 並列 A (worktree 1): Task 1 → Task 2 → Task 3 (同じ 4 ファイルを触るので直列)。
- 並列 B (worktree 2): Task 4 (新規ファイルだけ)。
- A と B を親がマージしたあと: Task 5 (統合)、Task 6 (Opus の検証)、Task 7 (文書)。

---

### Task 1: 定義で部品を省けるようにする

**Files:**
- Modify: `web/src/layout.ts` (`checkLayouts` の末尾の「1 回ずつ」の検査、`slotsOf` を追加)
- Modify: `web/src/layout-dom.ts` (`applyLayout`)
- Test: `web/src/layout.test.ts`

**Interfaces:**
- Produces: `export function slotsOf(node: LayoutNode): string[]` (layout.ts) — 定義の中に置かれた部品の名前を、重ね物と入れ子の積みを含めて出てきた順に返す (重複もそのまま)。いま layout.test.ts にある `slots()` と同じ中身。
- Produces: `export const REQUIRED_SLOTS: readonly string[] = ["main"]` (layout.ts) — 省けない部品。
- Produces: 置かれなかった部品の要素は `<div class="ld-unused" hidden>` (body の直下) に入る。

仕様:
- 各定義で、部品は高々 1 回。2 回以上は誤り (いまの文言 `部品「x」を n 回置いている` のまま)。
- `REQUIRED_SLOTS` のうち `slots` 引数にあるもので置かれていないものは誤り (文言 `部品「main」を置いていない`)。それ以外の部品は省いてよい。
- `applyLayout` は、新しい定義を並べたあと、その定義に無い部品 (`Object.keys(SLOTS)` のうち `slotsOf(next.root)` に無いもの) の要素を、隠し置き場へ移す。隠し置き場は並べ直しのたびに作り、`made` に入れる (前の置き場は既存の流れで消える。要素は先に新しい置き場へ移っているので外れない)。省いた部品の `data-variant` は消す (`part(slot)` を通す)。

- [ ] **Step 1: 失敗するテストを書く**

layout.test.ts の変更:
- ファイル内の `slots()` を消し、`slotsOf` を layout.ts から import して使う。
- 各定義のテスト `${l.name}: every part exists and is placed exactly once` を 2 本に分ける:
  - `${l.name}: every part exists, is placed at most once, and main is placed` — `slotsOf(l.root)` に重複が無い / 全部 `SLOTS` にある / `REQUIRED_SLOTS` を全部含む。
  - 出荷の 3 定義が既存の部品を全部置いている保証 (定義から部品を落としたことに気づくため)。部品名は列挙して固定する:

```ts
/** 出荷している自動の定義が置く部品 (省いたら見た目が変わる。新しい部品はここに足さない限り省いてよい) */
const SHIPPED_PARTS = ["topbar", "banners", "main", "settings", "detail", "history-head", "history", "notice", "credit",
  "inset", "ogasawara", "caption", "countdown", "legend", "clock", "toast", "hint"];
for (const name of ["landscape", "regular", "compact"]) {
  test(`${name}: places every shipped part`, () => {
    const l = LAYOUTS.find((x) => x.name === name)!;
    assert.deepEqual(SHIPPED_PARTS.filter((s) => !slotsOf(l.root).includes(s)), []);
  });
}
```

- `the check names what is wrong in a layout file` の中:
  - `compact\): 部品「banners」を置いていない` の行を、省いても誤りにならないことの確認に変える: `assert.equal(broken((d) => d.layouts[2].root.children.splice(1, 1)), "")`。
  - main を省くと誤り: `assert.match(broken((d) => (d.layouts[1].root.children[2].children[0] = { slot: "notice" })), /regular\): 部品「main」を置いていない/)` (main の代わりに置いた notice は 2 回目になるので、`「notice」を 2 回置いている` も一緒に出てよい)。
  - `topbarr` の行の期待値 `/知らない部品 "topbarr"[\s\S]*「topbar」を置いていない/` は `/知らない部品 "topbarr"/` に変える (topbar は省けるようになった)。
- `slotsOf` のテスト: `slotsOf({ slot: "main", overlays: { "top-left": { flow: "column", items: ["inset", { slot: "banners", variant: "compact" }, { flow: "row", items: ["legend"] }] } } })` が `["main", "inset", "banners", "legend"]`。

- [ ] **Step 2: 落ちることを確かめる**

Run: `cd web && npm test`
Expected: FAIL (`slotsOf` / `REQUIRED_SLOTS` が無い)

- [ ] **Step 3: layout.ts に `slotsOf`・`REQUIRED_SLOTS` を足し、`checkLayouts` の末尾の検査を上の仕様に変える**

- [ ] **Step 4: layout-dom.ts の `applyLayout` に隠し置き場を足す**

`missing()` の中の部品名の集め方も `slotsOf` に置き換えてよい (同じ結果になる)。

- [ ] **Step 5: 通ることを確かめる**

Run: `cd web && npm test && npm run typecheck`
Expected: PASS、型エラーなし

- [ ] **Step 6: Commit**

```bash
git add web/src/layout.ts web/src/layout-dom.ts web/src/layout.test.ts
git commit -m "feat: レイアウトの定義で部品を省けるようにする (main は必須、省いた部品は隠し置き場へ)"
```

---

### Task 2: 積み (Stack) に余白と場所取りを書けるようにし、compact の CSS の補いを定義へ移す

**Files:**
- Modify: `web/src/layout.ts` (`Stack` 型・`checkLayouts` の `stack()`)
- Modify: `web/src/layout-dom.ts` (`stack()`)
- Modify: `web/src/layout.json` (compact)
- Modify: `web/public/style.css:331-333` (3 行を消す)
- Test: `web/src/layout.test.ts`

**Interfaces:**
- Consumes: Task 1 の `slotsOf`。
- Produces: `Stack` に任意のキー `gap?: string; pad?: string; minHeight?: string;` (隅の積みと、入れ子の積みの両方)。

仕様:
- 値は CSS の長さだけ: 正規表現 `/^(?:0|\d+(?:\.\d+)?(?:px|em|rem|%|vw|vh|svh|dvh))$/`。`pad` は空白区切りで 1〜4 個 (例 `"6px"`、`"4px 8px"`)。合わなければ `${at}: gap "..." は使えない` (キー名を入れる)。
- 知らないキーの検査の許可リストに 3 つを足す (`flow`, `items`, `gap`, `pad`, `minHeight`)。
- layout-dom.ts の `stack()`: `gap` → `box.style.gap`、`pad` → `box.style.padding`、`minHeight` → `box.style.minHeight` と、あわせて `box.style.display = "flex"` (中身が全部 hidden でも消えずに場所を取るため。style.css の `.ld-stack:not(:has(> :not([hidden]))) { display: none }` より inline が勝つ)。
- layout.json の compact を次のように書き足す (いま style.css の 331〜333 行目が補っている値):
  - `bottom-right` の積み: `"pad": "6px"`
  - `top-left` の積み: `"gap": "8px"`
  - `top-left` の中の `{"flow": "row", "items": ["inset", "caption"]}`: `"gap": "4px", "minHeight": "140px"`
- style.css の次の 3 行を消す (328 行目の toast の margin は消さない):
  - `body[data-layout="compact"] .ld-bottom-right { padding: 6px; }`
  - `body[data-layout="compact"] .ld-top-left { gap: 8px; }`
  - `body[data-layout="compact"] .ld-top-left > .ld-stack { display: flex; gap: 4px; min-height: 140px; }`

- [ ] **Step 1: 失敗するテストを書く**

`the check names what is wrong in a layout file` に足す (compact は `d.layouts[2]`、その `root.children[2].children[0]` が main):

```ts
const corner = (d: any) => d.layouts[2].root.children[2].children[0].overlays["top-left"];
// 隅の積みと入れ子の積みの両方で使える
assert.equal(broken((d) => Object.assign(corner(d), { gap: "8px", pad: "4px 8px" })), "");
assert.equal(broken((d) => Object.assign(corner(d).items[0], { gap: "4px", minHeight: "140px" })), "");
for (const bad of ["big", "8", "1px; color: red", "", "1px 2px 3px 4px 5px"]) {
  assert.match(broken((d) => (corner(d).pad = bad)), /pad .* は使えない/, bad);
}
assert.match(broken((d) => (corner(d).items[0].minHeight = "fill")), /minHeight .* は使えない/);
assert.match(broken((d) => (corner(d).margin = "4px")), /知らないキー「margin」/);
```

- [ ] **Step 2: 落ちることを確かめる**

Run: `cd web && npm test`
Expected: FAIL (`知らないキー「gap」` などが出る)

- [ ] **Step 3: 型・確かめ・`stack()` を実装し、layout.json と style.css を上のとおり変える**

- [ ] **Step 4: 通ることを確かめる**

Run: `cd web && npm test && npm run typecheck && npm run build`
Expected: PASS (出荷の layout.json が確かめを通り、組み込みと同じ)

- [ ] **Step 5: Commit**

```bash
git add web/src/layout.ts web/src/layout-dom.ts web/src/layout.json web/src/layout.test.ts web/public/style.css
git commit -m "feat: 重ね物の積みに余白と場所取りを書けるようにし、スマホ縦の CSS の補いを定義へ移す"
```

---

### Task 3: URL (?layout=名前) で定義を選べるようにし、手動専用の試験定義の印を足す

**Files:**
- Modify: `web/src/layout.ts` (`Layout` 型・`pickLayout`・`chooseLayout` 新規・`checkLayouts`)
- Modify: `web/src/layout-dom.ts` (`applyLayout`)
- Test: `web/src/layout.test.ts`

**Interfaces:**
- Produces: `Layout` に任意のキー `manual?: true` — 画面の大きさによる自動選択から外す (URL で名前を指したときだけ使う)。
- Produces: `export function chooseLayout(layouts: readonly Layout[], width: number, height: number, forced: string | null): Layout` — `forced` と同じ名前の定義があればそれ (manual でも)。無い・空・null なら `pickLayout`。
- `pickLayout` は `manual` の定義を飛ばす。どれにも合わないときは「manual でない先頭」。

仕様:
- `checkLayouts`: 定義のキーの許可リストに `manual` を足す。値は `true` だけ (`${name}: manual は true だけ`)。全部の定義が manual なのは誤り (`manual でない定義が 1 つも無い`)。
- `applyLayout`: `chooseLayout(layouts, innerWidth, innerHeight, new URLSearchParams(location.search).get("layout"))`。URL の名前は読み込み時に 1 回読めばよい (モジュールの先頭で const)。
- `?layout=` は既存の URL の引数 (`demo`・`broadcast`・`sink`・`audio`) と重ならない。

- [ ] **Step 1: 失敗するテストを書く**

```ts
test("a manual layout is chosen only by name (?layout=)", () => {
  const ls = [
    { name: "trial", manual: true, root: { slot: "main" } },
    { name: "wide", when: { minWidth: 801 }, root: { slot: "main" } },
    { name: "narrow", root: { slot: "main" } },
  ] as Layout[];
  assert.equal(pickLayout(ls, 1440, 900).name, "wide");
  assert.equal(pickLayout(ls, 390, 844).name, "narrow");
  // どれにも合わないときも manual は選ばない
  assert.equal(pickLayout([ls[0], ls[1]], 390, 844).name, "wide");
  assert.equal(chooseLayout(ls, 390, 844, "trial").name, "trial");
  assert.equal(chooseLayout(ls, 390, 844, "wide").name, "wide");
  for (const f of [null, "", "nope"]) assert.equal(chooseLayout(ls, 390, 844, f).name, "narrow", String(f));
});
```

`the check names what is wrong in a layout file` に足す:

```ts
assert.equal(broken((d) => (d.layouts[1].manual = true)), "");
assert.match(broken((d) => (d.layouts[1].manual = "yes")), /manual は true だけ/);
assert.match(broken((d) => d.layouts.forEach((l: any) => (l.manual = true))), /manual でない定義が 1 つも無い/);
```

- [ ] **Step 2: 落ちることを確かめる**

Run: `cd web && npm test`
Expected: FAIL (`chooseLayout` が無い)

- [ ] **Step 3: 実装する**

- [ ] **Step 4: 通ることを確かめる**

Run: `cd web && npm test && npm run typecheck`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add web/src/layout.ts web/src/layout-dom.ts web/src/layout.test.ts
git commit -m "feat: ?layout=名前 で定義を選べるようにし、自動では選ばない試験定義 (manual) を書けるようにする"
```

---

### Task 4: EEW の常設パネルの見せ方 (純粋関数)  ※並列 B

**Files:**
- Create: `web/src/eew-panel.ts`
- Test: `web/src/eew-panel.test.ts`

**Interfaces:**
- Consumes: `EewEvent` (web/src/types.ts)、`scaleLabel(s: Scale): string` (web/src/scale.ts)。
- Produces:

```ts
export interface EewPanelRow { label: string; value: string }
export interface EewPanelView {
  /** "active" = 発表中の EEW がある / "none" = 無い */
  state: "active" | "none";
  /** 色分け用。active の 1 件目が警報なら true */
  warning: boolean;
  title: string;
  rows: EewPanelRow[];
  /** 1 件目のほかに発表中の件数 (0 なら出さない) */
  more: number;
  /** 行の代わりに出す一文 (none のとき) */
  message: string | null;
}
/** active は優先順 (揺れの大きい順) に並んだ発表中の EEW (並べ替えは呼び出し側)。last は取り消しでない最後の EEW (無ければ null) */
export function eewPanelView(active: readonly EewEvent[], last: EewEvent | null): EewPanelView;
```

仕様 (文言はこのとおり):
- active が 1 件以上: 1 件目 `e` を出す。
  - `title`: `${e.test ? "【テスト】" : ""}緊急地震速報 (${e.warning ? "警報" : "予報"})`
  - `rows` (この順):
    - `{ label: "震源", value: e.hypocenter?.name || "調査中" }`
    - `{ label: "発生", value: 日本時間の "HH:MM:SS" (origin_time_ms から。null なら "—") }`
    - `{ label: "規模", value: magnitude が数なら "M" + 小数 1 桁 (例 "M5.3")、null なら "—" }`
    - `{ label: "深さ", value: depth_km が数なら "約" + 整数 + "km" (例 "約50km")、null なら "—" }`
    - `{ label: "予測最大震度", value: scaleLabel(e.max_scale) }`
    - `{ label: "報", value: "第" + e.serial + "報" }`
  - `more`: `active.length - 1`、`message`: null、`warning`: `e.warning`、`state`: "active"。
- active が空: `state` "none"、`warning` false、`title` "緊急地震速報"、`more` 0、`message` "現在、発表はありません"。
  - `last` があれば `rows` に 1 行: `{ label: "最後の発表", value: "MM/DD HH:MM 震源名 (警報|予報・予測最大震度X)" }` (日時は last の origin_time_ms、null なら received_at_ms。日本時間。震源名が無ければ "震源不明")。無ければ `rows` は空。
- 時刻は必ず日本時間: `Intl.DateTimeFormat("ja-JP", { timeZone: "Asia/Tokyo", ... })` か、既存の書き方 (web/src/view.ts:77 の `toLocaleTimeString("ja-JP", { timeZone: "Asia/Tokyo" })`) に合わせる。ゼロ埋め 2 桁。
- DOM に触らない。HTML の組み立てもしない (エスケープは描く側の仕事)。

- [ ] **Step 1: 失敗するテストを書く**

テスト用の EEW を作る関数をテストファイルの中に置く (他のテスト、例えば web/src/held.test.ts の作り方を参考にしてよい)。`origin_time_ms` は `Date.UTC(2026, 9, 5, 12, 27, 5)` (= 日本時間 10/05 21:27:05) を使う。テスト:

- `active EEW: shows the top one with Japan time and more count` — 警報・M5.3・深さ 50・第 3 報・震源「千葉県北東部」・max_scale 45 (5弱) の 1 件 + 予報 1 件 → `title` "緊急地震速報 (警報)"、rows の value が順に `["千葉県北東部", "21:27:05", "M5.3", "約50km", scaleLabel(45), "第3報"]`、`more` 1、`warning` true、`message` null。
- `active EEW with unknowns` — hypocenter null・origin_time_ms null → value が `["調査中", "—", "—", "—", ...]`。`JSON.stringify(view)` に "null" "NaN" "undefined" の文字列が値として入らない (rows の value を検査)。
- `test EEW gets the test mark` — `test: true`・予報 → `title` "【テスト】緊急地震速報 (予報)"、`warning` false。
- `no EEW: says none and shows the last one` — active 空、last = 予報・max_scale 40 → `state` "none"、`message` "現在、発表はありません"、rows `[{ label: "最後の発表", value: "10/05 21:27 千葉県北東部 (予報・予測最大震度" + scaleLabel(40) + ")" }]`。
- `no EEW and no history` — `eewPanelView([], null)` → rows 空、message あり。
- タイムゾーンに依らないこと: テストの実行コマンドに `TZ=UTC` を付けて 1 回、`TZ=America/Los_Angeles` で 1 回流して両方 PASS すること (Step 4)。

- [ ] **Step 2: 落ちることを確かめる**

Run: `cd web && node --test --experimental-strip-types src/eew-panel.test.ts`
Expected: FAIL (モジュールが無い)

- [ ] **Step 3: `eewPanelView` を実装する**

- [ ] **Step 4: 通ることを確かめる**

Run: `cd web && TZ=UTC node --test --experimental-strip-types src/eew-panel.test.ts && TZ=America/Los_Angeles node --test --experimental-strip-types src/eew-panel.test.ts && npm run typecheck`
Expected: PASS (両方)

- [ ] **Step 5: Commit**

```bash
git add web/src/eew-panel.ts web/src/eew-panel.test.ts
git commit -m "feat: 緊急地震速報の常設パネルの見せ方 (発表中は最優先 1 件、平時は発表なしと最後の発表)"
```

---

### Task 5: EEW パネルを画面に足し、試験定義 "trial" に置く  ※A・B のマージ後

**Files:**
- Modify: `web/public/index.html` (要素を足す)
- Modify: `web/src/layout-dom.ts` (`SLOTS` に `"eew-panel": "#eew-panel"`)
- Modify: `web/src/view.ts` (`renderEewPanel`、`renderBanner` と並べ替えを共有)
- Modify: `web/src/main.ts` (`tick()` で `renderBanner(now)` の直後に `renderEewPanel(now)`)
- Modify: `web/src/layout.json` (定義 "trial" を末尾に足す)
- Modify: `web/public/style.css` (パネルの見た目)
- Test: `web/src/layout.test.ts`

**Interfaces:**
- Consumes: Task 1 (省略・隠し置き場)、Task 3 (`manual`・`?layout=`)、Task 4 (`eewPanelView`)。
- Produces: `export function renderEewPanel(now: number): void` (view.ts)。

仕様:
- index.html: `aside.side` の中、`#detail` の直前に `<section id="eew-panel" class="eew-panel" role="status" aria-label="緊急地震速報"></section>`。既存 3 定義には置かないので、起動時に隠し置き場へ移る (見た目は変わらない)。
- view.ts: `renderBanner` の先頭の「activeEews を byPriority で並べる」部分を `function sortedActiveEews(now: number): EewEvent[]` に切り出し、`renderBanner` と `renderEewPanel` の両方で使う。
- 「最後の EEW」: `app.world.store.list()` の `kind === "eew"` のグループから、`latestEew(g).cancelled` でないものを `heldEew(g)` にし、`received_at_ms` が最大のもの (無ければ null)。`activeEews` は使わない (3 分と calmSince で絞られるため)。
- 描画: `eewPanelView(...)` の結果を HTML にする。文字列はすべて `esc()` を通す。前と同じ HTML なら書き換えない (帯と同じく、毎フレームの書き換えを避ける)。`data-state` に state、`classList.toggle("warning", view.warning)`。構造は `<div class="ep-title">` / `<dl class="ep-rows">` (dt=label, dd=value) / `<div class="ep-more">ほか n 件</div>` (more>0 のとき) / `<div class="ep-message">` (message があるとき)。
- 試験定義 "trial": regular と同じ並びをコピーし、`"name": "trial"`、`"manual": true`、`"note"` に「試験中。?layout=trial で選ぶ。帯の代わりに EEW の常設パネルを右列の先頭に置く」と書く。違いは 2 点: ルートの `{"slot": "banners", ...}` を消す / 右列の `settings` の直後に `{"slot": "eew-panel", "size": "auto"}`。
- CSS: パネルは右列の他のパネルと同じ余白・枠線の流儀 (既存の `#detail` のスタイルを参考に)。`[data-state="active"]` は背景色を帯と同じ色 (警報 = `.eew-banner` の背景色、予報 = `#b35900`)、`none` は落ち着いた色。
- layout.test.ts: Task 1 の `SHIPPED_PARTS` には `eew-panel` を足さない (省いてよい部品)。"trial" について 1 本: `chooseLayout(LAYOUTS, 1440, 900, "trial")` が trial で、`slotsOf` に `eew-panel` を含み `banners` を含まない。`pickLayout(LAYOUTS, 1440, 900)` は regular のまま。

- [ ] **Step 1: 失敗するテストを書く** (layout.test.ts の trial の 1 本)
- [ ] **Step 2: 落ちることを確かめる** — Run: `cd web && npm test` / Expected: FAIL (trial が無い、または `eew-panel` が SLOTS に無い)
- [ ] **Step 3: index.html・SLOTS・layout.json を変える**
- [ ] **Step 4: view.ts・main.ts・style.css を変える**
- [ ] **Step 5: 通ることを確かめる** — Run: `cd web && npm test && npm run typecheck && npm run build` / Expected: PASS (`every SLOTS/BOXES selector names an element of index.html` も通る)
- [ ] **Step 6: Commit**

```bash
git add web/public/index.html web/public/style.css web/src/layout-dom.ts web/src/layout.json web/src/layout.test.ts web/src/view.ts web/src/main.ts
git commit -m "feat: 緊急地震速報の常設パネルを足し、試験定義 trial (?layout=trial) に置く"
```

---

### Task 6: 検証 (Opus)

実装はしない。結果を報告する。

- [ ] `cd web && npm test && npm run typecheck && npm run build`、リポジトリ直下で `cargo test -p eq-server layout` (layout.json を同梱しているため)。
- [ ] 基準線: `git worktree add` で origin/main を別の場所に出し、`web/` で `npm ci && npm run build`、`web/dist` を静的配信 (例 `python3 -m http.server`)。定義ファイル (api/layout) が無くても組み込みの定義で並ぶ。ブランチ側も同じく build して静的配信。
- [ ] Playwright (Chromium) で、1440×900 / 1280×720 / 390×844 / 844×390 / 915×350 の 5 つ、`?demo=standard` を開き、docs/ui-spec/tools/measure.mjs のセレクタ一覧の bounding rect を両方で取り、±1px で一致を確かめる (天気の札の案内は幅を除く)。Playwright の場所は check-layout.mjs のフォールバックを参照。
- [ ] Review Focus 1: 1440×900 で開き、844×390 → 390×844 → 1440×900 と viewport を変えたあと、`#eew-banner`・`#tsunami-banner`・`#warn-banner` が `document.body.contains` で文書内にあること。`?layout=trial` で開いて regular 幅のとき、`#eew-banner` が `.ld-unused` の中にあること。
- [ ] Review Focus 2: compact 390×844 で `.inset-okinawa` を一時的に hidden にしても `.legend` の位置が動かないこと。
- [ ] `?layout=trial&demo=standard` (1440×900) のスクリーンショットを、EEW の発表中と平時の 2 枚。
- [ ] code-reviewer (Opus) でブランチ全体の差分をレビューし、CRITICAL・HIGH を報告。

### Task 7: 文書

**Files:**
- Modify: `docs/ui-spec/layout-system.html` (「定義ファイル」の表に `manual`・積みの `gap`/`pad`/`minHeight`・部品は省ける (main は必須)・`?layout=` を足す。第 6 章の「CSS で補っている」の文を、定義に移したことに直す)
- Modify: `docs/ui-layout-backlog.md` (「2. 書式を育てる」の余白・場所取りを「済んだこと」へ。済んだことに「部品の省略・試験定義・EEW の常設パネル (試験定義だけ)」)

- [ ] 文書を直す
- [ ] Commit: `git commit -m "docs: レイアウトの定義の新しいキー (省略・余白・試験定義) と EEW の常設パネルを記す"`

## このあと (この計画の外)

- JDQ の形だけの段 A (2×2 の並び) を "trial" で組む。公開するとき、`data/banner` に「JDQ・JQuake の画面構成を参考に試験中 (各ソフトの公式・提携ではない)」の告知を置く (外に見える変更なので、置く前にユーザーに確認)。
- 版の上げ (0.34.0) とリリースはマージ後に別途。
