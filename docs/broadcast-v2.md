# ライブ配信 v2 の計画 (eq-server broadcast の作り直し)

v0.13 の `eq-server broadcast` は、headless Chrome、ffmpeg、BlackHole、sox を組み合わせて動いている。
これを、少ない部品で、Mac・Raspberry Pi 5・(可能なら) e2 のどこでも動く形にする。
この文書は、作業を複数の担当 (Sonnet 5.5 など) に分けて進められるように、検証の結果、設計、作業の単位、完了の条件をまとめたもの。

## 1. 目標と、目標にしないこと

目標

- 配信に要るものを `eq-server` 1 つと、置き換えのきく少数の外部ツールにまとめる。
  最初に BlackHole と sox を無くす (音を eq-server の中で作る)。
- 画面の取り込み、映像の圧縮、音の圧縮、送り出しを部品に分け、環境ごとに差し替えられるようにする。
  - 例: Mac は VideoToolbox、Pi は x264、手元の確認はファイル出力
- Raspberry Pi 5 (4GB、有線 LAN、常時稼働) で動かす。

目標にしないこと (今は)

- ffmpeg を完全に無くすこと。
  Linux で AAC を圧縮できる手ごろなライブラリが無い (後述)。ffmpeg は「圧縮と送り出し」の差し替えのきく部品の 1 つとして残す。
- 画面を Rust で描き直すこと。画面側 (TypeScript 約 7000 行) を二重に持つことになるため。
  ただし e2 で直接配信する場合だけは、これが唯一の道になる (4 章)。

## 2. 検証の結果 (2026-09-30、MacBook Air M4)

### 2.1 今の配信の負荷 (配信を 18 分動かした時点)

| プロセス | CPU (1 コア = 100%) | メモリ |
|---|---|---|
| Chrome 本体 (screencast の JPEG 作り) | 39% | |
| Chrome GPU | 35% | |
| Chrome 描画 (ページ) | 29% | |
| Chrome 合計 | 約 100% | 約 650MB |
| ffmpeg (JPEG を戻して x264 veryfast、AAC) | 22% | 217MB |
| eq-server (base64 を戻す) | 6% | 7MB |
| sox | 0.1% | 11MB |

無駄が大きいのは、画面を JPEG に圧縮し、base64 にして戻し、また H.264 に圧縮する往復。
ページを描く分 (約 30%) は、ブラウザを使う限り避けられない。

### 2.2 WKWebView + VideoToolbox (Mac、Swift の試作 `tools/spike/wkwebview_capture.swift`。`env -u SDKROOT swiftc -O` で組む)

- `WKWebView.takeSnapshot` で撮り、`VTCompressionSession` (H.264 Main、3Mbps) で圧縮した。
- 撮れたのは 1 秒に最大 13.6 枚。撮影の完了を待たずに次を頼むことはできない。
- CPU は合計 112%。そのうち WebKit の GPU の処理が 80%、ページの描画が 26%。
- 圧縮 (VideoToolbox) は、画像の変換を含めて約 9%。GPU での圧縮ははっきり軽い。
- **結論**: takeSnapshot は撮る処理が重く、Chrome の screencast より軽くならない。
  Mac で軽くするには、撮り方を ScreenCaptureKit (ウィンドウを GPU のまま受け取る) にする必要がある。これは未検証 (6 章 S1)。
  ScreenCaptureKit は、初回に macOS の「画面収録」の許可が要る。

### 2.3 x264 の負荷 (この地図の画面、1280x720、1 スレッド、M4 の 1 コアに対する割合)

| fps | preset | CPU |
|---|---|---|
| 30 | veryfast | 15.7% |
| 30 | ultrafast | 10.3% |
| 15 | veryfast | 8.5% |
| 10 | veryfast | 5.0% |
| 10 | ultrafast | 3.1% |

画面はほとんど止まっているので、x264 は軽い。コマ数を下げるほど、ほぼ比例して軽くなる。
Raspberry Pi 5 (Cortex-A76) は、1 コアあたり M4 の 1/3〜1/4 程度と見込む (未計測)。
その見込みだと 720p30 veryfast は Pi の 1 コアの 50〜65%、15fps なら 25〜35%。4 コアあるので収まる見込み。

### 2.4 音 (v0.13.2〜v0.13.4 で分かったこと)

