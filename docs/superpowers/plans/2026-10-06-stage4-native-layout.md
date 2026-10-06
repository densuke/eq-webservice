# 段階 4: 配信 (native) がレイアウトの定義で並べる Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 配信 (Rust の native、crates/eq-server/src/broadcast/native/) の並びを、web と同じ定義ファイル (web/src/layout.json、eq-server の `[layout] file` / `GET /api/layout`) の中の配信用の定義から決める。定義を書き換えて eq-broadcast を再起動すれば配信の並びが変わる。最終的な目標は、JDQ 風・JQuake 風の並びを定義で「それなりに」再現できること (丸写しはしない)。

**Architecture:** 定義の型 (serde)・検査・矩形の割り付け (純粋な関数) を新しいモジュールに置く。配信は起動時に `{server}/api/layout` を 1 回読み (だめなら組み込みの `crate::layout::BUILTIN`)、平時用 `broadcast` と地震の画面用 `broadcast-quake` の 2 つを割り付けて Renderer に持たせる。描画は「割り付けた矩形の原点 + 今の相対オフセット」で行い、描く順は今の `Renderer::render` のまま。

**Tech Stack:** Rust (serde・tiny-skia)、`cargo test`、web の `node --test`。

**Spec:** 2026-10-06 の会話での合意 (利用者の Go)。調査 (Opus) の結果をもとにした決めごと:
- 配信用の定義は `manual: true` の名前付きの定義 2 つ (`broadcast` = 平時、`broadcast-quake` = 地震の画面) を layout.json の layouts の末尾に足す。`when: {"minWidth": 801, "minHeight": 481}` を付ける (スマホでは選ばれない)。web のコードは変えない (web は manual を自動では選ばず、`?layout=broadcast` で同じ並びを確かめられる)。使う部品の名前は web の SLOTS にあるものだけ (知らない名前があると web がファイル全体を捨てるため)。
- cast.toml (`BroadcastConfig`) に `layout` (既定 "broadcast") と `layout_quake` (既定 "broadcast-quake") を足す。
- `auto` は部品ごとの固定の大きさを native 側に持つ (文字で測らない。フォントの無い CI でも結果が変わらないように)。
- size: `fill`・`fill:n`・`auto`・`px`・`%`・`vh/svh/dvh/vw` (1280×720 で計算) を扱う。`calc()/clamp()/min()/max()/em/rem` は配信用の定義では誤り (組み込みの定義に戻る)。
- 配信だけの表示 (状態の札・BGM の曲名・配信元・テスト配信の印・気象警報の帯) は定義に書かない。今と同じく上部バーの中か画面全体に重ねる。
- native に無い部品 (countdown・ogasawara・eew-panel・toast・hint・settings・caption・banners・history-head など) が配信用の定義にあれば、ログに出して描かない (誤りにしない)。
- 天気の案内の窓 (calm.rs の INFO_WINDOW) は地理で決めた位置のまま (割り付けの外)。
- 読み込みは起動時だけ。変えたら eq-broadcast を再起動。replay-video は BUILTIN。
- 2 つの定義の main と topbar の矩形は同じでなければ誤り。
- Task 1〜3 は配信の出力を 1 ビットも変えない (PNG の shasum が全部一致)。Task 4 は出力が変わる (地震の画面の右パネル)。
- 見送り: `only: "broadcast"` のブロック (web の検査を先に直す必要がある)、サブの地図の札・凡例・保持時間の移植、再読み込み。

## Global Constraints

- `cargo fmt --all`、`cargo clippy --all-targets -- -D warnings`、`cargo test -p eq-server` が通ること。layout.json を変えたら web の `npm test`・`npm run typecheck` も。
- 出力の一致の確かめ方: 変更前 (この計画の起点のコミット) と変更後で `EQ_NATIVE_PNG_DIR=<dir> EQ_NATIVE_FONT="/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc" cargo test --release -p eq-server write_ -- --nocapture` を流し、書き出した PNG の shasum が全部一致すること (前回の例: docs/native-submap-bench.md)。sub_map の書き出し (write_sub_map_png_when_asked) も含める。
- 日本語の conventional commit、署名なし。push とマージは親。
- 描く順・still_key・コマの重さは変えない。

## Review Focus

1. 定義が読めない・壊れている・配信用の名前が無い・使えない size があるときに、組み込みの定義へ戻って配信が止まらないこと。組み込みも壊れていれば起動を失敗させる (テストで組み込みが通ることを守る)。
2. 割り付けの矩形が今の定数と 1px 単位で一致すること (Task 2 のテスト) と、出力の PNG が一致すること (Task 3)。
3. web 側: 足した 2 つの定義が web の checkLayouts を通り、`?layout=broadcast` で開いても壊れないこと、自動では選ばれないこと。
4. serde の `deny_unknown_fields` と `untagged` の組み合わせで、web の定義 (note を含む) がすべて読めること (出荷の layout.json の全定義を Rust で読むテスト)。
5. 地震の画面と平時で定義を切り替えたとき、枠 (main・topbar) が同じなので base の絵や投影を作り直さないこと。

---

