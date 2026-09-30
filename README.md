# eq-webservice

[P2P地震情報](https://www.p2pquake.net/) の WebSocket API から地震情報を受け取り、
日本地図の上に **震央・P波/S波の広がり・都道府県ごとの震度** をほぼリアルタイムで描くツールです。
受け取った情報は、RSS・Discord などの配信先（プラグイン）にも流せます。

```
P2P地震情報 (wss) ──┐
                     ├─▶ eq-server (Rust) ──▶ ブラウザ (静的ページ + TypeScript, WebSocket)
Wolfx EEW (wss, 任意)─┘
                           │
                           └─▶ プラグイン: RSS / Discord / JSON Lines 蓄積 / 汎用 Webhook
```

## 構成

| パス | 内容 |
| --- | --- |
| `crates/eq-server` | 上流への接続（再接続付き）、重複排除、ブラウザ向け WebSocket、静的ファイル配信、プラグイン |
| `web/` | フロントエンド（TypeScript + esbuild、地図は SVG で自前描画。外部タイル不要） |
| `samples/` | 地震が起きていないときの確認用デモデータ (`scenarios/`) と設定 |
| `tools/simplify_geojson.py` | 都道府県境界データを軽量化するスクリプト |
| `tools/tsunami_areas.py` | 津波予報区の沿岸線データを軽量化するスクリプト |
| `tools/neighbors.py` | 周辺国の陸地 (背景) を切り出して軽量化するスクリプト |
| `tools/jma_areas.py` | 地震情報細分区域の境界と、震度観測点の位置 (属する細分区域つき) を作るスクリプト |

### 扱う情報

| code | 内容 | 画面での表示 |
| --- | --- | --- |
| 556 | 緊急地震速報（警報） | 警報バナー、震央、P波・S波の円、予測震度で都道府県を塗る（白の破線） |
| — | 緊急地震速報（予報）※ | 予報バナー（橙）。そのほかは警報と同じ |
| 551 | 地震情報（震度速報・震源・各地の震度） | 震央、観測震度で都道府県を塗る、観測点の一覧 |
| 552 | 津波予報 | 対象の沿岸を等級の色（大津波警報=紫、津波警報=赤、津波注意報=黄）で描き、解除まで表示。バナー、一覧・詳細 |
| 554 | 緊急地震速報の発表検出 | 受信のみ |
| — | 気象警報・注意報（気象庁防災情報XML の集約通報）※2 | 平時（地震・津波・揺れの報告の表示が無いとき）だけ、市町村等ごとに段階の色（注意報=黄、警報=赤、危険警報=紫、特別警報=黒）で塗る。ツールチップに発表中の種類。警報以上は上部の帯に「レベル３大雨警報: 岡山県 岡山市・倉敷市」のように文字でも出す（収まらなければ横に流す）。地震の情報が届けば地震の表示に切り替わる |
| 9611 | 地震感知情報（P2P地震情報の利用者による「揺れた」報告の集計。気象庁の発表ではない） | 報告のあった地域に広がる輪（信頼度が低いほど薄く）、控えめな音と「揺れの報告」の知らせ。最後の更新から 2 分、またはその揺れの緊急地震速報・地震情報が届くまで。一覧には入れない |

※2 気象庁防災情報XML (PULL 型) の「気象警報・注意報（Ｒ０６）（集約通報）」（全国分を 10 分ごとに発表）をサーバが 5 分ごとに取得して
`GET /api/warnings` で返します。`config.toml` の `[weather] enabled = false` で止められます。

※ P2P地震情報は緊急地震速報の警報（予測震度5弱以上）しか配信しないため、予報は
[Wolfx Open API](https://wolfx.jp/docs/open-api) の JMA 緊急地震速報から受け取ります。
`config.toml` の `[source] eew_url = "wss://ws-api.wolfx.jp/jma_eew"` で有効になります（既定は無効）。
Wolfx は非公式の中継サービスです。公開サイトで使う場合は [Wolfx の利用規約](https://wolfx.jp/legal/terms)
（再提供や、公式の警報として表示することの禁止など）を確認してください。

同じ地震についての複数の情報（震度速報 → 震源 → 各地の震度、EEW の続報）は 1 行にまとめて表示します。
複数の地震がほぼ同時に起きたときは、P波・S波の円はすべて描き、地図のカメラと詳細パネルは
揺れの大きい方（EEW は予測、地震情報は観測の最大震度）に合わせます。
震度の塗り分けはその地震の最後の情報から 10 分ははっきり表示し、その後だんだん薄くして 1 時間で消します。
地図のカメラは、揺れを描き終えて最後の情報から 3 分 (最大震度2以下の軽い地震は 1 分) たつと日本全体に戻り、平時の表示 (気象警報・注意報) に切り替わります（地震を選んでいるとき・津波予報が出ているときを除く）。地震の表示の間は上部の「警報・注意報」ボタンですぐ平時に戻せます (次に新しい地震の情報が届けば、また地震の表示になります)。
緊急地震速報で予測震度が出ている間は、予測の出た地域がはじめから収まるようにカメラを引きます。予測震度は、観測の震度と見分けられるよう、点線の枠で塗りと数字を点滅させます（観測の情報が届くと、観測のあった地域から観測の震度に置き換わり、まだ観測の無い地域は緊急地震速報が続く間は予測のまま残ります。観測の震度がまだ届いていなければ (震源の情報だけなど)、緊急地震速報が終わっても予測を残します (最大 1 時間)。続報で予測から外された地域は、いきなり消さずに 8 秒かけて薄れながら消えます。動きを減らす設定の端末では点滅しません）。
地震に寄ると（表示範囲が狭いとき）、都道府県の代わりに地震情報細分区域で塗り、震度観測点を震度の色の点で重ねます。
塗り分けた地域には震度を数字でも出します（震度が大きいほど大きな字）。
「全体図」ボタンで日本全体に戻し、地図上の震央を押すとその地震へ寄ります。
互いに 300km 以上離れた場所で地震が続いたときは、それらを一定の間隔で順に映します（巡回。地図を動かすか地震を選ぶと止まり、新しい情報が届くとしばらくその地震を映します）。
詳細パネルは観測震度ごとの地点数の要約を常に出し、観測点の一覧は最後の発表から一定時間たつと畳みます（要約を押すと開きます）。
日本全体の表示は九州〜北海道に絞り、南西諸島は左上の別枠に常に、小笠原は右下 (日時表示の上) の別枠に震度・津波予報・震央があるときだけ描きます。日時表示は、全体図で北海道に重ならないよう地図の右下に置きます
（寄っているときは本図だけで描くので、離島付近の地震でも P波・S波の円は本図に正しく描かれます）。

## 使い方

### ビルド済みのリリースを使う (サーバへの配置)

[Releases](https://github.com/densuke/eq-webservice/releases) から次のどちらかをダウンロードして展開します。
サーバ側で Rust や Node.js を使ってビルドする必要はありません。

- `eq-server-<version>-x86_64-unknown-linux-gnu.tar.gz` (Linux / amd64)
- `eq-server-<version>-aarch64-apple-darwin.tar.gz` (macOS / Apple Silicon)

バイナリ・地図ページ (`web/dist`)・設定例・systemd ユニット例が 1 つのディレクトリに入っています。
配置手順は [deploy/INSTALL.md](deploy/INSTALL.md) を見てください。

### 起動オプション

| オプション | 環境変数 | 内容 |
| --- | --- | --- |
| `-c, --config <path>` | | 設定ファイル (省略時は `./config.toml`、無ければ既定値) |
| `-p, --port <port>` | `EQ_PORT` | 待ち受けポートだけ変える |
| `-l, --listen <addr:port>` | `EQ_LISTEN` | 待ち受けアドレスとポート |
| `--static-dir <path>` | | 地図ページの場所 (既定は `web/dist`。無ければ実行ファイルの隣を探す) |
| `-V, --version` | | バージョン表示 |

優先順位は 起動オプション > 環境変数 > 設定ファイル (`[server] listen`) です。

### ソースからビルドする

必要なもの: Rust (stable)、Node.js 22 以降

```sh
# フロントエンドをビルド (web/dist に出力)
cd web && npm ci && npm run build && cd ..

# デモ: 架空の地震を「今」起きたものとして繰り返し再生
cargo run -p eq-server -- --config samples/demo.toml
# → http://127.0.0.1:8080/

# 本番: P2P地震情報に接続
cp config.example.toml config.toml   # 必要に応じて編集
cargo run --release -p eq-server -- --config config.toml
```

HTTP の口:

- `/` … 地図ページ
- `/ws` … ブラウザ向け WebSocket（接続時に `hello` で直近の情報とサーバ時刻、その後は `event` を送る）
- `/api/events` … 直近の情報（JSON）
- `/feed.xml` … RSS（rss プラグインを有効にした場合）
- `/healthz` … 死活監視

### 自分の地点・通知・到達カウントダウン

画面右上の「設定」で、自分の地点 (地図をクリック・タップして選ぶ、または「現在地を使う」) と通知のレベル
(通知しない / 警報のみ / 震度4以上 / 震度3以上) を設定できます。設定はこの端末の中 (ブラウザ) だけに保存されます。

- 自分の地点を設定すると、緊急地震速報を受けている間、その地点に主要動 (S波) が届くまでの秒数と予測震度を出します
  (速度一定の概算。予測震度は地点に最も近い震度観測点が属する細分区域の値)
- 通知は、画面が裏にあるときにブラウザ (OS) の通知で知らせます。「震度4以上」では、緊急地震速報 (警報)・最大震度4以上・
  津波警報以上に加え、自分の地点で震度3以上のときも通知します。デモの情報は通知しません
- 履歴に出す地震 (すべて / 震度2以上 / 3以上 / 4以上 / 5弱以上。緊急地震速報・津波予報は常に出す)
- 観測点の一覧を畳むまでの時間 (最後の発表から 5〜60 分、または最初から畳む・畳まない。既定は 10 分)
- 巡回の間隔 (5〜60 秒を 5 秒刻み、または巡回しない。既定は 10 秒)

### デモモード

画面右上の「デモ」から、いくつかの場面 (緊急地震速報の警報・予報、群発、全国同時、離島、津波警報) を
再生して表示や音を確かめられます。再生はブラウザの中だけで行うので、サーバやほかの閲覧者には影響しません。
デモ中も実際の情報は受け続け、緊急地震速報などが届くとデモを終えて実際の表示に戻ります。
`https://<サーバ>/?demo=standard` のように URL で場面を指定して開くこともできます (見守りモニタの動作確認用)。

画面左上には今の表示モード (リアルタイム / リプレイ中 / デモモード中) が出ます。

場面を再生している間は、一覧の上の操作で一時停止・倍速 (×1/×2/×4/×8)・再生位置の移動 (巻き戻し・早送り) ができます。
再生はデモ専用の時計で進み、報の間は、揺れの広がり (P波・S波) を描いている間は当時の間隔のまま、それ以外の長い間は 8 秒に詰めます (その間は時計が早く進みます)。
再生は最初の地震の発生の 5 秒前から始まります。

「記録:」で始まる場面は、過去の地震の当時の発表をそのまま再生し、時計も当時の日時で表示します (出典は場面の一覧に表示)。
架空の場面は、再生を始めた「今」の日時で再生します。

| 場面 | 内容 | 出典 |
|---|---|---|
| `noto2024` | 令和6年能登半島地震 (2024-01-01)。16:06 の前震から本震 (M7.6, 震度7)、16:18 の地震、大津波警報と解除まで | 気象庁 (緊急地震速報の発表状況)、P2P地震情報 |
| `tohoku2011` | 東北地方太平洋沖地震 (2011-03-11) 本震 (M9.0, 震度7)。緊急地震速報、大津波警報の拡大 (14:49〜12日03:20) と解除、M の更新 (7.9→9.0)。各地の震度は震度データベースの確定値 (当時の発表の時刻・順ではない)。12日午後からの津波警報の切り下げは省略 | 気象庁 (緊急地震速報の発表状況、震度データベース、災害時地震・津波速報) |
| `kobe1995` | 兵庫県南部地震 (1995-01-17、M7.3)。震度は当時の階級 (5・6 に弱・強の区別なし)。震度7 は後日の現地調査で決まった市町を点 (位置は概算) で示す。発表の時刻は再現していない | 気象庁 (震度データベース) |
| `kanto1923` | 関東大震災 (1923-09-01、M7.9、最大震度6)。震度は当時の階級。震度不明の観測点は除く。発表の時刻は再現していない | 気象庁 (震度データベース) |

記録の場面は `tools/scenario_*.py` で当時の発表から作ります (緊急地震速報は `tools/jma_eew.py` で気象庁の発表状況のページを読みます)。

場面のデータは `samples/scenarios/*.jsonl` (P2P地震情報 / Wolfx 形式、先頭の `# name:` `# description:` `# source:` で名前・説明・出典) で、
画面用の JSON は次のコマンドで作ります (サーバと同じ変換処理を使います。食い違いはテストで検出します)。

```sh
cargo run -p eq-server -- convert samples/scenarios web/public/demo
```

### 警戒音

画面右上の「音」ボタンで ON/OFF を切り替えます。設定はブラウザに保存され、次に開いたときも引き継がれます。
ただしブラウザは、ページを一度操作するまで音を出させないことが多く、その間は「クリック (スマホ・タブレットではタップ) すると警戒音が鳴るようになります」
という案内が出ます。見守り用のモニタなど、操作せずに開きっぱなしにする端末では、サイトの設定で音声を許可してください。

- Chrome / Edge: アドレスバー左のアイコン → サイトの設定 → 「音声」を「許可」
- Firefox: アドレスバー左のアイコン → 「自動再生」を「音声と動画を許可」
- Safari: Safari → 設定 → Web サイト → 自動再生 → このサイトを「すべてのメディアを自動再生」


### バナー (平時の案内・お知らせ)

`config.toml` で画像とテキストを置くディレクトリを指定すると、平時に画面右側の履歴の下半分へバナーを出します。

```toml
[banner]
dir = "/home/eq/banner"
interval_sec = 20   # 切り替える間隔 (秒)
```

- 画像 (png / jpg / jpeg / webp / gif) とテキスト (.txt) を、ファイル名 (拡張子を除く) ごとに 1 枚にして名前順に切り替えます。
  同じ名前の画像とテキストは、画像に文字を添えた 1 枚になります
- テキストの `http://` / `https://` で始まる行はリンク先になり (最初の 1 つ)、バナーを押すと新しいタブで開きます
- 置いた内容は文字は文字、画像は画像としてだけ出します (HTML としては扱いません。SVG も扱いません)
- 一覧は一巡するたびに読み直すので、ファイルはサーバを止めずにいつでも差し替えられます
- 地震の表示 (緊急地震速報・地震情報・津波予報・揺れの報告) の間とデモ中は隠します

### BGM (平時)

平時に BGM を流せます。音楽の配信は Icecast に任せ、この画面は配信の URL を鳴らすだけです
(地震の状態を知っているのは画面なので、止める・再開するは画面が受け持ちます)。

```
曲 (m4a など) --[tools/bgm_prepare.sh (手元の Mac などで)]--> 配信用の MP3
  --[rsync]--> サーバ --[eq-server bgm-send]--> Icecast (127.0.0.1) --[リバースプロキシ /stream/]--> 画面
```

1. 前処理 (ffmpeg のある手元で): `tools/bgm_prepare.sh <曲のディレクトリ> <出力のディレクトリ>` で、配信用の MP3
   (44.1kHz・160kbps 固定、頭 1.5 秒と終わり 3 秒にフェード、タイトル・アーティストのタグを引き継ぐ) を作り、rsync でサーバへ送る
2. 送り出し: `eq-server bgm-send <MP3 のディレクトリ> http://127.0.0.1:8010/bgm.mp3` (パスワードは環境変数 `ICECAST_SOURCE_PASSWORD`)。
   MP3 をファイル名順に、フレームの長さで再生の速さに合わせて送り続ける (音は読み解かないので軽い)。曲が変わるたびに曲名を Icecast に知らせ、
   1 曲ごとにディレクトリを読み直すので、曲はいつでも差し替えられる。ユニット例は `deploy/eq-bgm.service`
3. Icecast は 127.0.0.1 だけで待ち受け、リバースプロキシで配信 (`bgm.mp3`) と状態 (`status-json.xsl`) だけを同じサイトの `/stream/` に中継する
   (例は `deploy/Caddyfile.example`)
4. `config.toml`:

```toml
[bgm]
stream = "stream/bgm.mp3"
status = "stream/status-json.xsl"   # 再生中の曲名を出す (省略可)
```

画面の右上の「音」の隣に「BGM」ボタンが出ます。音量は「設定」で変えられます (端末ごとに保存)。
緊急地震速報・地震情報・津波予報・揺れの報告が届いて地震の表示になると直ちに止め (配信の受信もやめる)、平時に戻るとそのときの放送から 1 秒で音を上げて流します。
デモ中も止めます。ブラウザは一度操作するまで音を出させないことが多いので、そのときはボタンが「BGM ON (クリックで開始)」になります。
曲のつなぎ目は前処理のフェード (フェードアウト → フェードイン) です (2 曲を重ねるクロスフェードは、その場で音を混ぜる処理が重いので行いません)。

### ライブ配信 (YouTube など)

`eq-server broadcast <broadcast.toml>` で、画面の無い Chrome で地図のページを開き、その画面を ffmpeg で配信し続けます。
Chrome か ffmpeg が止まったら、両方を止めて 5 秒後に立ち上げ直します。必要なものは Chrome (Linux は Chromium) と ffmpeg です。

- 開くページは `?broadcast=1` を付けた配信用の表示です。操作ボタンを隠し、警戒音と BGM を最初から鳴らします
- 画面は DevTools の screencast で受け取り、設定した fps で ffmpeg に渡します (既定は 1280x720・30fps・x264)
- 音は ffmpeg の入力 (`audio`、Linux は PulseAudio のモニタ) か、音を出すコマンド (`audio_command`) で取り込みます。省くと無音です。
  Mac では BlackHole に流した音を sox で取り込みます (ffmpeg の avfoundation は音を 1 割ほど落として途切れるため)。
  Mac ではページの URL に `&sink=BlackHole%202ch` を付けると、ページの音だけを BlackHole へ流します (Mac 全体の出力先は変えません)。
  画面の無い Chrome はマイクを一度開くまで機器の名前を見せないため、ページはマイクを開いてすぐ閉じます (音は使いません)
- `mixer = true` にすると、音を eq-server の中で作ります (BlackHole・sox は要りません)。
  ページは音を鳴らさず、BGM の流す・止めると警戒音の種類を eq-server に知らせます。
  eq-server が BGM (`bgm_url`、既定は https://eq.fuga.jp/stream/bgm.mp3) を受けて戻し、警戒音を合成して混ぜ、44.1kHz のステレオで ffmpeg に渡します。
  `audio`・`audio_command` より優先します。設計は [docs/broadcast-v2.md](docs/broadcast-v2.md)
- Chrome のプロファイルは普段使いと分けます (`profile` で場所を決めるか、省けば毎回一時ディレクトリ)
- 送り先の `$VAR` は環境変数に置き換え、ffmpeg のログでは値を伏せます。ストリームキーは設定ファイルに書かないでください
- YouTube への送信に API キーは要りません。YouTube Studio のストリームキーだけで配信できます

設定例は [broadcast.example.toml](broadcast.example.toml) にあります。

```sh
set -a; . ~/.config/youtube-live-eq-webservice; set +a   # YOUTUBE_LIVE_API_KEY=...
eq-server broadcast broadcast.toml
```

配信は数秒以上遅れて届きます。緊急地震速報は公式の手段で受け取るよう、画面の注意書きはそのまま残しています。

#### Chrome を使わず eq-server が画面を描く (native)

`source = "native"` にすると、Chrome を起動せず eq-server が 1280x720 の画面を描いて ffmpeg に渡します (負荷の測り方は
[docs/broadcast-native.md](docs/broadcast-native.md) 11 章、設定は [broadcast.example.toml](broadcast.example.toml) の native の節)。

- `test = true`: テスト配信の表示。赤い帯 2 本・「TEST」の透かし・「[テスト]」を必ず描きます (消す設定はありません)。
  過去の地震を流すなどして確かめるときは、送り先を **本番とは別の配信 (限定公開) のストリームキー** にしてください (同じキーかどうかは判断できません)
- 安全装置: データの取得先 (`server`) が記録を流すサーバ (replay。`GET /api/source` が `replay` を返す) なのに `test = true` が無いと、
  配信を始めずにエラーを出して止まり、10 分後に見直します。取得先が答えないとき (古いサーバ) は replay ではないとみなします

#### ffmpeg を使わず eq-server が圧縮して送る (encoder = "builtin"、実験)

`source = "native"` で無音のとき、`encoder = "builtin"` にすると ffmpeg を使わず、eq-server が H.264 (openh264) と無音の AAC を FLV に詰めて
RTMP / RTMPS で送ります (ffmpeg が要らず、メモリも減ります。仕様は [docs/broadcast-builtin.md](docs/broadcast-builtin.md))。省けば今までどおり ffmpeg です。

- `output` の最初の要素が送り先です。`rtmp://` か `rtmps://` (ストリームキーは `$VAR` で環境変数から)、または `.flv` のパス (確かめる用)。
  `output = ["rtmps://a.rtmps.youtube.com/live2/$YOUTUBE_LIVE_API_KEY"]` のように、ffmpeg の引数 (`-f flv`) は書きません
- `builtin_bitrate` (bps、既定 300000): 映像の目標ビットレート。コマは飛ばさず、画面が大きく変わるときは画質を下げて目標に近づけます (超えることがあります)
- キーフレームは 2 秒ごと (時刻で決めるので、可変 fps でも崩れません)
- `mixer = true`・`audio`・`audio_command` (音のある配信) と `source = "chrome"` では使えません (起動時にエラー)。音があるときは ffmpeg を使ってください
- 送り先が切れたらエラーで止まり、5 秒後につなぎ直します (ffmpeg のときと同じ)

#### e2 から常時配信する

`deploy/eq-broadcast.service` (ユーザーユニット。CPUQuota 25%・MemoryMax 200M・Nice 19・Restart always) と
`deploy/broadcast.e2.toml` (平時 2fps・地震 10fps・無音) を使います。

```sh
sudo apt install ffmpeg fonts-noto-cjk
mkdir -p ~/work/eq-e2cast/map && cd ~/work/eq-e2cast
cp <eq-server のバイナリ> eq-server                      # リリースのバイナリ
cp <リポジトリ>/web/public/{japan,warning-areas,neighbors}.geojson map/   # 地図のデータ
cp <リポジトリ>/deploy/broadcast.e2.toml cast.toml
# ストリームキー (権限 600)。書式は YOUTUBE_LIVE_API_KEY=...
install -m 600 /dev/null ~/.config/youtube-live-eq-webservice && $EDITOR ~/.config/youtube-live-eq-webservice
mkdir -p ~/.config/systemd/user && cp <リポジトリ>/deploy/eq-broadcast.service ~/.config/systemd/user/
loginctl enable-linger $USER                             # ログアウトしても動かし続ける
systemctl --user daemon-reload && systemctl --user enable --now eq-broadcast
journalctl --user -u eq-broadcast -f
```

`eq-server` の入れ替えは、ファイルを置き換えてから `systemctl --user restart eq-broadcast` です。

## プラグイン（配信先）

`config.toml` の `[[sinks]]` に並べます。共通のキー:

- `type` … `rss` / `discord` / `jsonl` / `webhook`
- `kinds` … 流す種類（`quake` / `eew` / `eew_detection` / `tsunami` / `userquake`）。省略すると `userquake` 以外すべて
  （地震感知情報は利用者の報告の集計なので、`userquake` と書いたときだけ流します）
- `min_scale` … 最大震度がこれ未満の地震情報・EEW は流さない（`"3"`, `"5弱"` など）
- `enabled`, `name`

各プラグインは別々のタスクで動くので、Discord が遅くても他の配信先や画面の更新は止まりません。
詳しくは [config.example.toml](config.example.toml) を見てください。

新しいプラグインを作るときは `crates/eq-server/src/plugins/` に `Sink` トレイトの実装を追加し、
`plugins::build()` に `type` 名を登録します。Rust 以外で書きたい場合は `webhook` プラグインで
正規化済みの JSON を受け取れます。

## リリースの作り方

`Cargo.toml` の `version` を上げてから、同じ番号のタグを push します。

```sh
git tag v0.1.0 && git push origin v0.1.0
```

CI がビルド・テストのあと、両ターゲットの tar.gz と sha256 を GitHub Releases に添付します。
`v0.2.0-rc1` のように `-` を含むタグはプレリリースになります。
GitHub の画面の「Draft a new release」で新しいタグを作って公開しても構いません
(CI が完了すると、そのリリースに tar.gz が追加されます)。

## リバースプロキシ (Caddy) の例

ページは相対パスで WebSocket・地図データを読むので、サブパスの下にも置けます。

```caddyfile
eq.example.jp {
	reverse_proxy 127.0.0.1:8080
}

# サブパスの下に置く場合
example.jp {
	handle_path /eq/* {
		reverse_proxy 127.0.0.1:8080
	}
}
```

設定例は [deploy/Caddyfile.example](deploy/Caddyfile.example) にもあります。
Caddy の `reverse_proxy` は WebSocket をそのまま通します。サーバは 30 秒ごとに ping を送るので、
アイドル状態での切断は起きにくいはずです。

## CI

GitHub Actions（`.github/workflows/ci.yml`）で次をビルド・テストします。

- web: 型チェック、テスト、ビルド
- rust: `x86_64-unknown-linux-gnu`（ubuntu）と `aarch64-apple-darwin`（macOS arm64）で
  fmt / clippy / test / release ビルド、配布用 tar.gz の作成、展開した tar.gz を使ったデモ再生での動作確認
- release: `v*` タグのときだけ GitHub Releases を作成

## テスト

```sh
cargo test                     # Rust (変換・Hub・プラグインの判定・HTTP・デモの変換済みファイルの食い違い)
cd web && npm test             # 画面の判定 (優先度・番号・グループのまとめ方・デモ・通知・震度の推定など)
cargo llvm-cov --summary-only  # カバレッジ (計器として見るだけ。CI にも表示する)
```

コミット前に CI と同じチェック (fmt・clippy・テスト・依存の監査) を走らせるには、フックを有効にします
(依存の監査には `brew install cargo-audit` が要ります。無ければその部分だけ省きます)。

```sh
git config core.hooksPath .githooks
```

テストの要否は「壊れたときに気づけるか・戻せるか」で決めています。外部へ送る出力 (Discord・RSS・Webhook の判定と本文)、
上流の情報の変換、Hub の重複排除と保持、画面で地震を選ぶ・まとめる判定は必ずテストします。
次の箇所は意図的に自動テストしていません。

- 上流への接続と再接続 (`source/p2pquake.rs`, `source/wolfx.rs` の通信部分)、起動処理 (`main.rs`): 失敗はログとすぐに分かる形で表に出る。
  メッセージの変換や古い速報を捨てる判定など、判断を含む部分は関数に切り出してテストしている
- 画面の描画 (DOM・SVG・CSS): 目で確かめる。デモモードの場面で確認できるようにしている
- ブラウザ通知の表示・自動再生の制限・位置情報: ブラウザの権限が必要なため手で確かめる

## 今後の拡張候補

- 観測点の座標表を持ち、震度を観測点ごとの点で描く（今は都道府県単位で塗り分け）
- P波・S波を気象庁の走時表 (JMA2001) で計算する（今は速度を一定とした概算）

## 出典・ライセンス

- 地震情報: [P2P地震情報](https://www.p2pquake.net/)（気象庁発表の情報）
- 緊急地震速報（予報、有効にした場合）: [Wolfx Project](https://wolfx.jp/) の Open API 経由（気象庁発表の情報。非公式の中継）
- 地図: 地球地図日本（国土地理院）を [dataofjapan/land](https://github.com/dataofjapan/land) 経由で加工。
  営利目的で使う場合は、国土地理院の利用規約に従って利用報告が必要です。
- 津波予報区・地震情報細分区域: [気象庁「予報区等GISデータ」](https://www.data.jma.go.jp/developer/gis.html)を加工して作成
- 周辺国の陸地: [Natural Earth](https://www.naturalearthdata.com/)（パブリックドメイン）を加工
- 震度観測点の位置: [気象庁の震度観測点の一覧](https://www.data.jma.go.jp/eqev/data/intens-st/)を加工して作成
- 気象警報・注意報: [気象庁防災情報XML](https://xml.kishou.go.jp/)。区域 (市町村等) の境界は[気象庁「予報区等GISデータ」](https://www.data.jma.go.jp/developer/gis.html)を加工して作成 (`tools/jma_warning_areas.py`)
- 天気アイコン (配信の native 描画): [気象庁ホームページ](https://www.jma.go.jp/bosai/forecast/)の天気予報のアイコンを加工して表示 (公共データ利用規約 第1.0版。コードとアイコンの対応は `tools/jma_telops.py` で作る)
- 地震感知情報の地域の位置 (`web/public/userquake-areas.json`): [p2pquake/epsp-specifications](https://github.com/p2pquake/epsp-specifications) の `epsp-area.csv` を加工
  （MIT License, Copyright (c) 2018 takuya (P2PQuake)）
- H.264 の圧縮 (`encoder = "builtin"`): [Cisco OpenH264](https://github.com/cisco/openh264) を [openh264](https://crates.io/crates/openh264) クレート経由でソースから組み込み
  (BSD 2-Clause。H.264 の特許の扱いは OpenH264 の README を参照)
- ソースコード: GPL-3.0-or-later（[LICENSE](LICENSE)）