- macOS の ffmpeg 9.0.2 の avfoundation は、音のサンプルを約 1 割落とす (BlackHole でも内蔵マイクでも)。
- sox で CoreAudio から取れば欠けない。ただし BlackHole と sox の 2 つを入れる必要がある。
- 画面の無い Chrome は、マイクを一度開くまで出力先の機器の名前を見せない。
- 音の途切れや、BGM が止まったまま戻らない問題は、どれも「ページで鳴らした音を OS の外から取り込む」ことが原因だった。

### 2.5 e2 (e2-micro、us-west1)

- メモリ 953MB (空きは約 400MB)。2 vCPU の共有コアで、続けて使える CPU は 0.25 vCPU 分 (GCP の e2-micro の仕様。短時間だけ超えられる)。
- Chrome (約 650MB) は載らない。x264 の 720p30 を続けて回すのも、0.25 vCPU には収まらない。
- 外向きの通信量: 無料枠は月 1GB (北米から。GCP の無料枠の資料で確認)。
  1Mbps で流し続けると月約 324GB になり、料金がかかる (1GB あたり 0.085〜0.12 ドルなら月 28〜39 ドル。単価は要確認)。

### 2.6 e2 で Rust で描いて圧縮する試作 (`tools/spike/e2render`)

- tiny-skia で都道府県の塗りと輪郭、点 300 個、時計の枠を毎コマ描き直し、openh264 (画面向けの設定、1Mbps) で 1280x720 に圧縮した。
  文字はまだ描いていない。
- 作り方: `cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.35` で Mac から e2 向けに作り、e2 で `nice -n 19` で動かした。

| 場所 | 描く | 色の変換 (RGBA -> YUV) | 圧縮 | 合計 (1 コマ) |
|---|---|---|---|---|
| Mac (M4) | 7.3ms | 2.4ms | 4.2ms | 14.0ms |
| e2 (Xeon 2.2GHz、最初の約 40 秒) | 10.8ms | 26.1ms | 12.1ms | 49.0ms |
| e2 (全力を約 40 秒続けたあと) | | | | 約 500ms (10 倍遅くなる) |

- e2 で 1 コアに対する割合: 5fps で 24%、10fps で 49%、30fps で 147%。
- e2-micro は短時間だけ全力を出せるが、続けると続けて使える分 (0.25 vCPU) まで強く絞られる。
  この枠は eq-server・Icecast・Caddy と共有なので、**配信が使いすぎると本番の地震の配信も遅くなる**。
- 地図のような止まった画面なら、圧縮したあとの量は 10fps で 0.05Mbps と小さい (文字や動きが増えれば上がる)。
- 色の変換 (26ms) は openh264 の単純な実装で遅い。自前で整数演算にすれば数 ms にできる見込み。

## 3. 構成 (部品と、そのつなぎ目)

```
            ┌──────────── 画面 (FrameSource) ────────────┐
            │ ChromeScreencast (今)   JPEG                │
            │ ScreenCaptureKit (Mac)  BGRA / IOSurface    │
            │ NativeRenderer (e2 用)  RGBA                │
            └──────────────┬──────────────────────────────┘
                           │ Frame
 ページの音の知らせ ──> ┌──┴─────────────┐        ┌──────── 圧縮と送り出し (Encoder) ────────┐
 (CDP binding)          │ Broadcaster     │ Frame  │ FfmpegEncoder (今。Linux の既定)          │
 BGM (Icecast MP3) ──> │ (見張り・時計) ├──────> │ VideoToolbox + AudioToolbox + FLV/RTMP    │
 警戒音 (合成)     ──> │ AudioMixer      │ PCM    │   (Mac。後で)                             │
                        └─────────────────┘        │ FileEncoder (確認用)                      │
                                                   └───────────────────────────────────────────┘
```

つなぎ目 (Rust の trait。`crates/eq-server/src/broadcast/` に置く)

```rust
/// 画面 1 枚。圧縮する側が扱える形で渡す
pub enum Frame {
    Jpeg(Vec<u8>),
    Bgra { width: u32, height: u32, stride: usize, data: Vec<u8> },
}

/// 画面を出すもの。変化が無いときは、前の画面のまま待ってよい
/// (async fn in trait を使う。dyn にはできないので、選ぶところは enum で分ける。plugins の Sink と同じやり方)
pub trait FrameSource {
    async fn next(&mut self) -> anyhow::Result<Frame>;
}

/// 音: 44.1kHz・ステレオ・i16 のサンプル列 (L, R, L, R, ...)
pub struct Pcm(pub Vec<i16>);

/// 圧縮して送る。video は fps に合わせて呼ぶ (同じ画面を繰り返してよい)、audio は実時間に合わせて呼ぶ
pub trait Encoder {
    async fn video(&mut self, frame: &Frame) -> anyhow::Result<()>;
    async fn audio(&mut self, pcm: &Pcm) -> anyhow::Result<()>;
    /// 止まった理由を返す (ffmpeg が終わった、送り先が切れた、など)
    async fn closed(&mut self) -> anyhow::Error;
}
```

