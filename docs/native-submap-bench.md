# 配信 (native) のサブの地図の重さ

配信の右パネルの上に、表示中の地震 (無ければ最新の地震) へ寄せた 2 枚目の地図を描く試験の設定 `sub_map` を足し、`render()` と `step()` の重さを on/off で測った。計画は `docs/superpowers/plans/2026-10-06-native-submap-bench.md`。

## 試験の中身

- `BroadcastConfig::sub_map` (既定 false)。無効のとき、配信と再現動画の出力は変わらない (PNG の書き出し 9 枚の shasum が変更前と全部一致)。
- 描くもの: 海、陸と県境、県の塗り、震度の札、震央。細分区域の塗り・観測点の点・EEW の札は無い。
- 範囲は寄りと同じ `aim_of` -> `target_box` -> `fit_box`。ただし波は追わず (`origin_ms` を None にする)、動かない。表示中の地震が無ければ `history` の先頭 (最新の地震) を観測として描く。
- キャッシュは無い (毎回 `render()` のたびに描く)。上限を測るための実験で、`render()` は描き直しのたび (still_key が変わったとき) にしか呼ばれない。
- 描き直しの合図 `still_key` は変えていない (サブの範囲は地震で決まり、地震の情報は still_key に入っている)。

## 測り方

`sub_map_cost` (`#[ignore]`)。場面 3 つ x zoom x sub_map の 12 通りで、温めたあと `render()` 単体 200 回、`step()` 100 回 (5fps の時刻、`rev` 固定) を 1 回ずつ `Instant` で計る。

- calm: 最新の地震 (千葉県北東部 震度 4) から 10 分後 (落ち着きの時間を過ぎた平時)
- eew: 千葉県北東部沖の緊急地震速報、発生の 20 秒後から (波が動く)
- quake: 同じ地震の地震情報、受信の 10 秒後から (確定の地震)

`core-s per video-s` は step の平均 ms x 5 / 1000 (1 映像秒に使うコア秒。5fps、テストは単一スレッドなので壁時計 = CPU)。getrusage は libc が依存に無いので省いた。

## Mac の結果

- 機種: Apple M4 (`sysctl -n machdep.cpu.brand_string`)
- 日時: 2026-10-06 約 14:00 JST (2 回流して、ほぼ同じ値。下は 2 回目)
- コマンド:

```
EQ_NATIVE_FONT="/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc" \
  cargo test --release -p eq-server sub_map_cost -- --ignored --nocapture --test-threads=1
```

```
calm  zoom=false sub=false: render med 0.14 p95 0.16 / step med 0.00 p95 0.81 mean 0.16 ms, core-s per video-s 0.0008
calm  zoom=false sub=true : render med 1.12 p95 1.18 / step med 0.00 p95 1.77 mean 0.36 ms, core-s per video-s 0.0018
calm  zoom=true  sub=false: render med 0.14 p95 0.15 / step med 0.00 p95 0.80 mean 0.16 ms, core-s per video-s 0.0008
calm  zoom=true  sub=true : render med 1.12 p95 1.34 / step med 0.00 p95 1.78 mean 0.35 ms, core-s per video-s 0.0018
eew   zoom=false sub=false: render med 0.29 p95 0.31 / step med 0.84 p95 1.17 mean 0.89 ms, core-s per video-s 0.0044
eew   zoom=false sub=true : render med 1.91 p95 2.05 / step med 0.81 p95 2.78 mean 1.19 ms, core-s per video-s 0.0060
eew   zoom=true  sub=false: render med 3.18 p95 3.28 / step med 5.12 p95 5.26 mean 4.14 ms, core-s per video-s 0.0207
eew   zoom=true  sub=true : render med 4.90 p95 5.16 / step med 6.80 p95 7.07 mean 5.32 ms, core-s per video-s 0.0266
quake zoom=false sub=false: render med 0.19 p95 0.21 / step med 1.10 p95 1.38 mean 1.14 ms, core-s per video-s 0.0057
quake zoom=false sub=true : render med 1.18 p95 1.28 / step med 1.09 p95 2.31 mean 1.31 ms, core-s per video-s 0.0065
quake zoom=true  sub=false: render med 2.54 p95 2.73 / step med 2.44 p95 5.04 mean 2.95 ms, core-s per video-s 0.0147
quake zoom=true  sub=true : render med 3.55 p95 4.58 / step med 2.46 p95 6.31 mean 3.29 ms, core-s per video-s 0.0165
```

### on/off の差 (sub=true - sub=false)

