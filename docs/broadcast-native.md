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

- 津波予報、揺れの報告、デモ (緊急地震速報の予測の塗りと P波・S波の円は R3.1 で足した。docs/replay-video.md の 3 章)
- 警戒音 (mixer の alert)。N2 で alert.ts の判断を移す
- 南西諸島・小笠原の別枠、バナー、流れる文字、細分区域での塗り (寄った表示)
- 天気の絵文字 (色付きの絵文字は描けないので、3.2 のとおり文字で出す)

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
- ffmpeg への映像: native のときは I420 の生データで渡す (`-f rawvideo -pix_fmt yuv420p -s 1280x720 -framerate <fps> -i -`。N1 は RGBA だったが、8.2 で変えた)。
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

## 8. N1.1 (N1 の改善。2026-09-30 に本番の配信を native に切り替えたあとの残り)

### 8.1 天気のアイコン (気象庁の天気アイコン)

- 平時の天気の札は、今の漢字 1 文字 (晴・曇・雨・雪) をやめ、気象庁の天気予報の SVG アイコンを出す。
- 天気コード → アイコンの対応は、気象庁の天気予報のページ (`https://www.jma.go.jp/bosai/forecast/`) の中の `TELOPS` の表に従う。
  - 形: `コード: ["昼の svg", "夜の svg", "代表コード", "日本語", "英語"]`。例: `100: ["100.svg", "500.svg", "100", "晴", "CLEAR"]`。118 通り。
  - 表は起動のたびに取らず、`tools/jma_telops.py` でページから抜き出して `native/telops.rs` (コード → (昼, 夜) の表) を作り、リポジトリに入れる。
    ツールの使い方をファイルの頭に書く。
- アイコンの SVG は、実行時に `https://www.jma.go.jp/bosai/forecast/img/<名前>` から取る。
  - 描くのは `resvg` (Rust 製、tiny-skia の上で動く)。
  - 1 度描いたアイコンは、名前ごとに Pixmap で覚えておく (描き直しのたびに SVG を描かない)。
  - 取れない・描けないときは、今の漢字 1 文字に戻す。