### Task 1: 定義の型・読み込み・検査と、配信用の定義 2 つ (出力は変えない)

**Files:**
- Create: `crates/eq-server/src/broadcast/native/layout_def.rs` (型・解析・検査)
- Modify: `crates/eq-server/src/broadcast/native/mod.rs` (起動時に `{server}/api/layout` を読んでログに出す。どの定義を使うかを決めて保持するだけで、描画にはまだ使わない)、`crates/eq-server/src/broadcast/mod.rs` (`BroadcastConfig` に `layout`・`layout_quake`、Default)、`web/src/layout.json` (末尾に `broadcast`・`broadcast-quake`)

**Interfaces:**

```rust
// layout_def.rs
pub struct LayoutFile { pub version: u32, pub layouts: Vec<LayoutDef>, /* subMap と note は読んで捨ててよい */ }
pub struct LayoutDef { pub name: String, pub when: Option<When>, pub manual: Option<bool>, pub scroll: Option<String>, pub root: Node }
pub struct Node { pub slot: Option<String>, pub variant: Option<String>, pub dir: Option<Dir>, pub r#box: Option<String>, pub size: Option<String>, pub children: Option<Vec<Node>>, pub overlays: Option<BTreeMap<Corner, Stack>> }
pub struct Stack { pub flow: Dir, pub items: Vec<StackItem>, pub gap: Option<String>, pub pad: Option<String>, pub min_height: Option<String> /* JSON は minHeight */ }
pub enum StackItem { Name(String), Part { slot: String, variant: Option<String> }, Stack(Stack) }   // #[serde(untagged)]
pub fn parse(json: &str) -> anyhow::Result<LayoutFile>;
/// 配信で使う定義を名前で取り出して検査する (main を 1 回置く・同じ部品を 2 回置かない・size が native で扱える)
pub fn pick<'a>(file: &'a LayoutFile, name: &str) -> anyhow::Result<&'a LayoutDef>;
/// 起動時: 取れた JSON (無ければ None) から 2 つの定義を決める。だめなら BUILTIN から。BUILTIN もだめならエラー
pub fn load(json: Option<&str>, name: &str, name_quake: &str) -> anyhow::Result<(LayoutDef, LayoutDef)>;
```

- `note` はどこでも読んで捨てる (`#[serde(default)] note: Option<serde_json::Value>` など)。知らないキーは誤り。
- 2 つの定義の main と topbar の割り付けが同じかの検査は Task 2 (割り付けができてから) で `load` に足す。

配信用の定義の中身 (今の native の並びを表す。値は native の定数から。web で開いても大きく崩れないこと):
- `broadcast`: root column: `topbar` 36px / row fill: [ `main` fill (overlays: top-left column [inset] / bottom-left column [legend] / bottom-right column [clock]) , side 380px column: `detail` (詳細 y36〜182 に当たる固定の大きさ) / `history` fill / `notice` (お知らせの位置に当たる) / `credit` (出典) ]。正確な px は panel.rs・notice.rs の定数から決める。
- `broadcast-quake`: `broadcast` と同じで、右列の詳細の下にサブの地図 `map-sub` (今の SUB_RECT = y190・高さ 300 に当たる) を置き、`notice` は置かない (今は平時だけ描くため)。
- note に「配信 (native) 用。?layout=broadcast で web でも見られる」。

- [ ] **Step 1: 失敗するテストを書く** — 出荷の layout.json (`crate::layout::BUILTIN`) の全定義が `parse` で読めること、`pick` が broadcast と broadcast-quake を返すこと、main を置かない・2 回置く・`calc()` の size の定義が誤りになること、`load(Some("壊れた"), ..)` と `load(None, ..)` が BUILTIN の定義を返すこと、`BroadcastConfig` の `layout` の既定値と toml からの読み込み。
- [ ] **Step 2: 落ちることを確かめる**
- [ ] **Step 3: 実装する** (起動時の取得は `notice_loop` と同じ reqwest の作り。失敗は警告を 1 回だけ)
- [ ] **Step 4: 通ることを確かめる** — cargo の fmt・clippy・test、web の `npm test && npm run typecheck`
- [ ] **Step 5: PNG の shasum が変わらないことを確かめる** (描画は変えていないので一致するはず)
- [ ] **Step 6: Commit** — `feat: 配信がレイアウトの定義 (broadcast / broadcast-quake) を読んで検査する (描画はまだ定数)`

---

### Task 2: 矩形の割り付け (純粋な関数)

**Files:**
- Modify: `crates/eq-server/src/broadcast/native/layout_def.rs` (または新しい `layout_resolve.rs`)

**Interfaces:**

```rust
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }
/// 定義を幅 w・高さ h の画面に割り付け、部品の名前 → 矩形 を返す。重ね物 (overlays) の部品も入る。native に無い部品は返さない (ログは呼び出し側)
pub fn resolve(def: &LayoutDef, w: f32, h: f32) -> anyhow::Result<BTreeMap<String, Rect>>;
/// auto の部品の固定の大きさ (縦に積むときは高さ、横に並べるときは幅)
fn auto_size(slot: &str) -> f32;
```

