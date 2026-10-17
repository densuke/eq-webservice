# 揺れの広がりの再現 (配信) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 観測点の値が入った地震情報 (最大震度 3 以上) が届いたら、配信の地図で発生時刻から P 波・S 波の円を描き直し、S 波の縁が届いた観測点から灯す再現を 1 回流す。

**Architecture:** 新しい状態は持たない。`native/spread.rs` の純粋な関数が、届いた報と今の時刻から「いま再現の最中か」と S 波の到達半径を決める。`step.rs` がその波を今の波に加え、`draw.rs` が観測点の点を到達半径で絞り、案内の文字を出す。still は地震の画面のあいだ毎秒描き直しているので、点の絞り込みは still の描き直しに乗せる。

**Tech Stack:** Rust (edition は crate の設定どおり)、tiny-skia。テストは `cargo test -p eq-server`。

**Spec:** `docs/superpowers/specs/2026-10-18-quake-spread-replay-design.md`

## 実装との差 (2026-10-18、実装とレビューのあと)

この計画のとおりでないところ。仕様書と docs/broadcast-native.md の 16 章は実装に合わせてある。

- 案内は本図の左上ではなく右上 (左上は南西諸島の別枠と重なった)。
- `look` は再現の波を数えない (数えると記録の描き直しで刻みの音が鳴る)。`look` の引数も変えていない。
- 終わりは流す報の観測点で決める。再現の窓の外では、`spread_of` は点の計算をせずに返す。
- 発生時刻の分からない地震情報では、再現を止めない。
- テスト `the_still_is_redrawn_while_the_dots_spread` は、中身に合わせて `the_frame_is_redrawn_every_tick_while_spreading` にした。

## Global Constraints

- 配信 (native) だけ。web (`web/src`) は変えない。
- 流す条件: まとまりの中で観測点 (`!is_area`) を初めて含んだ報。その時点の最大震度が `Scale::S3` 以上。続報では流し直さない。
- `LEAD_MS = 2_000`、`HOLD_MS = 5_000`。長さの上限は `eew::WAVE_MAX_MS` (180 秒)。速さは等倍。
- 円と点はどちらも `eew::surface_radius_km` と `eew::VP_KM_S`・`eew::VS_KM_S` を使う。深さが無ければ `eew::DEFAULT_DEPTH_KM`。
- 止める: 流す報より後に緊急地震速報、または `same_quake` でない地震情報が届いたとき。同じ地震の続報では止めない。
- 案内の文言は「揺れの広がり (再現)」。新しい色は作らない (既存の `TEXT`・`MUTED` を使う)。音・読み上げは鳴らさない (`look` 以外の音の判断に触れない)。
- 平時の画面と、再現の無い地震の画面は、今と画素単位で同じ。

## Review Focus

- 震源 (緯度・経度) が無い地震情報: 再現は流さない (点の距離が測れない)。→ Task 1 のテスト `no_epicenter_no_spread`。
- 観測点の名前が観測点の表に無い (点の位置が引けない) 報だけ: 一番遠い点が決まらないので流さない。→ Task 1 のテスト `no_located_dots_no_spread`。
- 流す報が届く前から同じ地震の緊急地震速報が出ていた: それは「後から届いた」ではないので止めない。→ Task 1 のテスト `an_eew_before_the_report_does_not_stop_it`。
- 再現の最中に、同じ地震で区域だけの報 (震度速報の訂正など) が届く: 止めない・流し直さない。→ Task 1 のテスト `follow_up_reports_neither_restart_nor_stop`。
- 再現の途中で still が描き直されない秒がない: still_key に再現の到達半径が効く形で入っているか。→ Task 2 のテスト `the_still_is_redrawn_while_the_dots_spread`。

---

### Task 1: 再現の判断 (`spread.rs`)

**Files:**
- Create: `crates/eq-server/src/broadcast/native/spread.rs`
- Modify: `crates/eq-server/src/broadcast/native/mod.rs` (`mod spread;` を足す)

