# サブの地図の見出し・凡例・P 波 S 波・フェードアウト Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** web の右上のサブの地図 (試験定義 trial の `#map-sub`) を JDQ の右ペインに近づける: 地図の左上に見出しの札 (最大震度・震源・M・深さ・情報の種類と時刻・津波)、右下に凡例 (出ている震度の段階・震央・予想・P 波 S 波)、P 波・S 波の円、保持時間を過ぎたら薄くしてからフェードアウト。

**Architecture:** 見出しと凡例の中身は純粋な関数 (新規 `web/src/sub-caption.ts`) が決め、描くのは `web/src/sub-map.ts`。札と凡例はレイアウトの部品 (slot) にせず、サブの地図の要素の中に持つ (部品は 1 回しか置けないため)。濃さ (フェードアウト) は `web/src/hold.ts` の純粋な関数。段階 4 (配信 native) で Rust に写せるよう、判断はすべて純粋な関数とデータにする。

**Tech Stack:** TypeScript、`node --test`、Playwright (検証)。

**Spec:** 2026-10-06 の会話での合意:
- 右列の「詳細」パネルは trial でも残す (観測点の一覧があるため。見出しとの重複は段階 4 で考え直す)。
- 凡例は「出ている段階だけ」(JDQ と同じ)。
- 地図の出典表記はサブの地図に出さない (ページ下の出典で足りる)。
- 履歴から寄り図の地震を除く (JDQ の流儀) は今回やらない。
- 保持時間を過ぎたら、経過時間は添えず、薄くしてからフェードアウトして消す (震度が高いほど長く残すのは保持時間で既に決まっているので、フェードの長さは震度によらず一定)。
- P 波・S 波の色は左の地図と同じ。描くのはサブの地図に映している地震の分だけ。
- あわせて docs/ui-layout-backlog.md の「スマホで、トースト・音の案内が時計に重なりうる」を済みにする (v0.36.3 で直した)。

## Global Constraints

- web のコマンドは `web/` で: `npm test` / `npm run typecheck` / `npm run build`。layout.json を変えたら `cargo test -p eq-server layout`。
- 既存の 3 定義 (landscape / regular / compact) の見た目と左の地図の動きは変えない。サブの地図の描き直しの決まり (地震が変わったときだけ塗り直す) を崩さない: 毎フレーム変えてよいのは P 波 S 波の線だけ。
- 文言は既存の詳細パネル (web/src/view.ts の `renderDetail`、`TSUNAMI_TEXT`、groups.ts の `INFO_LABELS`) と同じものを使う。重複させず、純粋なモジュールへ移して両方から使う。
- 日本語の conventional commit、署名なし。push とマージは親。音の出るブラウザ (アプリ内のブラウザ枠など) は使わない。検証は scratchpad の Playwright (/private/tmp/claude-501/-Users-densuke-Documents-GitHub-eq-webservice--claude-worktrees-e2-deploy-systemd-setup-6f9b69/5cc78b2d-d533-4b91-abe9-48dc8c7a71d4/scratchpad/pw、`import { chromium } from "playwright"`)。

## Review Focus

1. EEW → 震度速報 (震源調査中) → 地震情報 (確定) → 取り消し、の各段階で見出しと凡例が正しい (震源が無い・M が無い・深さ 0 = ごく浅い・津波の種類ごとの文言)。→ Task 1 のテスト。
2. フェードアウトの境目 (保持時間ちょうど・フェードの途中・フェードの終わり) で濃さが正しく、終わったら地図・札・凡例が消えて日本全体になる。→ Task 2 のテスト + Task 4 の実測。
3. 波を描いている間も、塗り・札・凡例を毎フレーム描き直さない (署名は変わらない)。→ Task 3 の実装 + Task 4 で描き直しの回数を数える。
4. 定義ファイルの `subMap` に新しいキー `fadeSec` が無い古いファイルでも動く (組み込みの既定値)。正しくない値は誤り。→ Task 2 のテスト。
5. 1280×720・1440×900・1920×1080 で、見出しの札と凡例がサブの地図からはみ出さず、震央と塗りが札の下に隠れない (カメラの範囲を札の分だけ上へ広げる)。→ Task 3 の実装 + Task 4 の実測。