- 実装は 1 つずつ別のファイルにする。`broadcast.toml` の `source = "chrome"`、`encoder = "ffmpeg"` のように名前で選ぶ。
- 既定は今と同じ組み合わせ (`chrome` + `ffmpeg`)。部品を差し替えても、設定を変えなければ動きは変わらない。
- 時計 (fps に合わせて画面を送る) と音の送り出しは `Broadcaster` が受け持つ。部品は時計を持たない。

## 4. どこで配信するか

| 場所 | 画面 | 圧縮 | 見込み | 判断 |
|---|---|---|---|---|
| Mac (今) | Chrome | ffmpeg (x264) | 動いている。CPU 約 130% | 当面の本番 |
| Mac (改良) | ScreenCaptureKit | VideoToolbox | CPU 約 40% の見込み (S1 で確かめる) | S1 の結果しだい |
| Raspberry Pi 5 | Chromium | ffmpeg (x264、720p 15〜30fps) | 描画 + x264 で 1〜2 コア。4 コアに収まる見込み | 実機が手に入るまで保留 (穴を空けておく) |
| e2 (e2-micro) | NativeRenderer (Rust で描く) | openh264 (720p 5fps、変化したときだけ描く) | CPU は制限を掛ければ収まる見込み。通信料は月数ドル〜十数ドル | 検証を続ける (4.1) |

Pi は実機を手に入れるまで保留にする (W6 は穴として残す)。部品の分け方 (3 章) は Pi でも使えるようにしておく。

### 4.1 e2 で直接配信する案 (E 案)

ブラウザは載らないので、配信用の画面を Rust で描く。画面側 (TypeScript) の表示をそのまま再現するのではなく、配信用に絞った表示にする。

- 描くもの: 日本地図、発表中の警報・注意報の塗り、主要都市の天気、最新の地震 (震央・震度の塗り・数字)、時計、出典。
  データは eq-server が持っているもの (Hub のイベント、警報、天気) をそのまま使う。
- 描き直すのは、変化があったとき (情報が届いた、時計の秒が進んだ) だけ。圧縮は 5fps で送り続け、変化が無いコマは前の画面を使う。
- 音: BGM は無し (通信量を抑える)。YouTube は音の無い配信を嫌うので、無音の AAC のコマ (固定のバイト列) を流す。AAC の圧縮器は要らない。
- 送り出し: FLV を自前で組み、RTMP(S) で送る (ffmpeg を使わない)。
- 本番を守る: 配信は別のプロセス (systemd の別のユニット) にし、`CPUQuota=15%`・`Nice=19`・`MemoryMax=150M` で縛る。
  足りなければ、コマを落として続ける (本番の CPU を取らない)。
- 見込み: 平時は 1 秒に 1 回描いて 5 コマ圧縮する (描く 11ms + 変換 5ms (自前) + 圧縮 12ms×5 = 約 80ms/秒) ので、1 コアの約 8%。文字を描く分を足しても 15% に収まる見込み。
  地震の波を動かす間は描く回数が増えるので、15% の制限でコマが落ちる (2〜3fps になる) のは受け入れる。
- 通信料: 0.1〜0.3Mbps なら月 32〜97GB で、月 3〜12 ドル程度 (単価は要確認)。YouTube は低いビットレートに警告を出すかもしれない (E2 で確かめる)。

## 5. 手順 (フェーズ)

フェーズ A: 音を eq-server の中で作る (BlackHole と sox を無くす)。どの環境でも役に立つので最初にやる

1. ページは配信のとき、音を鳴らす代わりに eq-server へ「知らせ」を送る
   - BGM を流す・止める・音量
   - 警戒音の種類
2. eq-server は、BGM を Icecast から受けて戻し、警戒音を合成して混ぜ、PCM を圧縮の部品へ渡す
3. 今の `audio` / `audio_command` / `&sink=` は残す (古い設定でも動くように)。新しい既定は `audio = "mixer"`

フェーズ B: 部品のつなぎ目を入れる (動きは変えない)

