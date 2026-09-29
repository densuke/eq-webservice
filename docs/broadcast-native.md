# 配信の画面を Rust で描く (N 案) の仕様

`eq-server broadcast` の画面を、Chrome を使わずに Rust で描く。
`docs/broadcast-v2.md` の 3 章 (つなぎ目) の `FrameSource` の実装の 1 つで、e2 で直接配信する案 (4.1) の描画も兼ねる。
この文書は N1 (最初の試作) の仕様と、担当 (Sonnet) への指示をまとめたもの。

## 1. なぜやるか

- 今の配信 (Mac) は CPU 約 110% (Chrome 約 90%、ffmpeg 約 20%)、メモリ約 1.2GB。
  その大半は、Chrome がページを描き、JPEG にして渡す分。
- e2 の試作 (`tools/spike/e2render`) では、地図を tiny-skia で描くと Mac で 1 コマ 7ms。
  変化したときだけ描き直せば、描画の CPU は数 % で済む見込み。
- 同じ描画を Mac・Pi・e2 で使える。

## 2. N1 の範囲

作る (N1)

- 平時の画面: 日本地図、発表中の気象警報・注意報の塗り、主要都市の天気、時計、震度の凡例、地震の履歴、出典
- 地震の画面: 最新の地震 (地震情報) の都道府県ごとの震度の塗り、震度の数字、震央の印、詳細 (震源・規模・深さ・最大震度・発生時刻・津波の有無)
- 平時と地震の画面の切り替え (4 章の規則)
- mixer の BGM の流す・止める (平時なら流す)
- `broadcast.toml` の `source = "native"` で切り替える (既定は今までどおり `chrome`)

作らない (N2 以降)

- 緊急地震速報 (予測の塗り、P波・S波の円)、津波予報、揺れの報告、デモ
- 警戒音 (mixer の alert)。N2 で alert.ts の判断を移す
- 南西諸島・小笠原の別枠、バナー、流れる文字、細分区域での塗り (寄った表示)
- 天気の絵文字 (色付きの絵文字は描けないので、4.4 のとおり文字で出す)

## 3. 画面 (1280x720 固定)

今の Chrome の配信の見た目に寄せる。細部は合わせなくてよい。色は `web/public/style.css` の `:root` と同じにする。

```
y=0   ┌────────────────────────────────────────────────────────────────┐
      │ 地震情報マップ vX.Y.Z   [平時|地震]                BGM: 曲名    │ 上部バー 36px (背景 --panel #151b23、下線 --line #2a323d)
y=36  ├──────────────────────────────────────────────┬─────────────────┤
      │ 地図 (x 0..900, y 36..720)                    │ 右パネル        │ (x 900..1280、背景 --panel、左線 --line)
      │  背景 --sea #0a0f16、陸 --land #3a4250          │ ・詳細 (上)     │
      │  県境 --land-edge #0d1117                       │ ・履歴 (中)     │
      │  凡例 (左下、縦)            時計 (右下の枠)      │ ・出典 (下)     │
y=720 └──────────────────────────────────────────────┴─────────────────┘
```

### 3.1 地図

- 投影は `web/src/map.ts` と同じ: `x = (lon - 137) * cos(37°) * 100`、`y = -(lat - 37) * 100`。
  日本全体 (経度 128〜146.2、緯度 30〜45.8。`map.ts` の HOME) が地図の枠に収まるよう、縦横比を保って拡大・移動する。
