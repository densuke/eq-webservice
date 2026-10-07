# テストの速さ (debug) を測った記録

2026-10-07 に測った。コミット前のフック (`cargo test -q`、debug) が新しい worktree で 40〜60 分かかっていた件。

## 結論

- 遅さのほぼ全部は、サブの地図の 3 本のテスト (`zoom_tests.rs`) の `in_sub_rect()` が、**画素 1 つごとに** `Placed::builtin()` (組み込みの定義の JSON を読み直す) を呼んでいること。debug だと 1 本で 12〜85 分、release でも 1〜7 分かかる。
- `in_sub_rect()` の矩形を 1 度だけ求める形にすると (実験、この PR には入れていない)、debug 全体が 85 分から 3 分に縮む。
- 依存のクレートだけ opt-level 2 にする (この PR) と、描画のテストが 2〜4 倍速くなる (上の 3 本を除いた全体は 109 秒から 21 秒)。初回のビルドは約 16% 増える。

## 測り方

- 機械: Apple M4 (10 コア)。ほかの作業 (別の worktree のビルドとテスト) と同居していて、負荷平均が 12〜36 だった。**壁時計の秒数はぶれる**ので、CPU 時間 (user) も併記する。
- テストごとの時間は cargo-nextest で出した (`cargo install cargo-nextest --locked --no-default-features --features default-no-update`。既定の機能だと aws-lc-sys のリンクに失敗した)。
- 全体: `cargo nextest run -p eq-server --no-fail-fast` (668 本)。release は `--release` を付ける。
- 初回のビルド: `cargo clean` のあと `cargo test -p eq-server --no-run`。

## 全体の時間 (eq-server、668 本)

| 構成 | 全体 (壁) | user |
|---|---|---|
| debug、今まで | 5144 秒 (85.7 分) | 5410 秒 |
| debug、依存だけ opt-level 2 | 完走せず (サブの地図の 3 本が 3000 秒を超えても終わらず打ち切り) | |
| debug、今まで、遅い 3 本を除く | 109 秒 | 156 秒 |
| debug、依存だけ opt-level 2、遅い 3 本を除く | 21 秒 | 74 秒 |
| debug、今まで、`in_sub_rect` を直した実験 | 182 秒 | 166 秒 |
| debug、依存だけ opt-level 2、`in_sub_rect` を直した実験 | 31 秒 | 58 秒 |
| release、今まで | 408 秒 | |

初回のビルド (クリーンから、`--no-run`): debug 170 秒、依存を opt-level 2 にすると 197 秒 (+27 秒)。release は 184 秒。

## 上位 15 本 (秒)

「今まで」は debug、依存の最適化なし。「opt2」は依存だけ opt-level 2 (サブの地図の 3 本は打ち切りで不明)。「opt2+直し」は opt2 に `in_sub_rect` の実験を足したもの。

| テスト (broadcast:: を省く) | 今まで | opt2 | opt2+直し |
|---|---|---|---|
| native::zoom_tests::the_sub_map_shows_shaking_colors_during_a_confirmed_quake | 5133 | >3000 | 1.7 |
| native::zoom_tests::the_sub_map_paints_only_inside_its_rect | 2333 | >3000 | 1.3 |
| native::zoom_tests::the_sub_map_leaves_the_detail_alone_during_a_quake | 732 | 679 | 3.5 |
| native::zoom_tests::the_sub_map_does_not_add_redraws | 112 | 45 | 28 |
| native::zoom_tests::the_forecast_areas_of_the_eew_stay_in_the_view_after_the_scale_prompt | 109 | 29 | 24 |
| native::zoom_tests::a_scale_prompt_zooms_in_on_its_area_while_the_waves_are_still_drawn | 91 | 24 | 22 |
| builtin::h264::tests::every_frame_is_output_even_when_the_screen_changes_a_lot | 54 | 38 | 29 |
| native::zoom_tests::a_zoomed_picture_leaves_the_bar_and_the_side_panel_as_they_are | 30 | 10 | 12 |
| native::zoom_tests::the_view_zooms_in_on_the_epicenter_and_returns_to_the_whole_country_when_the_quake_screen_ends | 23 | 6 | 7 |
| native::zoom_tests::the_same_frame_times_give_the_same_pictures_regardless_of_the_wall_clock | 22 | 5 | 6 |
| native::zoom_tests::the_pending_hindsight_epicenter_is_zoomed_to_until_the_real_one_arrives | 8.5 | 3 | 5 |
| native::zoom_tests::zoom_off_keeps_the_whole_country | 6.1 | 3 | 6 |
| native::zoom_tests::the_sub_map_does_not_draw_when_calm | 3.4 | 0.7 | 1.5 |
| native::tests::the_stepper_skips_an_unchanged_frame_and_redraws_when_the_wave_moves | 3.2 | 1.4 | 1.7 |
| native::zoom_tests::an_observed_quake_zooms_on_the_shaken_area_not_on_the_prefecture_mainland | 2.3 | 0.9 | 1.5 |

(同居の負荷で 2 倍ほどぶれる。opt2 の欄の小さい値は負荷の軽い時間に測ったもの。)

## 直し方の案 (この PR では直さない)

1. **`zoom_tests.rs` の `in_sub_rect()`**: `Placed::builtin().unwrap().sub.unwrap()` を画素ごとに呼んでいる (画面 1280x720 なら 1 回の比較で 90 万回、JSON の解析)。矩形を `OnceLock` か引数で 1 度だけ求める。これだけで debug 全体が 85 分から 3 分 (実験の値)。呼び元は `the_sub_map_paints_only_inside_its_rect`、`the_sub_map_leaves_the_detail_alone_during_a_quake`、`the_sub_map_shows_shaking_colors_during_a_confirmed_quake`。`the_sub_map_leaves_the_detail_alone_during_a_quake` の `in_detail` は矩形を先に取っているので問題ない。
2. `the_sub_map_does_not_add_redraws`・`the_forecast_areas_of_the_eew_stay_in_the_view_after_the_scale_prompt`・`a_scale_prompt_zooms_in_on_its_area_while_the_waves_are_still_drawn`: 何十コマも進めている。確かめたいのは「描き直しが増えない」「寄りが戻る」ことなので、時刻を一気に進めて (`fast_forward` や now を飛ばす) 節目のコマだけ見れば、コマ数を 1/5 ほどにできる。
3. `builtin::h264::tests::every_frame_is_output_even_when_the_screen_changes_a_lot`: エンコードのコマ数を減らす (出力が全コマ出ることの確認は 数十コマで足りる)。画面の大きさを小さくできるならそれも。
4. `a_zoomed_picture_leaves_the_bar_and_the_side_panel_as_they_are`・`the_view_zooms_in_on_the_epicenter_and_returns...`・`the_same_frame_times_give_the_same_pictures...`: 各 10〜30 秒 (opt2 で 5〜10 秒)。1 と 2 と同じく、進めるコマを間引ける。
5. 新しい worktree の初回は、フックのビルドが長い。`CARGO_TARGET_DIR` を worktree 間で共有するか、`sccache` を使うと 170 秒の初回ビルドも縮められる (未検証)。