1. `FrameSource` と `Encoder` を入れ、今の Chrome と ffmpeg をその実装に移す
2. `FileEncoder` (ffmpeg でファイルへ) で、手元の確認と自動テストをしやすくする

フェーズ C: 環境ごとの部品

1. Pi: Chromium + ffmpeg (x264) で S2 の計測をし、既定の fps と preset を決める
2. Mac: ScreenCaptureKit と VideoToolbox の部品 (S1 の結果が良ければ)。
   AAC は AudioToolbox、送り出しは FLV + RTMP(S) を自前で書く (`rml_rtmp` を使うか判断)

## 6. 検証 (これから)

| # | 内容 | 必要なもの | 担当の目安 |
|---|---|---|---|
| S1 | ScreenCaptureKit で自分のウィンドウ (WKWebView) を 30fps で受け取り、VideoToolbox で圧縮する。CPU を測る | Mac、「画面収録」の許可 (利用者の操作) | Swift が書ける担当 |
| S2 | Pi 5 で Chromium + `eq-server broadcast` (ffmpeg、x264) を 720p 30/15/10fps で動かし、CPU・温度・メモリを 1 時間測る | Pi 5 の実機、PulseAudio | 利用者 + 担当 |
| S3 | `rml_rtmp` で FLV を YouTube (限定公開) に送れるか | ストリームキー (利用者の許可) | Rust 担当 |
| E1 | Rust で配信用の画面を描く (文字を含む、日本語フォントは使う文字だけに絞る)。e2 で CPU を測る | e2 | Rust 担当 |
| E2 | 自前の FLV + RTMP(S) と無音の AAC で YouTube (限定公開) に 10 分送る。低いビットレートで警告や切断が無いか | ストリームキー (利用者の許可) | Rust 担当 |
| E3 | e2 で CPUQuota=15% の別ユニットとして 24 時間動かし (送り先は /dev/null 相当)、eq-server の応答時間と CPU の絞られ方を見る | e2 | 担当 + 利用者 |
| E4 | GCP の外向き通信の単価を料金表で確かめ、月の費用を出す | | 誰でも |
| S4 | 配信用の軽い表示 (時計の秒の点滅・流れる文字・点滅などを減らす) で、ページの描画の CPU がどれだけ下がるか | Mac | 画面担当 |

## 7. 作業の単位 (分担して進める単位)

各作業は、触るファイルが重ならないように分けてある。完了の条件を満たしたら、PR を出す (マージは利用者が行う)。

### W1 画面: 配信のときの音の知らせ (フェーズ A)

- 触る: `web/src/broadcast.ts`, `web/src/sound.ts`, `web/src/bgm.ts`, `web/src/*.test.ts`
- 内容
  - `?broadcast=1&audio=mixer` のとき、`window.eqBroadcast(json)` があれば、音を鳴らさずにそれを呼ぶ
  - 知らせの形 (JSON)
    - `{"type":"bgm","play":true,"volume":0.4}`: 流す。平時になったとき
    - `{"type":"bgm","play":false}`: 止める。地震の表示・デモのとき
    - `{"type":"alert","level":"strong"}`: 警戒音。level は sound.ts の AlertLevel と同じ
  - 知らせを作る部分は純粋な関数に切り出し、テストする
- 完了の条件: `npm test` と `tsc` が通る。知らせの形を 8 章に合わせる

### W2 Rust: 警戒音の合成 (フェーズ A)

- 触る: `crates/eq-server/src/broadcast/mixer/synth.rs` (新規)
- 内容
  - `web/src/sound.ts` の `play()` と同じ音を 44.1kHz・ステレオ・i16 で作る
  - 音の組み立て: 波形 (sine / square / triangle) × 音の大きさの変化。0.01 秒で立ち上がり、長さの終わりまで指数的に下がる
  - `fn alert(level: AlertLevel) -> Vec<i16>`
- 完了の条件
  - 単体テスト: 各 level の長さ (サンプル数)、最大値が i16 の範囲に収まる、立ち上がりの形
  - 外部のライブラリを使わない

### W3 Rust: BGM の受信と戻し (フェーズ A)

- 触る: `crates/eq-server/src/broadcast/mixer/bgm.rs` (新規)、`Cargo.toml` (symphonia を足す。mp3 の機能だけ)
- 内容
  - Icecast の MP3 (`https://eq.fuga.jp/stream/bgm.mp3`) を受けて、44.1kHz・ステレオ・i16 に戻す
  - 途切れたら 5 秒後につなぎ直す。応答の大きさは受けながら処理する (`net.rs` の上限は使わない)