**Interfaces:**
- Consumes: `model::{same_quake, distance_km, event_place, QuakeSummary, Place}`、`eew::{quake_place, surface_radius_km, Wave, VP_KM_S, VS_KM_S, DEFAULT_DEPTH_KM, WAVE_MAX_MS}`、`shaken::Zones::dots(&[Point]) -> Vec<(f64, f64, Scale)>` (経度, 緯度, 震度)、`crate::quake::{Event, EventBody, Scale}`。
- Produces:
  - `pub const LEAD_MS: u64 = 2_000;` `pub const HOLD_MS: u64 = 5_000;`
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub struct Spread { pub lat: f64, pub lon: f64, pub depth_km: f64, pub start_ms: u64, pub end_ms: u64 }` (`start_ms` は再現の発生時刻 0 秒 = 流す報の受信時刻 + `LEAD_MS`)
  - `pub fn spread_of(events: &[Event], current: &QuakeSummary, zones: &Zones, now_ms: u64) -> Option<Spread>`: 受信時刻 R ≦ now ≦ end_ms のときだけ Some。
  - `pub fn reach_km(s: &Spread, now_ms: u64) -> Option<f64>`: S 波の地表での半径。開始前・未到達は None (= 点を全部隠す)。
  - `pub fn wave(s: &Spread, now_ms: u64) -> Wave`
  - `pub fn lit(s: &Spread, now_ms: u64, lon: f64, lat: f64) -> bool`: 震央からの距離 ≦ `reach_km`。

- [ ] **Step 1: 失敗するテストを書く** (`spread.rs` の `#[cfg(test)] mod tests`)。報は `Event { received_at_ms, body: EventBody::Quake(..) }` を直接組む (`hindsight.rs:143` の作り方にならう)。観測点の表は `Zones::new(vec![], &[], stations)` で 2 点 (震央から約 30km と約 120km) を入れる。
  - `spreads_from_the_first_report_with_stations`: 震度速報 (区域だけ、R0) → 各地の震度 (観測点あり、R1、最大震度 3)。`spread_of(.., R1 - 1)` は None、`spread_of(.., R1)` は Some で `start_ms == R1 + LEAD_MS`。
  - `scale_two_does_not_spread`: 最大震度 2 では None。
  - `no_epicenter_no_spread`、`no_located_dots_no_spread`: どちらも None。
  - `dots_light_only_inside_the_s_wave`: `start_ms` の時点ではどちらの点も `lit == false`。30km の点は `start_ms + 9_000` (S 波 3.75km/s、深さ 10km で地表 約 31km) で true、120km の点はまだ false。`reach_km(s, s.start_ms - 1) == None`。
  - `ends_five_seconds_after_the_farthest_dot`: `end_ms == start_ms + (120km の点に S 波が届く ms) + HOLD_MS` (届く ms は `sqrt(d² + depth²) / VS_KM_S * 1000` を切り上げ)。`spread_of(.., end_ms + 1) == None`。上限: 非常に遠い点 (2000km) でも `end_ms <= start_ms + WAVE_MAX_MS as u64`。
  - `a_later_eew_or_another_quake_stops_it`: R1 の後に緊急地震速報、または震央が 500km 離れた地震情報が届いたら、その受信時刻以降は None。
  - `an_eew_before_the_report_does_not_stop_it`: R1 より前に届いた同じ地震の緊急地震速報があっても Some。
  - `follow_up_reports_neither_restart_nor_stop`: R1 の後に同じ地震の各地の震度の訂正 (R2) が届いても `start_ms == R1 + LEAD_MS` のまま Some。

- [ ] **Step 2: テストが失敗するのを確かめる**

Run: `cargo test -p eq-server spread::`
Expected: コンパイルエラー (`spread_of` などが無い)

- [ ] **Step 3: `spread.rs` を実装する**

