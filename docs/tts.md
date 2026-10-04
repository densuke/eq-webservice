# 音声アナウンス (Google TTS)

地震・緊急地震速報・津波の情報を Google Cloud Text-to-Speech (Neural2) で読み上げる。読み上げ先はブラウザと配信 mixer。
無料枠 (月 100 万字) に収めるため、文を部品に分けて部品ごとにディスクへキャッシュし、有限の部品は起動時に事前合成する。


## S1. 文の部品
- 1 つの部品 = 1 回の合成 = 1 つのキャッシュファイル。
- 可変部分は部品の中に埋め込む (例: 「震源は能登半島沖。」で 1 部品)。値の範囲が有限なので、キャッシュは発散しない。
- 部品と部品の間には 150ms の無音を入れる。

`fn segments(ev: &Event) -> Vec<String>` の規則は次のとおり。空の Vec は「読まない」を意味する。

**ヘルパー**
- `place(h)`:
  - `h.name` が空でなければ「震源は{name}。」
  - 空、または hypocenter が無ければ「震源は調査中です。」
- `mag(h)`: `magnitude` が `Some(m)` で `m > 0` のときだけ「マグニチュード{m:.1}。」。それ以外は部品なし。
- `max(s)`: `s.is_known()` のときだけ「最大震度{読み}。」。
  - 読みは `scale_text(s)` で作る。基本は `s.label()` だが、Scale(46) だけは括弧が読み上げられないよう「5弱以上と推定」にする。

**Eew**
- 訓練報 (`test == true`) は読まない。ただし `ev.source` が "replay" または "demo" の場合は読む (alert.ts と同じ)。
- `cancelled`: ["先ほどの緊急地震速報は取り消されました。"]
- `warning`: ["緊急地震速報。", place, `max(s)` を「予想される最大震度は{label}。」に替えたもの (震度が既知のとき), "強い揺れに警戒してください。"]
- 予報: ["緊急地震速報、予報。", place, 予想最大震度 (既知のとき)]

**Quake** (`info_type` で分ける)
- `ScalePrompt`: ["震度速報。", max, 「{pref}などで揺れを観測しました。」 (`pref_max[0].pref`、無ければ省く)]
- `Destination`: ["震源に関する情報。", place, mag, tsunami]
- `ScaleAndDestination` / `DetailScale`: ["地震情報。", place, max, mag, tsunami]
- `Foreign`: ["遠地地震に関する情報。", place, mag, tsunami]
- `Other`: 読まない。
- `tsunami` の部品は `domestic_tsunami` で決める。

| domestic_tsunami | 部品 |
|---|---|
| "None" | この地震による津波の心配はありません。 |
| "Checking" | 津波の有無は現在調査中です。 |
| "NonEffective" | 若干の海面変動があるかもしれませんが、被害の心配はありません。 |
| "Watch" / "Warning" | 津波警報などが発表されています。 |
| それ以外 | なし |

**Tsunami**
- `cancelled`: ["津波予報は解除されました。"]
- それ以外: grade の重い順 (MajorWarning → Warning → Watch) に、その grade の区域があれば次を並べる。
  1. 「{grade.label()}を発表しました。」
  2. 区域ごとに「{area.name}。」
     - Watch は先頭 10 区域まで。それを超えたら「ほかの地域。」を足す。
     - MajorWarning と Warning は全区域を読む。
- 最後に、MajorWarning か Warning があれば「海岸から離れ、高台に避難してください。」を足す。
- 区域が 0 件なら読まない。

**EewDetection / Userquake**: 読まない。

`fn fixed_segments() -> Vec<String>`: prewarm 用に、地名を含まない全部品を返す。
- 固定句
- 「最大震度{l}。」と「予想される最大震度は{l}。」(震度 9 種 + 5弱以上(推定))
- 「マグニチュード0.1。」〜「マグニチュード9.9。」(0.1 刻み)
- domestic_tsunami の各文
- 津波の grade 文

