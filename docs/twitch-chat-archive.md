# Twitch のチャットに再現動画を案内する (設計メモ)

2026-10-25 時点の調査と設計案。実装・本番の変更・Twitch への投稿はまだしていない。

## ねらい

Twitch のライブ配信 (家の Pi の eq-broadcast から YouTube と同時に送っているもの) のチャット欄に、直近に YouTube へ上げた再現動画 (地震の記録) への案内を流す。

- 新しい再現動画が上がったら、そのあと平時に戻った最初の機会に 1 回。
- 平時は 60 分に 1 回、「最新の地震の記録」を 1 本。
- 地震の画面の間は流さない。配信が止まっているときも流さない。

## 1. Twitch のチャットへ送る方法 (公式ドキュメントで確認したこと)

### API

Helix の Send Chat Message (`POST https://api.twitch.tv/helix/chat/messages`) を使う。IRC (TMI) は使わない。

- 本文: `broadcaster_id` (送り先のチャンネルのユーザー ID)・`sender_id` (送る人のユーザー ID)・`message` (500 文字まで)。ほかに `reply_parent_message_id`・`for_source_only` (共有チャットのとき元のチャンネルだけに出す。app access token のときだけ使える)。
- 応答: `data[0]` に `message_id`・`is_sent`・`drop_reason` (`code`・`message`)。HTTP 200 でも `is_sent` が false なら落とされている (AutoMod・リンクの禁止など)。
- 状態コード: 200 送れた、400 ID の誤り・組み合わせの誤り、401 トークンかスコープが足りない、403 送る人がそのチャットで話せない、422 長すぎる、429 レート制限。

### 認可は 2 通り

公式の参照に書かれている条件は次のどちらか。

- A. **user access token** に `user:write:chat` がある (送る人本人のトークン)。
- B. **app access token** で、アプリが `sender_id` の人から `user:write:chat` と `user:bot` を、`broadcaster_id` の人から `channel:bot` を許可されている (送る人がそのチャンネルのモデレーターなら `channel:bot` は要らない)。

B なら、許可を 1 度もらえば、送るときのトークンは client credentials の app token でよい。これは Twitch の見張り (`deploy/twitch_live_check.py`) が既に取って `~/.local/state/twitch-live-watch/token.json` にとってある (約 60 日、期限前と 401 のときに取り直す)。ユーザーのリフレッシュトークンを持ち続けて回す仕組みが要らない。

**案: B を使う。** 許可は配信者本人のアカウントで、3 つのスコープ (`user:write:chat`・`user:bot`・`channel:bot`) をまとめて 1 回でもらう。送る人と配信者が同じとき `channel:bot` が要るかどうかは参照の書き方からは読み切れないので、3 つとも許可しておく。

要確認 (最初に 1 通送るときに確かめる): 送る人 = 配信者で B の app token が通るか。通らなければ A に切り替える (下の「A に切り替えるとき」)。

### 送る主体

**案: 配信者本人のアカウント (fugahogeds)。**

- 許可が 1 回で済む。別のアカウントを作る・モデレーターにする手間が要らない。
- 配信者の投稿のリンクはそのまま通る。2026-10-25 に配信者がチャット欄 (Web) から `https://youtube.com/@fugahoge873/live` を投稿し、リンクとして表示されることを確かめた。API から送った場合も同じかは、最初の送信で確かめる。チャンネルの「ハイパーリンクをブロック」は一般の視聴者だけを止める、とされている (Twitch 公式の文書では未確認)。別の bot アカウントなら、リンクを通すためにモデレーターにする必要がある。
- レート制限は配信者・モデレーター・VIP の枠 (30 秒に 100 通)。この仕組みは 1 時間に 1〜2 通なので、どちらでも余裕がある。

引き換えに、チャットボットのバッジ (Chat Bot Badge) は付かない。バッジの条件は「Send Chat Message で送る・app access token・配信者から `channel:bot` (かモデレーター)・**送る人が配信者ではない**」で、本人のアカウントでは満たせない。バッジが欲しくなったら、bot 用のアカウントを作り、そのアカウントから `user:write:chat`・`user:bot`、配信者から `channel:bot` をもらい、モデレーターにする (リンクを通すため)。

### トークンの取り方

Device Code Grant を使う。Pi に画面がなくても、表示された URL とコードを手元のスマホや Mac のブラウザで開いて許可すればよい。リダイレクト URI の登録も、受け口のサーバーも要らない。