---

### Task 1: 見出しと凡例の中身 (純粋な関数)  ※並列 A

**Files:**
- Create: `web/src/sub-caption.ts`、`web/src/sub-caption.test.ts`
- Modify: `web/src/view.ts` (`TSUNAMI_TEXT` と震源の文字列 `hypoText` を新しい純粋なモジュールへ移して import。見た目は変えない)。移し先は sub-caption.ts か、既存の純粋なモジュールのうち適切なもの (DOM に触らないもの)。

**Interfaces:**

```ts
export interface SubCaption {
  /** 最大震度 (EEW は予想最大震度)。分からなければ -1 */
  scale: Scale;
  /** 例 "緊急地震速報 (予報) 第5報" / "震度速報" / "各地の震度に関する情報" (INFO_LABELS) */
  kind: string;
  /** EEW の予報/警報の色分け用 */
  eew: "forecast" | "warning" | null;
  /** 震源名。無ければ "震源調査中" (EEW は "震源不明")、取り消しは "取り消されました" */
  title: string;
  /** "M5.3 / 深さ40km" など (hypoText と同じ規則。無ければ空文字) */
  facts: string;
  /** 発生時刻の文字列 (詳細パネルと同じもの) + " 発生" */
  time: string;
  /** 津波の一文 (TSUNAMI_TEXT)。EEW は null */
  tsunami: string | null;
}
export interface SubLegend {
  /** 出ている震度の段階 (大きい順、重複なし)。地震情報は観測点と県の最大の震度、EEW は予想の区域・県の震度 */
  scales: Scale[];
  /** EEW の予想の塗りを描いている */
  forecast: boolean;
  /** 震央を描いている */
  epicenter: boolean;
  /** P 波・S 波の円を描いている */
  waves: boolean;
}
export function subCaption(g: Group): SubCaption | null;   // quake・eew 以外は null
export function subLegend(g: Group, waving: boolean): SubLegend | null;
```

- quake の段階は `summarizeQuake(g)` の観測点 (`points` の scale) と `prefMax`、eew は `heldEew(g)` の `areas` (scale_from) と `pref_max`。
- 時刻・文言は詳細パネル (view.ts の renderDetail) と完全に同じになること (テストで詳細パネル側の文字列を作る関数と比べられるなら比べる)。

- [ ] **Step 1: 失敗するテストを書く** — 場面: EEW 予報 (第 5 報、M あり)、EEW 警報、EEW 取り消し、震度速報 (震源なし: title "震源調査中"、facts 空、tsunami "津波の有無を調査中")、地震情報 確定 (観測点 3 段階 → scales が大きい順・重複なし)、深さ 0 ("ごく浅い")、津波 Warning の文言、tsunami 種別以外の group → null。テスト用の Group の作り方は既存のテスト (groups.test.ts・held.test.ts・detail.test.ts) に合わせる。
- [ ] **Step 2: 落ちることを確かめる** — `cd web && npm test` / FAIL
- [ ] **Step 3: 実装する** (view.ts から文言を移す)
- [ ] **Step 4: 通ることを確かめる** — `npm test && npm run typecheck && npm run build` / PASS (詳細パネルの見た目は変えない)
- [ ] **Step 5: Commit** — `feat: サブの地図の見出しと凡例の中身を決める純粋な関数 (詳細パネルと同じ文言を共有)`

---

### Task 2: 保持時間のあとの薄さとフェードアウト (純粋な関数・設定)  ※並列 B

**Files:**
- Modify: `web/src/hold.ts`、`web/src/hold.test.ts`、`web/src/layout.ts` (`SubMapConfig` に `fadeSec`、checkLayouts)、`web/src/layout.json` (`subMap` に `"fadeSec": 120`)、`web/src/layout.test.ts`