## S2. いつ読むか
**チャイム (警戒音) が鳴るときに読む。** 判定ロジックは新しく作らない。
- ブラウザ: `web/src/main.ts` で `alertLevel()` が非 null を返したイベント、または `tsunamiAlert()` が非 null のとき。
- 配信 (native): `broadcast/replay/sound.rs` の `alert_level()` (再生と同じ判定) が音を出すとき。津波の報は警戒音なしで読み上げだけ送る (`alert_level` は津波を扱わないため)。接続直後の hello の報では鳴らさない。
- 読み上げキューは同時に 1 本だけ再生する。待っている間に同じ地震の新しい報が来たら、古いほうは捨てて新しいほうだけ残す (EEW の続報の連打対策)。

## S3. 音声形式とキャッシュ
- Google には `audioEncoding=LINEAR16`, `sampleRateHertz=44100` で頼む。
- 応答は WAV ヘッダ付きの mono i16 で、base64 になっている。
- キャッシュの保存先は `{cache_dir}/seg/{hex(sha256(voice + "\n" + text))}.wav` (mono 44.1kHz)。
  - 書き込みは tmp ファイルに書いてから rename する。
- 同じ部品を同時に頼まれても、合成は 1 回にする (single flight)。
- API に返す結果は mono 44.1kHz・16bit の WAV。mixer はこれを読み込み、L=R に広げて使う。

## S4. 予算 (無料枠の保護)
- `{cache_dir}/usage.json` に `{"month":"YYYY-MM","chars":N}` を記録する。month は UTC。
  - 月が変わったら 0 から数え直す。
- 合成する前に `chars + text.chars().count() > monthly_char_limit` なら `BudgetExceeded` を返し、合成しない。
- 合成に成功したときだけ加算する。キャッシュから読んだ分は数えない。
- 既定の上限は 900_000 (無料枠 100 万字の 9 割)。

## S5. HTTP API
**`GET /api/tts/event/{id}`** (公開)
- hub の直近イベントから id を探し、S1 の部品を合成・結合して WAV を返す。

| 状況 | 応答 |
|---|---|
| tts が無効 / id が無い / 部品が空 | 404 |
| 予算超過 / 合成失敗 | 503 |
| 成功 | 200 |

- 成功時のヘッダは `Content-Type: audio/wav` と `Cache-Control: public, max-age=86400`。
- キャッシュに無い部品 (一覧に無い地名など) は、その場で合成する。
  - 部品ごとの待ち時間は、EEW は 500ms、それ以外は 3 秒まで。EEW は数秒の遅れが致命的なので短くする。
  - 待ちきれなかった部品は飛ばして結合する。合成自体はバックグラウンドで続けて保存し、次の報から使えるようにする。
  - 全部品が失敗したときだけ 503。
  - 固定句 (「緊急地震速報。」など) は prewarm 済みなので、震央名が無くても初報は遅れずに読める。

**`POST /api/tts`** (窓口・要認証)。本文は `{"text": "...", "voice": "ja-JP-Neural2-C"?}`。

| 状況 | 応答 |
|---|---|
| 環境変数 `EQ_TTS_TOKEN` が未設定、または tts が無効 | 404 |
| `Authorization: Bearer <token>` が一致しない | 401 (比較は定数時間) |
| text が空 / 500 文字超 / voice が許可リスト (`ja-JP-Neural2-A`〜`D` と設定の既定の声) に無い | 400 |
| 予算超過 | 429 |
| 合成失敗 | 503 |
| 成功 | 200 (WAV) |

- voice を許可リストに絞るのは、Studio・Chirp など単価の高い声を使わせないため。
- 窓口の結果もキャッシュする (同じ文を 2 回頼んでも課金は 1 回)。

