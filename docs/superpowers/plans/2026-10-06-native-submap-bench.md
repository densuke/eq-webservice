# 配信 (native) にサブの地図を試しに描き、重さを測る Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 配信の右パネルの上に、表示中の地震 (無ければ最新の地震) へ寄った 2 枚目の地図を描く試験の設定 `sub_map` を足し、`render()` と `step()` の重さを on/off で測れるようにする。既定は無効で、無効のときの出力は 1 バイトも変えない。

**Architecture:** 地図を描く部品 (`Frame::zoomed`・`View::from_fit`・`fit_box`・`draw_land`・`draw_shake`) は面を引数に取るので、別の矩形 `SUB_RECT` とその枠 `Clip` でもう一度呼ぶ。範囲は寄りと同じ `aim_of` → `target_box` (波を追わない) → `fit_box`。描き直しの合図 `still_key` は変えない (サブの範囲は地震で決まり、地震の情報は既に still_key に入っている)。

**Tech Stack:** Rust (tiny-skia)、`cargo test`。

**Spec:** 2026-10-06 の会話。利用者: 「重さを見ておくこと自体はいい。n2 は専用なので若干増えても可。配信の遅れなどを検証してから要調整」。下調べ (Opus) の結果を下にまとめた。

## Global Constraints

- 既定 (`sub_map` を省く・false) では、配信と再現動画の出力が変更前と 1 バイトも変わらない。
- 区域の塗り・観測点の点は native に無いので、この実験では作らない (県の塗り・震度の札・震央だけ)。
- キャッシュ (サブの地図の Pixmap を覚えて貼る) は作らない。まずキャッシュなしの上限を測る。
- 日本語の conventional commit、署名なし。push とマージは親。

## Review Focus

1. 既定の出力が変わらないこと → 変更の前後で PNG の書き出し (`write_` のテスト) の shasum が全部一致。
2. `load_renderer` の zones の読み込み条件の変更で、`zoom` が true でも false でも従来と同じ Renderer になること。
3. サブの地図が `SUB_RECT` の外を汚さないこと → 正しさのテスト。
4. サブの地図で描き直しの回数が増えないこと → 正しさのテスト。

## 下調べで分かったこと (要点)

- 描き直し (`Renderer::render`) は still_key が変わったときだけ。静かなときも地震のときも 1 秒に 1 回。寄りが有効で表示範囲が動いている間だけ毎コマ。
- `draw_land(pm, &Frame, neighbors, prefs)` (draw.rs:235)、`draw_shake(pm, text, prefs, &Frame, &Shake)` (draw.rs:304) は面を引数に取る。
- `map_clip()` (frame.rs:95-98) は MAP_RECT 固定、`Clip::new` は非公開 (frame.rs:59)。Clip は 1280×720 の Mask で約 0.9MB。
- `draw.rs:346` の EEW の札の右端が MAP_W 固定 → サブの地図には `Shake { tag: None, .. }` を渡して札を出さない。
- 寄りの範囲の `aim_of` (step.rs:97-161) に `origin_ms: None` を渡すと `target_box` は波を追わない分岐 (camera.rs:100-107)。
- 既存の計測 `zoom_cost` (zoom_tests.rs:389-410、`#[ignore]`) が雛形。地図の置き場所は `env!("CARGO_MANIFEST_DIR")` で決まる。

---

### Task 1: 設定 `sub_map` とサブの地図の描画 (既定は無効)

**Files:**
- Modify: `crates/eq-server/src/broadcast/mod.rs` (`BroadcastConfig` に `pub sub_map: bool`、doc コメント「試験: 右パネルの上にサブの地図を描く (重さを測るため)。既定は無効」、`Default` に false)
- Modify: `crates/eq-server/src/broadcast/native/frame.rs` (`pub fn sub_clip() -> Option<Clip>`、`map_clip` の隣)
- Modify: `crates/eq-server/src/broadcast/native/draw.rs`
- Modify: `crates/eq-server/src/broadcast/native/step.rs`
- Modify: `crates/eq-server/src/broadcast/native/mod.rs` (`load_renderer`)
- Test: 既存のテストのファイル (tests.rs / zoom_tests.rs) に足す

**Interfaces (draw.rs):**
- `pub const SUB_RECT: (f64, f64, f64, f64) = (MAP_W as f64, BAR_H as f64, W as f64 - MAP_W as f64, 300.0);`
- `Renderer` に `sub_clip: Option<Clip>`・`sub: Option<Frame>` (new では None)
- `pub fn enable_sub_map(&mut self)` / `pub fn sub_map_enabled(&self) -> bool` / `pub fn set_sub_view(&mut self, fit: Option<Fit>)` (`Frame::zoomed(&self.main.view, View::from_fit(&f, SUB_RECT), clip)`)
- `render()` で `panel::draw_dynamic` の直後に、`self.sub` があれば `draw_sub_map(...)`: `SUB_RECT` を海の色で塗る → `draw_land` → `sub_shake(scene)` があれば `draw_shake` → 1px の枠 (`INSET_LINE`)
- `fn sub_shake<'a>(scene: &Scene<'a>) -> Option<Shake<'a>>`: `Shake::of(scene)` の `tag` を None に。無ければ `scene.history.first()` を観測として (forecast false、tag None)

