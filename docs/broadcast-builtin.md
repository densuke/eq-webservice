# 配信の圧縮と送り出しを eq-server に組み込む (N2、encoder = "builtin")

e2 から YouTube へ直接配信する (native・無音) とき、ffmpeg を使わずに eq-server だけで配信できるようにする。
ffmpeg の経路は既定として残し、`broadcast.toml` の `encoder` で選ぶ (出力のプラグイン)。
実験なので、うまくいかなければ設定 1 行で ffmpeg に戻せること、コードごと外せることを前提にする。

## 1. なぜやるか

- e2 に ffmpeg (依存を含め 139 パッケージ) を入れずに済む。実行ファイル 1 つで配信できる。
- ffmpeg の分のメモリ (e2 で 50〜80MB) を減らす。
- コマの時刻を自分で付けられる。可変 fps と `-maxrate` の組み合わせで画質が崩れた問題 (docs/broadcast-native.md 13 章) のような、
  ffmpeg の時刻の解釈のずれが起きない。

## 2. 範囲

作る

- Encoder のつなぎ目 (docs/broadcast-v2.md の 3 章 W5)。今の ffmpeg をその実装の 1 つに移す (動きは変えない)
- `builtin`: 映像は openh264 (`openh264` クレート、ソースから組み込む)、音は無音の AAC、FLV に詰めて RTMP / RTMPS で送る
- ファイルへの書き出し (FLV) で確かめられること

作らない

- 音のある配信 (BGM・警戒音) の builtin。AAC の圧縮器が要るので ffmpeg の経路を使う。`builtin` で `mixer = true` なら起動時にエラー
- Chrome の経路の builtin (JPEG を戻す必要がある)。`builtin` は `source = "native"` だけ

## 3. つなぎ目

```rust
/// 圧縮して送るもの。native は描き直したときの I420 の画面と、その時刻 (配信の開始からのミリ秒) を渡す
pub enum Encoder {
    Ffmpeg(ffmpeg::FfmpegEncoder), // 今の作り (子のプロセスに rawvideo を渡す)
    Builtin(builtin::BuiltinEncoder),
}

impl Encoder {
    /// 1 コマ送る。pts_ms は単調に増える (可変 fps ならコマの間隔は一定でない)
    pub async fn video(&mut self, i420: &[u8], pts_ms: u64) -> anyhow::Result<()>;
    /// 止まった理由 (送り先が切れた、ffmpeg が終わった)
    pub async fn closed(&mut self) -> anyhow::Error;
}
```

- `broadcast.toml` に `encoder` (`"ffmpeg"` 既定 / `"builtin"`) を足す。
- ffmpeg の経路の引数・動きは今と同じ (既存の ffmpeg_args のテストがそのまま通る)。
- `session()` の中の「何 ms ごとに送るか」(可変 fps) の判断は今のまま Broadcaster に置き、Encoder は渡された時刻を使うだけ。

## 4. builtin の中身

置き場所: `crates/eq-server/src/broadcast/builtin/`

- `h264.rs`: openh264 の圧縮器。I420 を受け、NAL を返す。
  - 設定: `UsageType::ScreenContentRealTime`、画質は固定の QP か目標ビットレート (設定 `builtin_bitrate`、既定 300k)。
  - キーフレームは時刻で 2 秒ごと (前のキーフレームから 2000ms 以上たったコマで強制する)。
- `aac.rs`: 無音の AAC-LC (44.1kHz・ステレオ) の 1 コマ (1024 サンプル) の決まったバイト列と、AudioSpecificConfig。
  - バイト列は ffmpeg で無音を AAC にして 1 コマ取り出したものを定数にしてよい (作り方をコメントに残す)。
  - 映像の時刻に合わせて、23.2ms ごと (1024 / 44100 秒) に音のタグを出す。
- `flv.rs`: FLV のヘッダ、onMetaData、映像 (AVC シーケンスヘッダ + NALU) と音 (AAC シーケンスヘッダ + 生データ) のタグ。
  - 映像の NAL は Annex B から長さ前置き (AVCC) にする。SPS/PPS から AVCDecoderConfigurationRecord を作る。
- `rtmp.rs`: RTMP のクライアント (handshake、connect、createStream、publish、チャンクで送る)。`rml_rtmp` を使うか自前で書くかは担当が判断してよい。
  - `rtmps://` は tokio-rustls (既存の rustls の設定) で TLS を張ってから同じ手順。
  - 切れたら Err を返す (つなぎ直しは今の run のループが 5 秒後に行う)。
  - 送り先の URL (ストリームキー入り) はログに出さない (既存の redact を使う)。