| 場面 | zoom | render | step 平均 | 1 映像秒あたりのコア秒 |
|---|---|---|---|---|
| calm | off | +0.98 ms (0.14 -> 1.12、約 8 倍) | +0.20 ms | +0.0010 |
| calm | on | +0.98 ms | +0.19 ms | +0.0010 |
| eew | off | +1.62 ms (0.29 -> 1.91、約 6.6 倍) | +0.30 ms | +0.0016 (0.0044 -> 0.0060) |
| eew | on | +1.72 ms (3.18 -> 4.90、約 1.5 倍) | +1.18 ms | +0.0059 (0.0207 -> 0.0266) |
| quake | off | +0.99 ms (0.19 -> 1.18、約 6 倍) | +0.17 ms | +0.0008 |
| quake | on | +1.01 ms (2.54 -> 3.55、約 1.4 倍) | +0.34 ms | +0.0018 |

要約:

- サブの地図は `render()` 1 回あたり約 1.0 〜 1.7 ms を足す (県の陸と県境を、もう一度全部塗るのが主)。EEW は波が動く間の震央と県の塗りが加わって大きめ。
- 描き直しは 1 秒に 1 回 (5fps) なので、zoom=off ではどの場面でも 1 映像秒あたり +0.001 〜 +0.002 コア秒 (コア 0.1 〜 0.2 %)。無視できる。
- zoom=on の EEW だけは、表示範囲が動く間毎コマ `render()` が走るので、サブも毎コマ描かれて +0.006 コア秒 (コア 0.6 %)。全体は 0.027 コア秒 / 映像秒。
- p95 は、1 秒に 1 回の描き直しのコマで跳ねる (step p95 で +1 〜 2 ms)。5fps なら 1 コマ 200 ms のうち 1 〜 8 ms で、間に合わなくなる水準ではない。
- Mac (M4) の値なので、n2 (専用だが遅い CPU) の倍率は Task 3 で測る。n2 は Mac より数倍〜十数倍遅い前提で見積もること。

## n2 での計測 (2026-10-06 14:04〜14:09 JST)

- 機械: n2 (e2-micro 相当、2 vCPU は 1 コアのハイパースレッド)。配信 (eq-broadcast、fps 5 / fps_calm 2、zoom なし) を流したまま。
- 手順: Mac で `cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.35 --tests -p eq-server` で test バイナリを作って n2 へ送り、次で流した。

```
systemd-run --user --wait --collect -p CPUQuota=25% -p CPUWeight=1 -p Nice=19 -p MemoryMax=150M -p MemorySwapMax=0 -p RuntimeMaxSec=300 \
  -E EQ_NATIVE_MAP_DIR=$HOME/work/eq-webservice/current/web/dist -E EQ_NATIVE_FONT=/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
  /tmp/eq-subbench sub_map_cost --ignored --nocapture --test-threads=1
```

- 5 分の上限で打ち切られ、12 場面のうち 7 場面まで出た (CPU 時間 62 秒、メモリの最大 67MB)。
- CPUQuota で絞っているので、平均と p95 は待たされた時間を含み、意味が無い。比べられるのは中央値 (絞られなかったコマ) だけ。

```
calm  zoom=false sub=false: render med 1.21 p95 68.39 / step med 0.00 p95 6.73 mean 15.31 ms
calm  zoom=false sub=true : render med 5.14 p95 985.45 / step med 0.00 p95 100.47 mean 36.84 ms
calm  zoom=true  sub=false: render med 1.20 p95 96.61 / step med 0.00 p95 94.64 mean 37.41 ms
calm  zoom=true  sub=true : render med 5.37 p95 988.35 / step med 0.00 p95 237.72 mean 53.81 ms
eew   zoom=false sub=false: render med 2.03 p95 229.26 / step med 5.54 p95 902.16 mean 112.30 ms
eew   zoom=false sub=true : render med 11.09 p95 1206.46 / step med 7.02 p95 900.62 mean 147.44 ms
eew   zoom=true  sub=false: render med 85.87 p95 998.52 / step med 222.40 p95 1037.05 mean 380.27 ms
```

要約 (中央値で):

- サブの地図は n2 で `render()` 1 回あたり平時 +3.9 ms、EEW 中 +9.1 ms (Mac の約 4〜6 倍)。
- n2 の配信は zoom なしなので描き直しは 1 秒に 1 回。増えるのは 1 秒あたり 4〜9 ms = コアの 0.4〜0.9 %。
- 計測の間 (14:04〜14:10) に配信の `busy chip` は 0 回。
- zoom=on の EEW (85.9 ms/回、毎コマ) は、サブの地図が無くても n2 では重い。配信で zoom を有効にする話は別に考える。