**Interfaces (step.rs):**
- `Stepper` に `fn aim_sub(&mut self, current, groups, eews, now)`: 無効なら何もしない。対象 = `current.or(groups.first().map(eew::Current::Quake))` → `aim_of(..)` の震源の `origin_ms` を None → `target_box` → `fit_box(b, SUB_RECT.2 / SUB_RECT.3)` → `renderer.set_sub_view(..)`。`step()` の `aim_camera` の直後に呼ぶ。`still_key` は変えない。(引数の型は step.rs の実際の型に合わせる)

**load_renderer:**
- 細分区域 (zones) と観測点 (stations) の読み込みを `cfg.zoom || cfg.sub_map` のときに行う。`enable_zoom` を「寄りの枠を立てる」と「zones を入れる (`set_zones`)」に分け、寄りの枠は `cfg.zoom` のときだけ。`cfg.sub_map` なら `enable_sub_map()`。

- [ ] **Step 1: 失敗するテストを書く** (常に走るもの)
  - `the_sub_map_paints_only_inside_its_rect`: 同じ Scene (chiba の EEW) を off の Renderer と、on + `set_sub_view(Some(fit))` の Renderer で描き、`SUB_RECT` の外の全画素が一致し、内側に違う画素が 1 つ以上ある。
  - `the_sub_map_does_not_add_redraws`: 同じ Input の列 (chiba を 200ms ごとに 50 コマ) で、off/on の Stepper の `step()` が Some を返す回数が同じ。
  - `the_sub_map_shows_the_latest_quake_when_calm`: 落ち着きの時間 (3 分) を過ぎた時刻で、on のときサブの地図の中に震度の色の画素がある (文字を描かない設定で、フォントが無くても通る)。
  - 設定: `sub_map` を省くと false、`sub_map = true` が読める (既存の BroadcastConfig の toml のテストの形に合わせる)。
- [ ] **Step 2: 落ちることを確かめる** — Run: `cargo test -p eq-server broadcast` / Expected: FAIL (コンパイル不可または失敗)
- [ ] **Step 3: 実装する**
- [ ] **Step 4: 通ることを確かめる** — Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test -p eq-server` / Expected: PASS
- [ ] **Step 5: 既定の出力が変わらないことを確かめる** — 変更前 (origin/main) と変更後で `EQ_NATIVE_PNG_DIR=<dir> EQ_NATIVE_FONT=<font> cargo test --release write_ -- --nocapture` (tests.rs:573・zoom_tests.rs:412 の書き出し。フォントの場所は既存のテスト・docs を参照) を流し、`shasum` が全部一致。
- [ ] **Step 6: Commit** — `feat: 配信に試験の設定 sub_map を足し、右パネルの上にサブの地図を描けるようにする (既定は無効)`

---

### Task 2: 計測のテスト `sub_map_cost`

**Files:**
- Modify: `crates/eq-server/src/broadcast/native/zoom_tests.rs` (`#[ignore]` の `fn sub_map_cost()`)

仕様:
- 場面: (平時、EEW 中、確定の地震) × (sub_map off/on) × (zoom off/on)。材料は `chiba_events()` (zoom_tests.rs:34-67) と `tests.rs` の `quake()`。平時は同じ events で now を落ち着きの時間より後ろに。
- 各 10 コマ温めたあと、`render()` 単体を 200 回と、`step()` を 5fps の時刻で 100 回、1 回ずつ `Instant` で計る。
- 出力 (1 場面 1 行): `{phase} zoom={z} sub={s}: render med {:.2} p95 {:.2} / step med {:.2} p95 {:.2} mean {:.2} ms, core-s per video-s {:.4}` (= step の平均 ms × 5 ÷ 1000)。可能なら `getrusage(RUSAGE_SELF)` のユーザー時間も (libc が依存に無ければ省く。依存は足さない)。
- 地図の置き場所は `EQ_NATIVE_MAP_DIR`、フォントは `EQ_NATIVE_FONT` で上書きできる (n2 で test バイナリを動かすため)。

- [ ] **Step 1: 実装する**
- [ ] **Step 2: Mac で流す** — Run: `EQ_NATIVE_FONT=<font> cargo test --release -p eq-server sub_map_cost -- --ignored --nocapture --test-threads=1` / Expected: 12 行の結果。結果を `docs/native-submap-bench.md` (新規) に貼る (Mac の機種名、日時、コマンド、結果、on/off の比の要約)。
- [ ] **Step 3: Commit** — `test: 配信のサブの地図の重さを測る sub_map_cost と、Mac での結果`

---

### Task 3: n2 での計測 (親が利用者と時間を決めてから)

- n2 では「描くだけ」を測る (ffmpeg を伴う broadcast_load.py・replay-video は使わない)。
- Mac で `cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.35 --tests --no-run -p eq-server` で test バイナリを作り scp。n2 の地図の場所 (current/web) とフォントを環境変数で渡す。
- `systemd-run --user --wait -p CPUQuota=5% -p CPUWeight=1 -p Nice=19 -p MemoryMax=150M -p MemorySwapMax=0 <test バイナリ> sub_map_cost --ignored --nocapture --test-threads=1`。値はコア秒 (systemd-run の "CPU time consumed" か getrusage) を正にする。
- 1 回 60 秒以内。01:00〜02:30 JST を避ける。流している間と後に `journalctl --user -u eq-broadcast` の `busy chip` と YouTube の健全性を見る。
- 結果を `docs/native-submap-bench.md` に足す。