- 出力先: `output` の最初の要素が `rtmp://` か `rtmps://` なら送る。ファイルのパス (`.flv`) ならファイルに書く (確かめる用)。

## 5. 確かめ方

1. ファイル: `encoder = "builtin"`、`output = ["test.flv"]` で 60 秒書き、`ffprobe` で映像 (h264 1280x720) と音 (aac 44100 stereo) が読めること、
   コマの時刻 (可変 fps で間隔が変わる)、キーフレームが約 2 秒ごとなことを確かめる。PNG を切り出して文字がつぶれていないこと。
2. 手元の RTMP: ffmpeg を受け取る側にして (`ffmpeg -listen 1 -i rtmp://127.0.0.1:19350/live/test -c copy recv.flv`)、
   builtin から `rtmp://127.0.0.1:19350/live/test` に 60 秒送り、受け取った recv.flv を 1 と同じく確かめる。途中で受け側を止めたら Err で終わること。
3. YouTube (RTMPS): 利用者の許可を得てから、コーディネーターが行う (担当は行わない)。
4. 負荷: Mac で CPU とメモリ (eq-server だけになる) を ffmpeg の経路 (同じ設定) と比べる。

## 6. テスト (cargo test)

- flv: ヘッダ・タグの長さ・前タグ長・時刻の書き方、AVCDecoderConfigurationRecord の中身
- aac: 無音のコマが ffprobe なしでも決まった長さであること、音の時刻の刻み (23.2ms) と映像の時刻の混ぜ方 (時刻順)
- h264: 決まった I420 (単色) を圧縮してキーフレームが出ること、時刻で 2 秒ごとにキーフレームを強制すること
- rtmp: handshake とチャンクの組み立て (送るバイト列) の単体テスト。実際の通信は 5 章 2 の手での確認
- ffmpeg の経路: 既存のテストがそのまま通ること

## 7. 完了の条件

- `encoder` を省けば今と同じ (ffmpeg)。builtin は native・無音のときだけ
- 5 章 1・2・4 の結果 (ffprobe の出力、PNG、CPU とメモリの比較) を PR に書く
- `cargo audit` で新しい指摘が無いこと。openh264 のライセンス (BSD) を README の依存の説明に書く
- 利用者の配信 (e2 の eq-broadcast、`~/work/eq-broadcast`) と e2 には触らない。YouTube には送らない。PR まで作り、マージしない

## 8. v0.17.1: 地震の画面でコマが飛ばされる問題

### 8.1 見つかったこと (2026-09-30、e2)

e2 で本番の配信と同じ枠 (`systemd-run --user -p CPUQuota=25% -p MemoryMax=200M -p MemorySwapMax=0`) で、
`tools/broadcast_load.py samples/scenarios/noto2024.jsonl --speed 4 --duration 100` を builtin (`builtin_bitrate = 1000000`、fps 10・fps_calm 2、test = true) で回した。

- メモリ: 最大 75MB (問題なし)
- CPU: 平均 18%、最大 27% (eq-server だけ)
- **出たコマ: 0.2〜1.2fps** (地震の画面は 10fps、平時は 2fps のはず)。平時の部分でも 1.0fps だった
- **キーフレームの間隔: 最小 2.0 秒、最大 4.8 秒** (YouTube は 4 秒以下を求める)
- 送信量: 平均 100kbps

見当: `h264.rs` の `.skip_frames(true)` (openh264 の目標ビットレートを守るためのコマ飛ばし)。
Mac の N2 の確認でも、300k で平時 2fps の 78 コマ中 44〜70 コマしか出ていなかった。地震の画面は 1 コマが大きく、ほとんどが飛ばされる。
強制したキーフレームも飛ばされると、次のキーフレームが遅れる。

### 8.2 直すこと

1. **コマを飛ばさない**。渡したコマは必ず 1 コマ出す (`skip_frames(false)`)。
   - 画質の決め方は担当が測って選ぶ: `RateControlMode::Bitrate` のまま飛ばしなし、`Quality`、`Off` + 固定の QP など。
     選んだ理由と数値を PR に書く。条件は「全コマ出る」「平時の送信量が今 (約 160〜220kbps、e2 全体) から大きく増えない (目安 300kbps 未満)」「文字が読める」。
   - `builtin_bitrate` の意味が変わるなら、README・broadcast.example.toml・設定の説明を合わせる。設定の名前は変えない (e2 の cast.toml が使っている)。