- 完了の条件
  - 単体テスト: 手元の短い MP3 (テスト用に数秒ぶんを tools で作って置く) を戻すと、サンプル数が長さと合う
  - 本物の配信でつなぎ直せることは、手で確かめる

### W4 Rust: 混ぜる部分と、CDP の知らせの受け口 (フェーズ A。W1〜W3 のあと)

- 触る: `crates/eq-server/src/broadcast/mixer/mod.rs` (新規), `broadcast/chrome.rs`, `broadcast/mod.rs`
- 内容
  - CDP の `Runtime.addBinding` で `eqBroadcast` を作り、`Runtime.bindingCalled` を受けて AudioMixer へ渡す
  - AudioMixer: BGM (音量、流し始めは 1 秒かけて上げる) と警戒音を足して、飽和させる
  - 実時間に合わせて 20ms ごとに PCM を出す
  - 出した PCM は、今の名前付きパイプの仕組み (`audio.rs`) で ffmpeg に渡す (形式は s16le 44.1kHz)
- 完了の条件
  - 単体テスト: 混ぜたときの飽和、音量、1 秒の立ち上がり
  - Mac で、BlackHole と sox 無しに、BGM と警戒音 (デモの場面) が録画に入る (音の欠け 0。v0.13.2 と同じ測り方)

### W5 Rust: 部品のつなぎ目 (フェーズ B)

- 触る: `crates/eq-server/src/broadcast/{mod.rs, source/, encoder/}`
- 内容
  - 3 章の trait を入れ、Chrome の screencast を `source/chrome.rs`、ffmpeg を `encoder/ffmpeg.rs` に移す
  - `broadcast.toml` に `source` / `encoder` を足す (既定は今と同じ)
- 完了の条件
  - 設定を変えなければ、今と同じ引数で ffmpeg が起動する (単体テストで ffmpeg の引数を比べる)
  - Mac でファイル出力の確認が通る

### W6 Pi 5 の計測と既定値 (フェーズ C、S2。実機が手に入るまで保留)

- 触る: `docs/broadcast-pi.md` (新規)、`broadcast.example.toml`
- 内容
  - Pi OS (64bit) で Chromium・ffmpeg・PulseAudio の null sink を用意する手順を書く
  - fps・preset ごとに CPU・温度・メモリを 1 時間測る
  - systemd のユニット例を置く
- 完了の条件: 表にした計測結果と、推奨の設定

### W7 Mac: ScreenCaptureKit + VideoToolbox (フェーズ C、S1 が良ければ)

- 触る: `crates/eq-server/src/broadcast/source/sck.rs`, `encoder/videotoolbox.rs`, `encoder/flv.rs`, `encoder/rtmp.rs`
  (macOS のときだけ組み込む。objc2 系のクレートを使う)
- 完了の条件
  - CPU の合計が、今の Chrome + ffmpeg (約 130%) の半分以下
  - YouTube (限定公開) で 1 時間途切れない

## 8. 決めごと (担当の間で合わせるもの)

- 音の形式: 44.1kHz、ステレオ、i16、L R の交互
- 配信のページへの知らせは、W1 の JSON だけ。追加するときはこの文書を先に直す
- 秘密 (ストリームキー) は環境変数で渡し、ログには出さない (`redact`)。部品を増やしても、この決まりを守る
- 子のプロセスは `kill_on_drop` で止める。止める合図 (Ctrl+C・SIGTERM) で全部止まること
- テスト: 純粋な関数 (合成・混ぜる・引数作り) は必ずテストする。
  実機 (ブラウザ・YouTube・Pi) の確認は手で行い、PR に結果を書く
- ライセンス: このリポジトリは GPL-3.0-or-later。
  - x264 (GPL) と openh264 (BSD) は使ってよい
  - fdk-aac は GPL と両立しないので使わない (Linux の AAC は ffmpeg の内蔵 aac を使う)

## 9. 未解決

- e2 の通信料 (GCP の料金表で確かめる)。e2 案を考え直すときの前提になる
- ScreenCaptureKit は「画面収録」の許可が要る。常時動かすときに、再起動のたびに許可が要らないか (S1 で確かめる)
- YouTube の RTMP の受け口は Opus を受けるか (受けるなら、AAC 無しで ffmpeg を外せる)。S3 のついでに調べる