`spread_of` の手順: `quake_place(current)` に緯度・経度が無ければ None。`events` を受信時刻の順に見て、`event_place` が `same_quake` の地震情報のうち、`!is_area` の点を初めて含む報の受信時刻 R を取る。R までに届いた同じ地震の報の最大震度が `Scale::S3` 未満なら None。R より後で now 以下に届いた緊急地震速報、または `same_quake` でない地震情報があれば None。`zones.dots(&current.points)` が空なら None。一番遠い点の距離から `end_ms` を出し、`start_ms + WAVE_MAX_MS` で頭打ちにする。深さは `current.hypocenter` の `depth_km`、無ければ `DEFAULT_DEPTH_KM`。`reach_km` は `surface_radius_km(VS_KM_S, depth, (now - start) 秒)`、`wave` は P 波・S 波の両方。

- [ ] **Step 4: テストが通るのを確かめる**

Run: `cargo test -p eq-server spread::`
Expected: すべて PASS

- [ ] **Step 5: コミット**

```bash
git add crates/eq-server/src/broadcast/native/spread.rs crates/eq-server/src/broadcast/native/mod.rs
git commit -m "feat: 揺れの広がりの再現の判断 (配信、純粋な関数)"
```

### Task 2: コマに再現の波を入れる (`step.rs`)

**Files:**
- Modify: `crates/eq-server/src/broadcast/native/step.rs` (`frame_waves`・`look`・`Stepper::step`)
- Modify: `crates/eq-server/src/broadcast/native/draw.rs` (`Scene` に `pub spread: Option<Spread>` を足す。既存の `Scene { .. }` を作っている所 (`mod.rs:356` 付近、`dots_bench.rs`、テスト) には `spread: None`)
- Test: `crates/eq-server/src/broadcast/native/step.rs` のテスト、または既存の `wave_layer_tests.rs`

**Interfaces:**
- Consumes: Task 1 の `spread_of`・`wave`・`reach_km`、`Renderer::zones() -> &Zones`。
- Produces: `Scene.spread: Option<Spread>` (Task 3 が使う)。

- [ ] **Step 1: 失敗するテストを書く**
  - `the_spread_wave_replaces_the_quake_wave`: Task 1 と同じ報 (最大震度 3、観測点あり) で `look(events, R1 + LEAD_MS + 5_000, None).waving == true`。同じ報で再現が終わった後 (`end_ms + 1`) は、地震情報の発生時刻から 180 秒を過ぎていれば `waving == false`。
  - `the_still_is_redrawn_while_the_dots_spread`: `Stepper::step` を `start_ms + 3_000` と `start_ms + 4_000` で呼ぶと、どちらも Some を返す (still が毎秒描き直される)。
  - 再現の無い地震 (最大震度 2) の画面は、今と同じコマ (既存の `the_same_state_gives_the_same_frame` が通り続けること)。

- [ ] **Step 2: テストが失敗するのを確かめる**

Run: `cargo test -p eq-server the_spread_wave_replaces_the_quake_wave the_still_is_redrawn_while_the_dots_spread`
Expected: FAIL (`waving == false`)

- [ ] **Step 3: 実装する**

`frame_waves` に `spread: Option<&Spread>` を足し、Some なら `spread::wave` を加え、同じ地震 (`same_quake`) の今の波は外す。`look` と `step` は、地震の画面の地震が `Current::Quake(q)` のときだけ `spread_of(i.events, q, self.renderer.zones(), now)` を求める (`look` は表を持たないので引数に `&Zones` を足し、呼び出し側から渡す)。サブの地図の波 (`sub_wave`) も、再現の最中は `spread::wave` にする。`still_key` はすでに `now / 1000` を含むので変えない (到達半径は秒ごとに still と一緒に変わる)。`Scene` の `spread` に渡す。

- [ ] **Step 4: テストが通るのを確かめる**

Run: `cargo test -p eq-server`
Expected: すべて PASS (既存のテストを含む)

- [ ] **Step 5: コミット**

```bash
git add crates/eq-server/src/broadcast/native/
git commit -m "feat: 配信のコマに揺れの広がりの再現の波を入れる"
```