1. `POST https://id.twitch.tv/oauth2/device` に `client_id` と `scopes` (空白区切り)。応答の `device_code`・`user_code`・`verification_uri`・`interval`・`expires_in`。
2. 人が `verification_uri` を開き、`user_code` を確かめて許可する。
3. `POST https://id.twitch.tv/oauth2/token` に `client_id`・`scopes`・`device_code`・`grant_type=urn:ietf:params:oauth:grant-type:device_code` を `interval` 秒おきに送る。許可される前は `authorization_pending` (400) が返る。登録済みのアプリが confidential なら `client_secret` も付ける (公開クライアントなら不要。どちらかは dev.twitch.tv のアプリの設定で確認する)。

B の案では、3 で返ったトークンは使わない (許可を確定させるために交換までは行う)。保存しないで捨ててよい。

Authorization Code Grant でも取れるが、アプリにリダイレクト URI (`http://localhost:3000` など) を登録し、コードと `client_secret` を交換する手順が要るので、Device Code より手数が多い。

### リフレッシュトークンの寿命 (A に切り替えたときに要る話)

- アクセストークンは約 4 時間 (応答の `expires_in` を正とする)。
- confidential クライアントのリフレッシュトークンは期限なし。public クライアントのものは 30 日 (Device Code の public では使い捨てで、30 日使わないと切れる)。
- 更新 (`grant_type=refresh_token`) のたびに新しいリフレッシュトークンが返ることがあるので、毎回保存し直す。
- 本人がパスワードを変えた・アプリの連携を外したときに無効になる。そのときは 401 が返るので、Device Code の手順をやり直す。Client Secret を作り直したときに無効になるかは公式の文書に書かれていない。

### スパム扱い・制限の注意

- 1 通 500 文字まで。
- レート制限を超えると 1 時間無視される (配信者・モデレーター・VIP のチャンネルを除く)。この仕組みは 1 時間に 1〜2 通なので届かない。429 が返ったら、その回は送らずに次の回を待つ (すぐに再試行しない)。
- AutoMod やチャンネルの設定 (リンクのブロック・フォロワー限定など) で落とされると、200 で `is_sent: false` が返る。`drop_reason` をログに出し、続けて落とされたら Discord に知らせる。
- 同じ文面を短い間隔で繰り返さない。60 分おきの案内も、本数が増えれば「最新の記録」の中身が変わる。中身が同じでも 60 分空ければ、連投扱いにはならない見込み。

## 2. 案内の中身と頻度

### いつ流すか

5 分ごとのタイマーで動かし、毎回次を順に調べる。

1. Twitch が配信中か (Helix の Get Streams。`twitch_live_check.py` の `streams()`・`judge()` を使い回す)。配信中でなければ何もしない。
2. 配信が平時か。`~/.local/state/eq-broadcast/state.json` の `calm` が true で、`updated_ms` が 120 秒以内 (`calm_state.rs` の `STALE_MS` と同じ)。古い・読めないときは「わからない」として流さない。
3. 最新の再現動画 (下の 3 章の方法) が、まだ案内していないものなら、今すぐ 1 回流す。
4. そうでなければ、最後に流してから 60 分たっていれば、最新の 1 本を流す。

地震の画面の間に上がった動画は、平時に戻ってから最初の見回り (5 分以内) で案内される。

### 文面の例

```
【地震の記録】10月25日 8時59分ごろ 日向灘 最大震度3 の再現動画を YouTube に上げました https://youtu.be/<youtube_id>
```

- 日時は最初の地震の `origin_ms` を JST で。
- 地名は `quakes[0].name`。複数の地震を 1 本にまとめた動画は「日向灘 ほか 2 件」とする。
- 最大震度は `quakes[].max_scale` の最大を、`crates/eq-server/src/quake/scale.rs` の `label()` と同じ対応で文字にする (10→1、20→2、30→3、40→4、45→5弱、46→5弱以上(推定)、50→5強、55→6弱、60→6強、70→7、ほかは出さない)。
- URL は `https://youtu.be/<youtube_id>`。
- 平時の 60 分おきの案内は「【地震の記録】最新の再現動画: …」と頭を変え、新着の案内と見分けられるようにする。再生リスト「地震アーカイブ」(`https://www.youtube.com/playlist?list=PLeTi1hiiSBto`) を添えてもよい (2 つの URL で 500 文字を超えないこと)。

## 3. どこで動かすか

### 案: Pi の小さな Python スクリプト + systemd のユーザータイマー

`twitch_live_check.py` と同じ流儀にする。