**Interfaces:**

```ts
// layout.ts の SubMapConfig に足す
/** 保持時間を過ぎて fadedAlpha になったあと、0 まで薄くする秒数 (任意。無ければ組み込みの 120) */
fadeSec: number;

// hold.ts: SubMapState の faded を alpha に置き換える
export interface SubMapState { key: string; /** 濃さ 0〜1 (1 = 保持時間の内) */ alpha: number }
export function subMapState(now: number, g: { key: string; kind: string; updatedAt: number } | undefined, scale: number, cfg: SubMapConfig): SubMapState | null;
export function subMapSigs(state: SubMapState | null, g: { updatedAt: number } | undefined, w: number, h: number): { paint: string; fade: string };
```

仕様:
- 経過 e = now − updatedAt、保持 H = holdMs(scale)。e ≤ H → alpha 1。H < e < H + fadeSec·1000 → alpha = fadedAlpha × (1 − (e − H) / (fadeSec·1000))、0.05 刻みに切り下げ (署名の変化を 0.05 ごとに抑えるため)。e ≥ H + fadeSec·1000、または切り下げた alpha が 0 → null (消える)。
- `subMapSigs`: paint は今と同じ (key|updatedAt|w×h)、fade は `String(state.alpha)`。fadedAlpha は alpha に含まれるので引数から外す。
- checkLayouts: `fadeSec` は任意、0 より大きい数。無い古い定義ファイルは組み込みの 120 を使う (`subMapConfig()` が既定を補う)。
- 既存の `faded` を使っている箇所 (sub-map.ts) はこの Task では触らない。型が変わって typecheck が落ちる場合は、sub-map.ts の該当 2〜3 行だけを `alpha` に置き換える最小の修正をしてよい (`setFade(state.alpha)`)。

- [ ] **Step 1: 失敗するテストを書く** — 震度 2 (保持 60 秒、fadedAlpha 0.4、fadeSec 120): e = 60000 → 1、60001 → 0.4 未満で 0.35 以上 (切り下げで 0.35 または 0.4。式どおりの値をテストに書く)、e = 60000 + 60000 → 0.2、e = 60000 + 119999 → null か 0 でない最小値 (式どおり)、e = 180000 → null。fadeSec の検査 (0・負・文字列は誤り、無くても通る)。subMapSigs の fade が alpha の文字列。
- [ ] **Step 2: 落ちることを確かめる**
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる** — `npm test && npm run typecheck && npm run build`、`cargo test -p eq-server layout`
- [ ] **Step 5: Commit** — `feat: サブの地図は保持時間を過ぎたら薄くしてからフェードアウトして消える (subMap.fadeSec)`

---

### Task 3: サブの地図に見出し・凡例・P 波 S 波を描く (統合)  ※A・B のあと

**Files:**
- Modify: `web/src/sub-map.ts`、`web/src/main.ts` (波を渡す)、`web/src/scene.ts` (必要なら、主の地図の波の値を返す)、`web/public/style.css`
- Modify: `docs/ui-spec/layout-system.html` (サブの地図の振る舞いに見出し・凡例・波・フェードアウトを書く、subMap の表に fadeSec)、`docs/ui-layout-backlog.md` (サブの地図に重ねる詳細・凡例と P 波 S 波を済みに。「スマホで、トースト・音の案内が時計に重なりうる」を済みに)

**Interfaces:**
- Consumes: Task 1 の `subCaption`・`subLegend`、Task 2 の `SubMapState.alpha`・`subMapSigs`。
- Produces: `export function renderSubWaves(wave: { key: string; lat: number; lon: number; pKm: number | null; sKm: number | null } | null): void` (sub-map.ts)。

