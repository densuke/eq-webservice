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