- 地形は `web/public/japan.geojson` (都道府県。`properties.name` が都道府県名)。起動時に 1 回読み、塗りの path を作っておく。
- 平時の警報・注意報は `web/public/warning-areas.geojson` (市町村等。`properties.code` が 7 桁のコード) を `/api/warnings` の区域ごとに塗る。
  色は `web/src/map.css` の `.warn[data-level=...]` と同じ (注意報 #f2e700 50%、警報 #ff2800 65%、危険警報 #aa00ff 70%、特別警報 #0c000c 85%)。
  段階の判定は `web/src/warnings.ts` の `warningLevel` と同じ規則。
- 地震の画面では、都道府県を最大震度の色で塗る (`web/src/scale.ts` の COLORS)。
  塗った都道府県の中に震度の数字の札を出す (色は scaleColor / scaleTextColor)。数字の置き場所は、都道府県の本土の外接矩形の中心で N1 はよい。
- 震央は、赤い × (白い縁取り) を出す。

### 3.2 平時の天気 (`/api/weather`)

- 都市の点 (白) と、札 (明るい地 rgba(240,244,248,0.92)、暗い字 #0d1117) に「晴 24°」のように 1 文字の天気と気温を出す。
- 天気の 1 文字は、天気コードの百の位で決める (1 晴、2 曇、3 雨、4 雪)。
- 札の向きは `web/src/weather-layer.ts` の SIDE と同じ (神戸・東京は左、大阪・千葉は右、高知は下、ほかは上)。
- アメダスの雨の点は、`web/src/weather.ts` の rainColor の色で半径 2.5px の点にする。

### 3.3 右パネル

- 詳細 (上): 地震の画面のときは、最大震度の四角 (震度の色、数字)、震源名、発生時刻、「震源 名 / M / 深さ」、「津波 …」を今のページの詳細と同じ文言で出す。
  平時は、最新の地震 (最大 1 件) を同じ形で出す。
- 履歴 (中): 直近の地震を新しい順に最大 5 件。1 行目に震源名、2 行目に「MM/DD HH:MM M 規模」、左に最大震度の四角。
- 出典 (下、小さい字): 今のページの出典と同じ文言 (`web/public/index.html` の `.credit`)。

### 3.4 そのほか

- 時計: 地図の右下の枠 (今のページの `.clock-panel` に寄せる)。日付と「HH:MM」「秒」。JST。
- 凡例: 地図の左下に縦 (今のページの `.legend`)。
- 上部バー: 題、版、平時か地震か、BGM の曲名 (Icecast の `/stream/status-json.xsl` の title。15 秒ごと)。

## 4. 表示の判断 (純粋な関数にしてテストする)

### 4.1 データ

- 地震: `GET /ws` (WebSocket) の `hello` (直近のイベント) と `event` を受ける。形は `crates/eq-server/src/http.rs` の ServerMessage、
  イベントは `quake::Event` (serde でそのまま読める)。切れたら 5 秒後につなぎ直す。
- 警報: `GET /api/warnings` を 5 分ごと。天気: `GET /api/weather` を 5 分ごと。
- 取得先は `broadcast.toml` の `server` (既定 `https://eq.fuga.jp`)。

### 4.2 地震のまとめ方

- 地震情報 (kind = quake) を、同じ地震ごとにまとめる。同じ地震かは `web/src/priority.ts` の sameQuake と同じ
  (発生時刻の差が 90 秒以内かつ震央の距離が 200km 以内。震源が無ければ発生時刻だけ)。
- まとめた地震の最大震度・震源・規模・深さは、そのまとまりの最新の情報から取る。
  都道府県ごとの最大震度は、各地の震度 (points) の都道府県ごとの最大。

### 4.3 平時と地震の画面の切り替え

- 地震の画面にする条件: 最後の情報から `settleMs(最大震度)` 以内の地震がある。
  settleMs は `web/src/priority.ts` と同じ (震度 2 以下は 1 分、それ以外 3 分)。
- 複数あれば、最大震度の大きい方、同じなら新しい方を出す。
- それ以外は平時。平時の間は mixer に BGM を流す知らせ (音量 0.4) を出し、地震の画面の間は止める知らせを出す。
- 時刻は、`hello` と `event` の `server_time_ms` でサーバの時計に合わせる (ずれを覚えておく)。

### 4.4 描き直し

- 描き直すのは、データが変わったとき・平時と地震が切り替わったとき・時計の秒が進んだときだけ。
- `FrameSource` は、描き直していない間も前の画面を返す (fps は Broadcaster の時計が決める)。

## 5. 作り

- 置き場所: `crates/eq-server/src/broadcast/native/`
  - `mod.rs`: `NativeSource` (FrameSource の実装)。データ取得のタスクと、描画の呼び出し
  - `model.rs`: 表示の判断 (4 章)。純粋な関数だけ
  - `draw.rs`: 描画 (tiny-skia)。model の結果を受けて Pixmap に描く
  - `text.rs`: 文字 (フォントの読み込み、字形のキャッシュ、1 行を描く)
  - `geo.rs`: GeoJSON を読んで投影し、path にする
- 依存: `tiny-skia`、文字は `fontdue` (字形は一度描いたらキャッシュ)。ほかは既存のもの (tokio-tungstenite、reqwest、serde_json)。
- フォント: `broadcast.toml` の `font` で指定 (ttc なら `font_index`)。既定は
  - macOS: `/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc`
  - Linux: `/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc`
  フォントのファイルはリポジトリに入れない。
- 地図のデータ (geojson) は `web/dist` か `web/public` から読む (`broadcast.toml` の `map_dir`、既定 `web/public`)。
- ffmpeg への映像: native のときは RGBA の生データで渡す (`-f rawvideo -pix_fmt rgba -s 1280x720 -framerate <fps> -i -`)。
  Chrome のとき (JPEG) の引数は変えない。
- 既存の `session()` は、`source` によって Chrome と native を選ぶ。3 章の trait を無理に入れなくてよい
  (enum で分けるくらいでよい。W5 でつなぎ目を整える)。

## 6. テスト

必須 (cargo test)

- model: 平時→地震→平時の切り替え (settleMs の 1 分・3 分の境目)、同じ地震のまとめ (90 秒・200km)、
  複数あるときの選び方、都道府県ごとの最大震度、サーバの時計合わせ
- geo: 投影 (`map.ts` と同じ式。137°E 37°N が原点)
- draw: 決まったデータで 1 コマ描き、既知の位置の画素の色を確かめる
  - 平時: 東京都の中が警報の色 / 海の色
  - 地震: 震度 4 の県の中が震度 4 の色
  - フォントが無い環境 (CI) でも通るよう、文字を描かない設定で行う
- ffmpeg の引数: native のとき rawvideo になり、chrome のときは今までと同じ

手で確かめる (PR に結果を書く)

- Mac で `source = "native"`・`mixer = true` で 10 分、ファイルに出す (YouTube には送らない)
- 画面の見た目 (5 分ごとに 1 コマ PNG に書き出して PR に貼る)
- CPU: eq-server と ffmpeg の合計の平均 (Chrome は動かない)。目標は 40% 未満
- 音: 欠け 0、BGM が平時に流れる

## 7. 完了の条件

- 6 章のテストが通り、`cargo clippy --all-targets -- -D warnings`・`cargo fmt --check` が通る (pre-commit フック)
- `source` を指定しなければ、今までと同じ動き (Chrome) のまま
- PR (マージはしない) に、画面の PNG、CPU の数値、音の確認の数値、未対応のものの一覧を書く
- 1 ファイル 400 行程度まで、関数は 50 行程度まで