仕様:
- sub-map.ts は `#map-sub` の中に 2 つの要素を作る: 見出しの札 `.sub-caption` (左上)、凡例 `.sub-legend` (右下)。`paint()` のとき (塗り直しのときだけ) 中身を作る。文字列はすべて `esc()`。地震が無い・state が null のときは隠す。
- 見出しの札の構成 (左上、幅はサブの地図の 60% まで): 大きな震度の札 (既存の `badge` の見た目を流用できればする)、kind (EEW は予報 = 橙 #b35900・警報 = 赤 var(--alert) の文字色)、title (大きめ)、facts、time、tsunami (あれば)。
- 凡例 (右下、縦に並べる): scales の各段階の色札と震度の文字 (scaleColor・scaleLabel)、forecast なら「予想」、epicenter なら「震央」(✕の印)、waves なら「P波」「S波」(線の色は左の地図の .legend の P波・S波と同じ)。
- 濃さ: `subMap.setFade(alpha)` と同時に、見出しと凡例の要素の opacity も alpha にする。alpha の変化 (fade の署名の変化) のときは opacity と setFade だけを変える。
- P 波 S 波: main.ts の tick で、主の地図の波 (scene.ts の renderScene が描いている主の地震の波) を求めた直後に `renderSubWaves(...)` を呼ぶ。渡すのは主の地震の分 (key・中心・pKm・sKm)。sub-map.ts は、それがサブの地図に映している地震 (q.key、または relatedQuake で同じ地震) と同じときだけ `subMap.setWaves([...])`、違う・null のときは `subMap.setWaves([])`。波を描いているかどうかが変わったら凡例の「P波・S波」の行だけを出し入れする (塗りは描き直さない)。renderScene が波を返さない形なら、scene.ts に最小の変更を入れて値を取り出す (左の地図の動きは変えない)。
- カメラ: 見出しの札が左上を覆うので、震央と塗りが札の下に隠れないよう、`paint()` で寄る範囲の箱を上へ広げる (箱の高さ × 札の高さ ÷ (サブの地図の高さ − 札の高さ) だけ上端を伸ばす)。札の高さは描いたあとの実測 (offsetHeight) を使う。凡例は小さいので右下はそのまま。
- 見出しの札と凡例には `aria-hidden="true"` (同じ内容が右列の詳細パネルにあり、読み上げが重複するため)。
- style.css: `.sub-caption`・`.sub-legend` は `#map-sub` の中で position: absolute。既存の詳細パネル・凡例の配色に合わせ、背景は半透明の暗色 (地図が透けて見える程度)。pointer-events: none。

- [ ] **Step 1: 実装する** (純粋な部分は Task 1・2 でテスト済み)
- [ ] **Step 2: 確かめる** — `npm test && npm run typecheck && npm run build`
- [ ] **Step 3: 自分で目で見る** — build した web/dist を静的配信し (ws は abort)、Playwright で `?layout=trial&demo=standard` を 1440×900 で開いて、EEW 中・震度速報・確定の時点のスクリーンショットを撮る (scratchpad/shots/f-*.png)。
- [ ] **Step 4: Commit** — `feat: サブの地図に見出しの札・凡例・P 波 S 波を描き、保持時間のあとはフェードアウトする`、文書は別コミット `docs: サブの地図の見出し・凡例・波・フェードアウトを記し、済んだ課題を消し込む`

---

### Task 4: 検証 (Opus)

- [ ] コードレビュー (差分全体)。Review Focus 1〜5、XSS (esc の漏れ)、描き直しの回数。
- [ ] 自動: `npm test` / `npm run typecheck` / `npm run build` / `cargo test -p eq-server`。
- [ ] 既存 3 定義: 5 画面 (1440×900 / 1280×720 / 390×844 / 844×390 / 915×350) × (`?demo=standard` / 平時) で origin/main と bounding rect ±1px。
- [ ] trial (1280×720・1440×900・1920×1080): `demo=standard` で EEW → 震度速報 → 確定の見出しと凡例の文言・はみ出し、波がサブの地図にも出ること、波の間にサブの地図の塗り直しが起きないこと (paint の回数を数える)。`demo=forecast` を ×8 で流して、保持時間 → 薄く → フェード → 消える (日本全体・札と凡例なし) を確かめる。スクリーンショット。