- 夜 (18 時〜翌 6 時) は夜のアイコンにする (今のページの weatherIcon と同じ判断)。
- 大きさは札の中で高さ 18px 程度。札の幅はアイコン + 気温に合わせる。
- 利用条件: 気象庁ホームページのコンテンツ利用条件 (公共データ利用規約 第1.0版) に従い、出典を示す。
  天気のアイコンはシンボルマーク・ロゴにあたらない。
  右パネルの出典の最後に「天気アイコン: 気象庁ホームページ (https://www.jma.go.jp/bosai/forecast/) を加工」を足す。
  README の出典にも同じ内容を足す。

### 8.2 ffmpeg の負荷を下げる

- 今は RGBA (1280x720x4 = 3.7MB) を 1 秒に 30 回 ffmpeg に渡し、ffmpeg が毎回色を変換している。そのため ffmpeg は約 28%。
- 描き直したときだけ、Rust 側で YUV 4:2:0 (I420、1.4MB) に変換する。ffmpeg には `-pix_fmt yuv420p` の rawvideo で渡す (同じ画面は変換済みのものを繰り返し渡す)。
- 変換は整数の演算で書き、BT.601 の限定範囲 (ffmpeg の既定の変換と同じ) にする。単体テストで、決まった色 (白・黒・赤・海の色) の Y/U/V の値を確かめる。
- 目標: 同じ条件で ffmpeg と eq-server の合計が 25% 未満 (N1 は約 38%)。

### 8.3 南西諸島の別枠

- 今のページと同じく、地図の左上に「南西諸島」の別枠 (経度 122.9〜131.4、緯度 24.0〜30.0、高さ 150px、背景は海の色、枠線 #3a4452) を出す。
- 別枠の中にも、都道府県の塗り (警報・震度)、天気の札 (那覇)、雨の点を同じように描く。
- 凡例と重ならないようにする (凡例は左下なので重ならないはず)。

### 8.4 完了の条件 (N1.1)

- 6 章と同じ種類のテスト (アイコンの表の引き方、夜のアイコン、I420 の変換、別枠の投影) を足す
- (8.5 のテストも足す)
- 手で確かめる: ファイル出力 10 分で CPU (目標 25% 未満)、平時の画面の PNG (アイコン・別枠が出ていること)
- 利用者の配信 (`~/work/eq-broadcast`) には触らない。PR まで作り、マージしない

### 8.5 周りの国の陸地

- 周りの国の陸地 (朝鮮半島・中国・ロシアなど) を背景に描く。データは `map_dir` の `neighbors.geojson` (今のページの `web/src/map.ts` の loadNeighbors)。
  スタイルは `web/src/map.css` の `.neighbor`: 塗り #20262f、線 #2b323d 0.6px、fill-rule evenodd。
- 日本の都道府県より下に描く。起動時に 1 回 path にして、地図の下地のキャッシュに含める (描き直しのたびには作らない)。南西諸島の別枠の中にも同じく描く。
- ファイルが無ければ描かずに続ける (警告を出す)。
- テスト: ソウル付近の画素が `.neighbor` の色になること (日本の県はその上に描かれること)、ファイルが無くても動くこと。

## 9. N1.2 (メモリを減らす。e2 で BGM 無しの直接配信を試すための前提)

### 9.1 背景 (2026-09-30 の e2 での試験)

- native・5fps・ultrafast・BGM 無しで e2 (e2-micro) で動かしたところ、eq-server と ffmpeg が `MemoryMax=400M` に張り付いた。
  e2 全体がスワップを使い、本番の eq-server の応答が 0.05 秒から 0.9〜3.6 秒に落ち、BGM の送り出しも Icecast に切られた。
- 配信の側も追いつかず (5fps のはずが 1.6fps、実時間の 1/3)。CPU は 20% の枠の中 (6〜16%) だった。
- Mac で測った eq-server の起動時の最大メモリ: 日本語フォントあり 234MB、なし 120MB。落ち着くと約 30〜40MB。
  - fontdue が CJK フォントの全字形 (数万字) を最初に前処理する (約 110MB)
  - GeoJSON (特に `warning-areas.geojson` 838KB) を `serde_json::Value` に展開してから path にしている (起動時の山)

### 9.2 やること

- 文字: fontdue をやめ、字形を必要なときに 1 字ずつ読む `ab_glyph` にする。フォントのファイルは読み込んだまま (または mmap) 持ってよい。
  描いた字形のキャッシュ (text.rs) の形は今のまま。
- GeoJSON: `serde_json::Value` を使わず、必要な形だけの型 (`FeatureCollection { features: Vec<Feature> }`、座標は `Vec<Vec<Vec<[f64; 2]>>>` など) で読み、
  path を作ったら元の座標は捨てる。読み終えたら読み込んだ文字列も捨てる。
- ffmpeg の引数に `-threads` を指定できるようにする (`encode` に書けるので、コードの変更は要らなければしない)。
- 目標: eq-server の起動時の最大メモリ 100MB 以下、落ち着いたとき 40MB 以下 (日本語フォントあり)。

### 9.3 e2 での試験の手順 (担当は Mac まで。e2 での試験はコーディネーターが行う)

- 一時的なユニット: `systemd-run --user -p CPUQuota=20% -p Nice=19 -p MemoryMax=200M -p MemorySwapMax=0`
- 出力は捨てる (`-f null`)、`-progress` で速さを見る。本番の応答時間 (`/api/weather`) と BGM の送り出しのログを 1 分ごとに見て、影響が出たらすぐ止める。
- e2 の Claude Code のセッション (約 250MB) は、試験の間は止めてもらう (利用者に頼む)。

### 9.4 完了の条件 (N1.2)

- Mac で `/usr/bin/time -l` の最大メモリ (maximum resident set size) を、日本語フォントあり・なしで測り、PR に書く (目標は 9.2)
- 画面が N1.1 と変わらないこと (平時の PNG を比べる)
- 既存のテストが通り、GeoJSON の読み取りと文字の描画のテストを必要なら足す
- 利用者の配信 (`~/work/eq-broadcast`) には触らない。PR まで作り、マージしない

## 10. N1.3 (e2 で BGM 無しの常時運用に向けた改善)

### 10.1 背景 (2026-09-30 の e2 での 15 分試験、v0.15.2)

- native・無音・5fps・ultrafast・ABR 500k・AAC 128k・`-threads 1`、`CPUQuota=25%`・`MemoryMax=200M`・`MemorySwapMax=0`。
- 平均 CPU 9.1%、メモリは落ち着いて 82〜85MB だが起動時の最大 190MB (上限の近く)。4661 コマを 5fps・実時間どおり、警告 0。
  本番の応答 0.001 秒前後、BGM の送り出しのエラー 0。送信量 約 508kbps (月約 160GB)。
- 平時しか測れていない (地震の画面は未計測)。

### 10.2 可変 fps

- `broadcast.toml` に `fps_calm` (平時のコマ数) を足す。今の `fps` は地震の画面のときのコマ数とする。`fps_calm` を省けば今までどおり一定 (`fps`)。
- 描く側は、平時か地震か (model の判断) に合わせて、ffmpeg に渡す間隔を変える。切り替えはすぐ反映する。
- ffmpeg の入力は、届いた時刻をコマの時刻にする: `-use_wallclock_as_timestamps 1 -f rawvideo -pix_fmt yuv420p -s WxH -i -`
  (native のときだけ。Chrome の経路は変えない)。出力は `-fps_mode passthrough`。
- キーフレームは時刻で 2 秒ごと: `-g` の代わりに `-force_key_frames expr:gte(t,n_forced*2)` (YouTube は 4 秒以内を求める)。
- 例: Mac・Pi は `fps = 15`・`fps_calm = 2`、e2 は `fps = 10`・`fps_calm = 2`。

### 10.3 送る量を減らす

- 音のビットレートを設定で決める (`audio_bitrate`、既定は今の `128k`)。無音で配信するなら `32k`。
- 映像は `encode` で CRF と上限を指定できることを確かめ、設定例に書く:
  `["-threads", "1", "-c:v", "libx264", "-preset", "ultrafast", "-tune", "stillimage", "-crf", "28", "-maxrate", "500k", "-bufsize", "1000k"]`
  (**可変 fps では -maxrate を付けてはいけない**。13 章)
- 平時の送信量を測って PR に書く (目標: 平時 250kbps 未満、音込み)。

### 10.4 配信元を画面に出す (label)

- `broadcast.toml` の `label` (例 `"配信元: e2"`) を、上部バーの右側に出す。BGM の曲名があるときは、その左に並べる。
- 省けば今までどおり (何も出さない)。

### 10.5 起動時のメモリの山を下げる

- e2 では起動時に eq-server と ffmpeg の合計が 190MB まで上がった。どちらが山を作っているか (eq-server の地図の読み込み、ffmpeg (x264) の先読みなど) を Linux 相当の条件で測る。
- ffmpeg なら `-rc-lookahead`・`-tune zerolatency` など、x264 の先読みを減らす設定を試し、画質と送信量への影響も見る。
- 目標: 起動時の最大 (eq-server と ffmpeg の合計) 150MB 未満。

### 10.6 地震の画面の負荷の試験

- 手元で、記録の地震を流すサーバ (`[source] type = "replay"`、`samples/scenarios/*.jsonl`、`rebase_time = true`) を立て、native の `server` をそこに向ける。
  noto2024 (大きい地震と多くの情報) と standard を使う。
- 地震の画面の間の CPU・メモリ・コマの速さ・送信量を、Mac で測って PR に書く。e2 での試験はコーディネーターが行う。
- 手順を `docs/broadcast-native.md` の付録か `tools/` のスクリプトに残し、e2 でも同じ手順で測れるようにする。

### 10.7 完了の条件 (N1.3)

- 可変 fps: 平時と地震の画面の切り替えで、コマの間隔が変わること (ffprobe でコマの時刻を見て確かめる)、キーフレームが約 2 秒ごとに入ること
- 送信量・起動時のメモリ・地震の画面の負荷の数値を PR に書く
- 既存のテストが通り、ffmpeg の引数 (可変 fps のとき・`fps_calm` を省いたとき・Chrome のとき) のテストを足す
- 利用者の配信 (`~/work/eq-broadcast`) と e2 には触らない。YouTube には送らない。PR まで作り、マージしない

## 11. N1.3 の結果と測り方 (v0.16.0)

### 11.1 実装したこと

- `fps_calm` (native のみ): 指定すると ffmpeg の入力は `-use_wallclock_as_timestamps 1`、出力は `-fps_mode passthrough` と `-force_key_frames expr:gte(t,n_forced*2)`。
  描く側の「平時か」の知らせ (watch) で、送る間隔をすぐ切り替える (平時は `fps_calm`、地震の画面は `fps`)。
  `fps_calm` を省くと今までどおり (`-framerate fps`、`-g fps*2`)。Chrome の経路は変えていない。
- `audio_bitrate` (既定 `128k`)、`label` (上部バーの右。BGM の曲名はその左)。
- 起動時のメモリの山は、eq-server ではなく ffmpeg (x264) が作っていた。`encode` の設定だけで下げられるので、コードは変えていない。

### 11.2 測った数値 (Mac、Apple Silicon、`tools/broadcast_load.py`。e2 とは数値が違う)

設定: `fps = 10`・`fps_calm = 2`・`audio_bitrate = "32k"`・
`encode = ["-threads","1","-c:v","libx264","-preset","veryfast","-tune","zerolatency","-crf","28","-maxrate","500k","-bufsize","1000k"]`。CPU は 1 コアを 100% とする。
**この数値は -maxrate 付きで測ったもので、平時の画面が崩れていた (13 章)。送信量は CRF だけの設定で測り直す前提で読むこと (e2 の本番は CRF 23 で平時 約 216kbps)。**

| 場面 | CPU (eq-server + ffmpeg) | メモリ (eq-server / ffmpeg) | コマ | 送信量 (音込み) |
|---|---|---|---|---|
| 平時 (eq.fuga.jp の実データ) | 約 2% | 約 50MB / 約 48MB | 2fps | 約 76kbps |
| 地震の画面: noto2024 (100 秒、4 倍速) | 平時 約 2%、地震 約 4.4% | 41MB / 49MB | 2 → 10fps | 平均 約 175kbps |
| 地震の画面: standard (100 秒) | 平時 約 1.5%、地震 約 3.5% | 38MB / 48MB | 2 → 10fps | 平均 約 182kbps |

- 切り替わりは 5 秒ごとの表で 1.6〜2.0fps → 10.0fps と出る。キーフレームの間隔は 1.6〜2.4 秒 (ffprobe のパケットの旗で数えた)。
- 平時の送信量は encode で大きく変わる (音 32k 込み、同じ CRF 28、`-tune stillimage`): ultrafast 約 410kbps、veryfast 約 250kbps、`zerolatency` (veryfast) 約 76kbps。
  目標 (平時 250kbps 未満) は `veryfast` + `zerolatency` で満たす。文字は読めるが、天気アイコンは少しぼやける (CRF 28)。
- 起動時のメモリ (Mac、`/usr/bin/time -l` は eq-server、ffmpeg は 0.1 秒ごとの標本):
  - eq-server の最大 約 40MB (日本語フォントあり)。山を作っているのは ffmpeg だった。
  - ffmpeg の最大: v0.15.2 の既定の encode (`veryfast`・`-b:v 3000k`・`-threads` なし) で 約 224MB、`-threads 1` で 約 94MB、さらに `-rc-lookahead 5` で 約 79MB、`-tune zerolatency` で 約 48MB (ultrafast + stillimage は 約 45MB)。
  - 合計 (eq-server + ffmpeg) の最大は上の設定で 約 90〜98MB (目標 150MB 未満)。e2 (Linux) は ffmpeg のスレッドや libc が違うので、数値は e2 で測り直す。
- `-tune stillimage` (veryfast) は先読み (`rc-lookahead` 40) のため、出力が 20 秒ほど遅れる。`zerolatency` は先読みが無く遅れない。
- 既知 (N1.3 の前から): 起動の直後に ffmpeg が「音の入力が数秒〜15 秒遅れた」という警告を出す (無音の `-re` の入力が、最初の映像を待つため)。少しあとに追いつく。

- **e2 での実測 (2026-09-30、本番と同じ枠 CPUQuota 25%・MemoryMax 200M、noto2024 speed 4 の 100 秒、CRF 23 だけ・`-threads 1`・veryfast・zerolatency)**
  - 平時 2fps、地震 9〜10fps。キーフレームの間隔 1.6〜2.4 秒。
  - CPU 平均 13.7% (eq-server 3.5% + ffmpeg 10.3%)。
  - 最大メモリ 126MB (eq-server 49 + ffmpeg 77)。
  - 送信量 平均 251kbps (音込み)。
  - 常時の配信 (平時) は CPU 約 7%、送信量 約 200kbps。

### 11.3 地震の画面の負荷の測り方

```sh
cargo build --release
# 記録の地震を流すサーバ (別のポート 18099) を立て、native の server をそこへ向ける。出力は一時ディレクトリのファイルだけ
tools/broadcast_load.py samples/scenarios/noto2024.jsonl --speed 4 --duration 100 --time-l \
  --toml 'fps = 10' --toml 'fps_calm = 2' --toml 'audio_bitrate = "32k"' \
  --toml 'encode = ["-threads","1","-c:v","libx264","-preset","veryfast","-tune","zerolatency","-crf","23"]'
# 平時 (実データ。読むだけ): replay を立てずに eq.fuga.jp の画面を測る
tools/broadcast_load.py x --server https://eq.fuga.jp --duration 90 --toml 'fps = 10' --toml 'fps_calm = 2'
```

- 5 秒ごとの表: CPU (eq-server・ffmpeg)・メモリ・コマ数・映像の送信量。最後に平均の CPU・最大メモリ・送信量・キーフレームの間隔を出す。
- Linux (e2) でも動く (CPU とメモリは /proc から読む)。e2 では `systemd-run --user -p CPUQuota=25% -p MemoryMax=200M -p MemorySwapMax=0 tools/broadcast_load.py ...` のように包むと、本番と同じ枠で測れる。
- 記録は最初に緊急地震速報が続く。R3.1 から native も描くので、最初から地震の画面 (波が動くので 10fps) になる (それより前の版は、地震情報 (震度) が来てから)。
- 送信量は、エンコーダが先読みで抱えているコマを除いて、出力の長さで割っている。

## 12. N1.4 (テスト配信の表示と安全装置、e2 の常時運用の置き方)

### 12.1 背景

- 2026-09-30 11:10 から、e2 (e2-micro) から YouTube へ直接配信している (native・無音・可変 fps 10/2)。
  地震の画面の負荷は、公開しない試験 (noto2024) で CPU 約 15%、メモリ最大 127MB、送信 約 200kbps と分かった。
- YouTube で地震の画面の見た目を確かめたいが、記録の地震 (replay) を本番の配信に流すと、実際の地震と取り違えられるおそれがある。

### 12.2 テスト配信の表示 (`test = true`)

- `broadcast.toml` に `test` (既定 false) を足す。true のとき native は次を必ず描く (消す設定は作らない)。
  - 上部バーのすぐ下と画面の最下部に、赤い帯 (背景 #b3001b、白い太字): 「テスト配信: 過去の地震の再生です。実際の地震ではありません」
  - 地図の中央に、うすい (不透明度 15% 程度) 大きな「TEST」の透かし
  - 上部バーの題の横に「[テスト]」
- 文字が描けない (フォントが無い) ときも、赤い帯と透かしの形だけは描く。

### 12.3 安全装置

- データの取得先 (`server`) が replay (記録を流すサーバ) のときは、`test = true` が無ければ配信を始めずにエラーで止める。
  - replay かどうかは、eq-server が返すもので判断する。eq-server に `GET /api/source` (`{"type": "p2pquake" | "replay"}`) を足し、native は起動時にそれを見る。
    取れない (古いサーバ) ときは、replay ではないとみなす。
- `test = true` のときは、送り先 (`output`) に本番と同じストリームキーの環境変数を使っていないかは判断できないので、文書と設定例で「テスト用の別の配信 (限定公開) のキーを使う」と強く書く。

### 12.4 e2 の常時運用の置き方をリポジトリに残す

- `deploy/eq-broadcast.service` (e2 に置いたユーザーユニット。CPUQuota 25%・MemoryMax 200M・MemorySwapMax 0・Nice 19・Restart always) と、
  e2 向けの `broadcast.toml` の例 (`deploy/broadcast.e2.toml`) を足す。README の配信の節に、置き方 (フォントと ffmpeg を apt で入れる、地図のデータを置く、キーの置き場所と権限) を書く。
- 置いてある実物 (e2 の `~/.config/systemd/user/eq-broadcast.service`) は下のとおり。

```ini
# eq-server broadcast: e2 から YouTube へ直接配信する (native・無音)。本番の eq-server を守るため CPU とメモリを縛る
[Unit]
Description=eq-webservice ライブ配信 (eq-server broadcast -> YouTube)
After=eq-server.service network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=%h/work/eq-e2cast
ExecStart=%h/work/eq-e2cast/eq-server broadcast cast.toml
# ストリームキー (YOUTUBE_LIVE_API_KEY=...、権限 600)
EnvironmentFile=%h/.config/youtube-live-eq-webservice
Environment=RUST_LOG=info
Restart=always
RestartSec=10
Nice=19
CPUQuota=25%
MemoryMax=200M
MemorySwapMax=0

[Install]
WantedBy=default.target
```

### 12.5 完了の条件 (N1.4)

- `test = true` の帯と透かしが出ること (PNG で確認)、`test` を省いた画面は今と変わらないこと
- replay のサーバに向けて `test` 無しで起動するとエラーで止まり、`test = true` なら動くこと (テストか手での確認)
- `/api/source` の単体テスト
- 利用者の配信 (e2 の eq-broadcast、`~/work/eq-broadcast`) には触らない。YouTube には送らない。PR まで作り、マージしない

## 13. 可変 fps では -maxrate を付けない (v0.16.1、e2 の本番で見つかった)

- 症状: `fps_calm` を指定した配信で、`encode` に `-maxrate` / `-bufsize` を付けると、平時 (2fps) の画面がブロック状に崩れる。CRF をいくつにしても送信量が同じ (約 127kbps) になる。
- 原因: 可変 fps は入力を `-use_wallclock_as_timestamps 1` で渡すので、ffmpeg は入力を 1 秒 25 コマとみなす。x264 の VBV は 1 コマあたり maxrate/25 (500k なら 20kbit) に絞るため、実際は 1 秒 2 コマしか来なくても、1 コマに使える量が足りない。
- 対処: 可変 fps の `encode` は CRF だけにする。e2 の本番は `["-threads", "1", "-c:v", "libx264", "-preset", "veryfast", "-tune", "zerolatency", "-crf", "23"]` で、平時の送信量は約 216kbps・文字はくっきり。
- コード: `encode` を省いた (既定のままの) とき、`fps_calm` があれば既定を `["-c:v", "libx264", "-preset", "veryfast", "-crf", "23"]` にする (Mac・Pi は上限つきの既定と組み合わせると同じ不具合になるため)。
  一定の fps (fps_calm を省く) の既定は今までどおり (`-b:v 3000k -maxrate 3000k`。25 でなく実際の fps で入るので問題ない)。
  `encode` を明示して `-maxrate` を含めた場合は、そのまま使い、起動時に警告を出す。