### Task 3: 点の絞り込みと案内 (`draw.rs`)

**Files:**
- Modify: `crates/eq-server/src/broadcast/native/draw.rs` (`Shake::of`、案内の描画)
- Test: `crates/eq-server/src/broadcast/native/dots_tests.rs`

**Interfaces:**
- Consumes: Task 1 の `lit`、Task 2 の `Scene.spread`。
- Produces: なし。

- [ ] **Step 1: 失敗するテストを書く** (`dots_tests.rs`。既存の点のテストの組み方にならう)
  - `only_the_reached_dots_are_drawn_while_spreading`: 震央の近くと遠くに 1 点ずつの地震。`spread` の `start_ms` から近い点だけに届く時刻の `Scene` で描くと、近い点の画素は震度の色、遠い点の画素は県の塗りのまま。`spread: None` では両方とも震度の色。
  - `the_spread_label_is_drawn_only_while_spreading`: `spread` ありとなしで、本図の左上の案内の範囲の画素が違う。`spread: None` のコマは今と同じ。

- [ ] **Step 2: テストが失敗するのを確かめる**

Run: `cargo test -p eq-server only_the_reached_dots_are_drawn_while_spreading the_spread_label_is_drawn_only_while_spreading`
Expected: FAIL

- [ ] **Step 3: 実装する**

`Shake::of` の地震情報の枝で、`scene.spread` が Some なら `dots` を `spread::lit(s, scene.now_ms, lon, lat)` で絞る。案内は `draw_hindsight` と同じく本図だけ (`!frame.is_inset()`)・文字が描けるときだけ、`t.top()` に「揺れの広がり (再現)」を `TEXT` 色 13px で、本図の左上の内側 (左端 + 12、上端 + 22) に描く。

- [ ] **Step 4: テストが通るのを確かめる**

Run: `cargo test -p eq-server`
Expected: すべて PASS

- [ ] **Step 5: コミット**

```bash
git add crates/eq-server/src/broadcast/native/draw.rs crates/eq-server/src/broadcast/native/dots_tests.rs
git commit -m "feat: 再現の最中は S 波が届いた観測点だけを灯し、案内を出す"
```

### Task 4: 重さと見た目の確認、文書

**Files:**
- Modify: `crates/eq-server/src/broadcast/native/dots_bench.rs` (再現の最中の 1 コマを測る行を足す)
- Modify: `docs/broadcast-native.md` (再現の節を足す: 流す条件・止める条件・案内・仕様へのリンク)
- Modify: `README.md` (表示の表の 551 の行に「観測点の値が届いたとき (最大震度 3 以上) は、揺れの広がりを 1 回再現」を足す)

- [ ] **Step 1: `dots_bench` に再現の最中の計測を足す** (能登の記録の、観測点の最も多い報の `start_ms + 20_000` で `spread` ありの `Scene` を描く)。

- [ ] **Step 2: 測る**

Run: `EQ_NATIVE_FONT=<フォントのパス> EQ_DOTS_OUT=/tmp/dots cargo test --release -p eq-server station_dots_cost -- --ignored --nocapture`
Expected: 再現の最中の 1 コマの時間が、点ありの今のコマと同じ程度 (点を絞るだけなので増えない)。結果の数字をコミットの本文に書く。

- [ ] **Step 3: 目で確かめる**: 同じ実行で書き出した PNG (`/tmp/dots`) を開き、`start_ms + 5_000`・`+ 20_000`・`+ 60_000` で、点が S 波の円の内側だけにあり、案内が出ていることを確かめる。

- [ ] **Step 4: 文書を書き、全体を確かめる**

Run: `cargo fmt --all --check && cargo clippy -q --all-targets -- -D warnings && cargo test -q`
Expected: すべて通る

- [ ] **Step 5: コミット**

```bash
git add crates/eq-server/src/broadcast/native/dots_bench.rs docs/broadcast-native.md README.md
git commit -m "docs: 揺れの広がりの再現 (配信) の説明と、再現の最中の重さの計測"
```