- `deploy/twitch_chat_archive.py` (本体)・`deploy/twitch-chat-archive.timer`・`deploy/pi/user/twitch-chat-archive.service`。Pi では他の見張りと同じく `~/work/eq-e2cast/tools/` に置く。
- service は `Type=oneshot`・`Slice=eq.slice`。timer は 5 分ごと。
- 認証情報は見張りと同じ `~/.config/twitch-live-watch` (`TWITCH_CLIENT_ID`・`TWITCH_CLIENT_SECRET`)。`read_env()`・`app_token()` は `twitch_live_check.py` から import して使い回す (トークンのキャッシュも共有。401 なら 1 回だけ取り直す)。
- 送る先と送る人のユーザー ID は、初回に `GET /helix/users?login=fugahogeds` で調べて設定に書く (秘密ではない)。
- 状態は `~/.local/state/twitch-chat-archive/state.json` に `{"announced_youtube_id": ..., "last_post_ms": ...}`。送れた (`is_sent: true`) ときだけ書き直す。
- 判断 (流すか・何を流すか・文面) は純粋な関数にして、`deploy/test_twitch_live_check.py` と同じように単体テストを付ける。送信は外とのやり取りだけ。

### queue/*.json から最新の動画を選ぶ

`/opt/eq-cast/replay/queue/*.json` を全部読み、`state == "uploaded"` かつ `youtube_id` があるものの中から `uploaded_ms` が最大のものを選ぶ。

ファイル名や更新時刻で選ばない。作り終えて上げる前のもの (`state: "done"`・`youtube_id: null`、1 日の上限で翌日に回されたものなど) が新しい名前で並ぶことがある (2026-10-25 の `20261025-111000.json` がそう)。

使う項目: `youtube_id`・`uploaded_ms`・`quakes[].origin_ms`・`quakes[].name`・`quakes[].max_scale`。形は `crates/eq-server/src/broadcast/replay/worker/queue.rs` の `Job`。

### eq-server 本体に入れる案との比較

- 本体 (replay-worker) なら、上げた瞬間を知っているので queue を読み直さずに済む。ただ、Twitch の認可・チャットの都合を本体に持ち込み、Rust 側の変更・リリース・Pi への差し替えが要る。配信の平時判定も別プロセス (eq-broadcast) の状態ファイルを読むことになり、得は小さい。
- スクリプトなら、見張りと同じ置き方で、本体を触らずに足し外しできる。5 分の遅れは案内として問題にならない。

**案: スクリプト。**

## 4. 秘密の置き場所と、ユーザーの操作が要る手順

### 置き場所

- `TWITCH_CLIENT_ID`・`TWITCH_CLIENT_SECRET`: 既存の Pi の `~/.config/twitch-live-watch` (権限 600)。増やさない。
- B の案では、ユーザーのトークンを保存しない。app token は既存のキャッシュ (`~/.local/state/twitch-live-watch/token.json`、600) を共有する。
- A に切り替えたときだけ、`~/.config/twitch-chat-archive/token.json` (600) にアクセストークンとリフレッシュトークンを置く。git に入れない。スクリプトはトークンの値をログ・標準出力に出さない (見張りと同じく、例外の文には URL と状態だけ)。

### 許可の手順 (配信者本人がブラウザで行う)

1. (確認) dev.twitch.tv の Console で、登録済みのアプリの「クライアントの種類」(Confidential / Public) を見る。Device Code の交換で `client_secret` を付けるかがこれで決まる。
2. Pi で許可用の小さなスクリプト (実装時に `deploy/twitch_chat_authorize.py` として作る) を実行する。表示されるのは `verification_uri` と `user_code` だけ。
3. 手元のブラウザで `verification_uri` を開き、fugahogeds でログインして `user_code` を確かめ、3 つのスコープ (`user:write:chat`・`user:bot`・`channel:bot`) を許可する。
4. スクリプトが許可を確認して終わる。B の案ならトークンは保存しない。
5. テスト: 平時・配信中に本番のスクリプトを 1 回手で動かし、チャットに 1 通出ること、`is_sent: true` を確かめる。

許可を取り消したくなったら、Twitch の設定の「接続」からアプリの連携を外す。

### A に切り替えるとき

送る人 = 配信者で B の app token が 401 か 403 になったら、user access token で送る。

- 2〜4 の手順で返ったアクセストークン・リフレッシュトークンを `~/.config/twitch-chat-archive/token.json` (600) に保存する。
- 送るときに期限 (保存した `expires_at`) が近いか 401 なら、`grant_type=refresh_token` で更新し、返った新しいリフレッシュトークンで上書きする。
- リフレッシュトークンが 401 で無効になったら Discord に知らせ、許可の手順をやり直す。

## 決めずに残していること

- 案内の間隔 (60 分) と、新着の案内に再生リストの URL を添えるか。
- `is_sent: false` が何回続いたら Discord に知らせるか (案: 3 回)。
- 古い動画 (例: 上げてから 7 日以上) は、平時の案内から外すか。