- 割り付けの規則は web の flex と同じ: 容器の向きに沿って、固定 (px・%・vh・vw・auto の固定値) を先に引き、残りを fill の重みで分ける。交差方向は容器いっぱい。
- 重ね物: 隅に寄せ、積み (flow) の向きに gap で並べる。pad の既定は 10px (web の .ld-corner の padding と native の今の余白)。部品の大きさは native の固定の大きさ (inset は今の別枠の大きさ、legend は LEGEND_RECT、clock は 176×74)。
- `load` に「2 つの定義の main と topbar の矩形が同じ」の検査を足す。

- [ ] **Step 1: 失敗するテストを書く** — `resolve(broadcast, 1280, 720)` が今の定数と一致: main = MAP_RECT (0,36,900,684)、side の各部品 (detail・history・notice・credit) の位置、inset = 今の別枠の位置 (frame.rs の OKINAWA)、legend = LEGEND_RECT、clock = 今の時計の位置 (panel.rs)。`broadcast-quake` の map-sub = SUB_RECT (900,190,380,300)。fill:2 の重み、%・vh の計算、交差方向、隅の積みの gap の単体テストも。
- [ ] **Step 2: 落ちることを確かめる**
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる**
- [ ] **Step 5: Commit** — `feat: レイアウトの定義を 1280×720 の矩形に割り付ける純粋な関数 (今の定数と一致)`

---

### Task 3: 描画を割り付けの矩形で行う (出力は 1 ビットも変えない)

**Files:**
- Modify: `crates/eq-server/src/broadcast/native/draw.rs`・`panel.rs`・`notice.rs`・`frame.rs`・`camera.rs`・`calm.rs`・`step.rs`・`test_mark.rs`・`mod.rs` (load_renderer)

仕様:
- Renderer に「平時の割り付け」と「地震の画面の割り付け」を持たせ、`render()` で Scene (quake・eew の有無) に応じて使い分ける。main と topbar は同じなので、base の絵・投影 (`View::fit_home(main)`)・隠す型は 1 つのまま。
- 定数 (MAP_RECT・MAP_W・SUB_RECT・SIDE_X・panel の y の即値・notice の BOX_X/BOX_Y・LEGEND_RECT・時計の位置・OKINAWA の x/y・map_aspect・main_bounds・card_room など) を、矩形の原点 + 今の相対オフセットに置き換える。部品の矩形が無い (定義に置かれていない) ときは描かない。
- notice.rs のコンパイル時の assert は実行時の検査に (重なるときはログを出して描かない)。
- 描く順・still_key・天気の案内の窓 (地理の位置) は変えない。
- replay-video (replay/mod.rs) は BUILTIN の 2 定義を使う。

- [ ] **Step 1: 起点の PNG を書き出す** (変更前、shasum を控える)
- [ ] **Step 2: 実装する** (既存のテストの「枠の外を塗らない」「SUB_RECT の中」「詳細の欄に描かない」の画素の検査を、矩形を使う形に直す)
- [ ] **Step 3: 確かめる** — cargo の fmt・clippy・test、PNG の shasum が全部一致 (平時・地震・EEW・お知らせ・警報・テスト・寄り・サブの地図)。`sub_map_cost` を `--ignored` で流して、Mac で重さが変わっていないこと (前の値は docs/native-submap-bench.md)。
- [ ] **Step 4: Commit** — `refactor: 配信の描画を定数ではなく、レイアウトの定義から割り付けた矩形で行う (出力は同じ)`

---

### Task 4: 地震の画面を JDQ 風に寄せる (出力が変わる。PR と画像まで、マージは利用者が見てから)

- `broadcast-quake` を JDQ 風に寄せる: 例えば右列を「詳細 / サブの地図 (大きめ、右列の半分程度) / 履歴 (残りの高さ、行数 = 高さ / 52)」。参照の比率は docs/ui-spec/layout-system.js の `LS_PROFILES` の jdq (上段の右に寄り図 約 51%)。丸写しはしない。
- 平時の `broadcast` が JQuake 風 (地図 68 : 右列 32) に近いことを、割り付けの数値で示す (今は 70.3 : 29.7)。
- 書き出した PNG (平時・EEW・確定) と、JDQ・JQuake の比率との比較 (割り付けの数値) を PR に載せる。

---

### Task 5: 検証 (Opus)・PR・マージ・差し替え

- Task 1〜3: Opus のコードレビュー (差分全体、Review Focus)。PNG の一致の確認 (Opus がもう一度、別の場所で流す)。web の `?layout=broadcast` と `?layout=broadcast-quake` を Playwright で開いて壊れていないこと。既存の web の定義の 5 画面の比較 (origin/main と ±1px)。
- 版 0.38.0 で PR → CI → マージ → タグ → n2 の差し替え (`eq-server` を再起動すると配信と BGM も再起動)。差し替え後、配信のログに定義を読んだ旨が出ること、配信の直近のコマを 1 枚取り出して目で確かめること、busy chip が増えないこと。
- Task 4: 別の PR。画像と比率を載せ、マージしない。