2. **キーフレームの間隔は 2 秒ごとを守る** (前のキーフレームから 2000ms 以上たった最初のコマ)。平時 2fps で最大 2.5 秒。
3. **`tools/broadcast_load.py` を builtin でも使えるようにする**。
   - `--toml 'encoder = "builtin"'` のとき、出力を `.flv` のファイル (`output = ["…/out.flv"]`) にする (今は mpegts 固定で、builtin では使えない)。
   - `--toml` で `map_dir`・`font` を渡すと、ツールが足す行と重なって設定の読み込みで落ち、表が 0 だけになる。
     重なる key は起動前にエラーにする (`--map-dir`・`--font` を使うよう案内する)。
   - 配信側が途中で落ちたら、broadcast.log の最後の数行を出して止める (0 の表を出し続けない)。
4. Cargo.toml を 0.17.1 にする。

### 8.3 テスト (cargo test)

- h264: 画面が毎コマ大きく変わる (乱数や縞で、毎コマ違う) 1280x720 の I420 を 10fps で 60 コマ渡し、**60 コマ全部が空でない出力** になること。
  キーフレームが時刻で 2 秒ごと (0, 2000, 4000 …ms) に出ること。
- 既存のテストはそのまま通ること。

### 8.4 確かめ方 (Mac。e2・YouTube には触らない)

1. `tools/broadcast_load.py samples/scenarios/noto2024.jsonl --speed 4 --duration 100` を builtin と ffmpeg (e2 の本番と同じ encode:
   `["-threads","1","-c:v","libx264","-preset","veryfast","-tune","zerolatency","-crf","23"]`) で回し、表と最後のまとめを PR に貼る。
   - 見るもの: 地震の画面で 10fps 近く出ること、平時で 2fps、キーフレームの間隔の最大、平均・最大の送信量、CPU、最大メモリ。
2. 平時 (`--server https://eq.fuga.jp --duration 90`、読むだけ) も builtin で回し、送信量とコマ数を書く。
3. 地震の画面の PNG を 1 枚切り出し、文字がつぶれていないこと。
4. CPU の内訳の見当: builtin の地震の画面で、描く (native) と圧縮 (openh264) のどちらが重いか。
   測る仕組みを常設する必要はない (一時的な計測でよい。コミットしない)。PR に数値だけ書く。

### 8.5 完了の条件

- 8.3 のテストが通る。CI (Linux・macOS の rust、audit、coverage、web) が全部通る
- 8.4 の結果を PR に書く
- e2 の eq-broadcast、`~/work/eq-broadcast`、e2、YouTube には触らない。PR まで作り、マージしない

## 9. e2 での結果と判断 (2026-09-30)

e2 (e2-micro、本番と同じ枠 `CPUQuota=25%`・`MemoryMax=200M`) で、
`tools/broadcast_load.py samples/scenarios/noto2024.jsonl --speed 4 --duration 100` (fps 10・fps_calm 2) を比べた。

- **builtin (v0.17.1、コマ飛ばしなし、`builtin_bitrate = 1000000`)**
  - 平時は 2.0fps、地震の画面は **約 3fps** (10fps に届かない)。
  - キーフレームの間隔は 2.0〜2.5 秒。
  - CPU は平均 16%、最大メモリ 75MB、送信量は平均 144kbps。
- **ffmpeg (libx264 veryfast・zerolatency・CRF 23・`-threads 1`)**
  - 平時は 2fps、地震の画面は **9〜10fps**。
  - キーフレームの間隔は 1.6〜2.4 秒。
  - CPU は平均 13.7% (eq-server 3.5% + ffmpeg 10.3%)、最大メモリ 126MB (eq-server 49 + ffmpeg 77)、送信量は平均 251kbps。
- 原因
  - openh264 の圧縮が、libx264 (veryfast、1 スレッド) の約 2 倍の CPU を使う。Mac では 1 コマ約 9.3ms。
  - 描く側 (native) は軽い。e2 の CPU では、10fps の圧縮に追いつかない。
- 平時の配信 (2fps) は、builtin でも問題なかった。
  - CPU 7%、RSS 約 70MB。
  - YouTube の遅延は 5〜6 秒で、ffmpeg (約 7 秒) より短い。
- **判断**
  - e2 の常時配信は ffmpeg に戻した (2026-09-30 15:07)。
  - builtin は、ffmpeg を入れられない、または CPU に余裕のある機械 (Pi 5 など) のために残す。
  - `cast-builtin.toml` (e2 の `~/work/eq-e2cast`) に設定を残してある。
- v0.17.0 の builtin は `skip_frames(true)` のため、地震の画面で 0.2〜1.2fps・キーフレーム最大 4.8 秒だった (8 章)。v0.17.1 で直した。
