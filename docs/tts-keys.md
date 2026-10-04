# 音声アナウンスの鍵の作り方

音声アナウンス ([tts.md](tts.md)) には、環境変数で渡す秘密情報が 2 つあります。

| 変数 | 用途 | 無いとき |
|---|---|---|
| `GOOGLE_TTS_API_KEY` | Google Cloud Text-to-Speech を呼ぶ API キー | 読み上げ全体が無効になる (警告ログを出して起動は続ける) |
| `EQ_TTS_TOKEN` | 任意の文を読ませる窓口 `POST /api/tts` の Bearer トークン | 窓口だけが無効になる (404)。地震の読み上げは動く |

どちらも設定ファイル (`config.toml`) やリポジトリには書きません。

## 1. GOOGLE_TTS_API_KEY

### 1-1. プロジェクトと API を用意する

1. [Google Cloud コンソール](https://console.cloud.google.com/) で、プロジェクトを作るか既存のものを選ぶ。
   例: `eq-webservice`
2. 「お支払い」でプロジェクトに請求先アカウントを紐づける。
   - 無料枠の範囲で使う場合でも、Text-to-Speech は請求先が無いと有効にできない。
3. 「API とサービス」→「ライブラリ」で **Cloud Text-to-Speech API** を探し、「有効にする」。

### 1-2. API キーを作り、使える範囲を絞る

1. 「API とサービス」→「認証情報」→「認証情報を作成」→「API キー」。
2. 作成されたキーの編集画面で、次の 2 つの制限を付ける。
   - **API の制限**: 「キーを制限」→ **Cloud Text-to-Speech API** だけを選ぶ。
     - キーが漏れても、他の API には使えなくなる。
   - **アプリケーションの制限**: 「IP アドレス」→ 本番サーバ (n2) のグローバル IP を登録する。
     - 手元で試すときは、手元の IP を一時的に足す。
3. 保存し、キーの値を控える。画面以外には残さない。

### 1-3. 使いすぎの歯止め (推奨)

アプリ側にも月の上限 (`monthly_char_limit`、既定 90 万字) がありますが、Google 側にも歯止めを掛けておきます。

- **予算アラート**: 「お支払い」→「予算とアラート」で、このプロジェクトに少額 (例: 100 円) の予算を作り、50%・100% で通知する。
  - 無料枠を超えて課金が始まったら、すぐ気づける。
  - ただし予算アラートは通知だけで、利用は止まらない。
- **割り当て (クォータ)**: 「API とサービス」→ Cloud Text-to-Speech API →「割り当てとシステム上限」で、1 分あたりのリクエスト数を下げる (例: 60)。
  - 事前合成は 200ms 間隔なので、毎分 300 回まで出る。
  - 下げると事前合成に時間はかかるが、失敗した部品は次の起動で合成し直される。

### 1-4. 料金の確認

Neural2 / WaveNet の無料枠は月 100 万字です。課金は文字数単位で、空白や句読点も数えます。
料金と無料枠は変わることがあるので、有効にする前に [料金のページ](https://cloud.google.com/text-to-speech/pricing) で確かめてください。

## 2. EQ_TTS_TOKEN

外部のサービスは不要です。推測できない十分に長い乱数を作ります。

```bash
openssl rand -hex 32
```

64 文字の 16 進文字列が出ます。これをそのままトークンにします。
窓口を使う側 (スクリプトなど) にも同じ値を渡します。

窓口を使わないなら、作らなくてかまいません (窓口が 404 になるだけ)。

## 3. 本番サーバに置く

`deploy/eq-server.service` は `/etc/default/eq-server` を `EnvironmentFile` として読みます。
そこに 2 つを書き、権限を 600 にします。

```bash
sudo install -m 600 -o root -g root /dev/null /etc/default/eq-server   # 無ければ作る (既にあれば不要)
sudoedit /etc/default/eq-server
```

追記する内容 (値は自分のものに置き換える):

```sh
GOOGLE_TTS_API_KEY=AIza...
EQ_TTS_TOKEN=0123abcd...
```

ユーザー単位の systemd ユニットで動かしている場合は、`~/.config/` の下など、そのユニットの `EnvironmentFile` に書きます。
`eq-broadcast.service` のストリームキーと同じ置き方です。

あわせて `config.toml` で読み上げを有効にします。

```toml
[tts]
enabled = true
```

最後に再起動し、ログを確かめます。

```bash
sudo systemctl restart eq-server
journalctl -u eq-server -n 50 | grep tts
```

- `tts enabled voice=ja-JP-Neural2-B` と出れば有効です。
- 続けて `tts prewarm 50/...` と事前合成の進み具合が出ます。
- `GOOGLE_TTS_API_KEY is not set` と出たら、環境変数が渡っていません。

## 4. 動作確認

窓口を設定した場合は、次のコマンドで WAV が取れれば鍵 2 つとも正しく通っています。

```bash
curl -fsS -H "Authorization: Bearer $EQ_TTS_TOKEN" -H "Content-Type: application/json" \
  -d '{"text":"音声アナウンスのテストです。"}' http://127.0.0.1:9995/api/tts -o test.wav
```

| 応答 | 原因 |
|---|---|
| 401 | トークンが違う |
| 404 | トークン未設定、または読み上げが無効 |
| 503 | Google の呼び出しに失敗 (キーの値、API の有効化、IP 制限を確認) |
| 429 | 月の上限に達した |

ポート (例では 9995) は、実際の待ち受けに合わせてください。

## 5. 鍵を替えるとき

- **API キー**: 新しいキーを作って `/etc/default/eq-server` を書き換え、再起動してから古いキーを削除する。
  - キャッシュ (`data/tts/`) はキーと関係ないので、そのまま使える。
- **トークン**: `openssl rand -hex 32` で作り直し、サーバと窓口を使う側の両方を書き換える。
- 漏れた疑いがあるときは、先に古いものを無効にする (Google 側でキーを削除、トークンは書き換えて再起動)。