## S6. 設定 (`config.toml` の `[tts]`)
```toml
[tts]
enabled = false                 # 既定は無効 (キーが無い環境で動かないように)
voice = "ja-JP-Neural2-B"
cache_dir = "data/tts"
monthly_char_limit = 900000
prewarm = true
```
- 秘密情報は環境変数だけから読む。
  - `GOOGLE_TTS_API_KEY`: 未設定なら、`enabled = true` でも警告を出して無効扱いにする。
  - `EQ_TTS_TOKEN`
- API キーは `X-Goog-Api-Key` ヘッダで送り、URL やログには出さない。
- 構造体は既存と同じく `#[serde(deny_unknown_fields, default)]` にする。

## S7. 配信 mixer
- native 配信で読み上げを流すには、`[broadcast]` に `voice = true` を書く (既定は false)。
  サーバの `[tts]` が無効 (またはキー未設定) のときに true にすると、読み上げの取得が 404 になるので、tts を有効にした環境でだけ true にする。
  false のときも警戒音は鳴る。万一 404 が返っても、mixer は debug ログを 1 行出して諦める (warn にしない)。
- 津波は、予報が出た・等級が上がったとき (ブラウザの `tsunamiAlert` と同じ規則) だけ読む。続報・解除・古い予報の遅れた到着では読まない。
- `Notice` に `Voice { url: String }` を足す。JSON では `{"type":"voice","url":"..."}`。
- mixer は url を取得し、WAV を読み込んで読み上げキューに積む。
- 読み上げは順番に 1 本ずつ流す。読み上げ中は BGM の音量に `DUCK = 0.3` を掛ける。警戒音は読み上げと重なってもよい。
- キューの上限は 4。超えたら古いものから捨てる。

---

## 実装の場所

| 役割 | ファイル |
|---|---|
| 部品化 (S1) | `crates/eq-server/src/tts/phrase.rs` |
| WAV の読み書き・結合 | `crates/eq-server/src/tts/wav.rs` |
| 予算 (S4) | `crates/eq-server/src/tts/budget.rs` |
| Google 呼び出し | `crates/eq-server/src/tts/google.rs` |
| キャッシュ・結合 (S3) | `crates/eq-server/src/tts/cache.rs` |
| 事前合成 | `crates/eq-server/src/tts/prewarm.rs` (地名は `epicenters.txt`, `tsunami_areas.txt`。作り方は `tools/tts_places.py`) |
| HTTP API (S5) | `crates/eq-server/src/tts/http.rs` |
| 配信 mixer (S7) | `crates/eq-server/src/broadcast/mixer/mod.rs` |
| native 配信の判定 | `crates/eq-server/src/broadcast/native/model.rs` (`live_alert`) |
| ブラウザ | `web/src/voice.ts` |

## 無料枠の見積もり

- 事前合成は約 6,000 字 (固定句、震央地名 343、津波予報区 66、都道府県 47)。キャッシュが残っていれば 2 回目以降の起動では 0 字。
- 発報時に合成が起きるのは、一覧に無い地名が出たときだけ。1 件あたり十数字。
- 窓口 (`POST /api/tts`) は 1 回 500 字まで。同じ文はキャッシュされるので、課金は 1 回だけ。
- 月 100 万字の無料枠に対して、上限 (`monthly_char_limit`) の既定は 90 万字。超えると合成を止める (キャッシュ済みの部品は引き続き使える)。

## 運用上の注意
- `{cache_dir}/seg` は、新しい地名や `POST /api/tts` の文が増えるたびに増える。掃除はしない (1 件は数十 KB 程度)。必要なら手で古いものを消す。
- `data/tts` (キャッシュと `usage.json`) はデプロイで消さずに引き継ぐ。消すと全部品を合成し直して無料枠を使い、月の使用量の記録も失う。

## 後回しにしたこと

- 地名の誤読を直す読み替え辞書。実際に聞いてから、必要になったものだけ `phrase.rs` に足す。
- 本番 (`deploy/`) で mixer を有効にし、キーを置く作業。リリースのときに行う。
