# Twitch のチャットに再現動画を案内する (設計メモ)

2026-10-25 時点の調査と設計案。実装は `deploy/twitch_chat_archive.py` (案内)・`deploy/twitch_chat_authorize.py` (許可)・`deploy/pi/user/twitch-chat-archive.service`・`deploy/twitch-chat-archive.timer`。置き方は docs/broadcast-pi.md。

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

B なら、許可を 1 度もらえば、送るときのトークンは client credentials の app token でよく、ユーザーのリフレッシュトークンを回す仕組みが要らない。

**結果: A を使う。** 最初は B を試した。配信者本人のアカウントで 3 つのスコープ (`user:write:chat`・`user:bot`・`channel:bot`) を Device Code で許可したあと、送る人 = 配信者として app token で送ると、2026-10-25 に次の 401 が返った (app token を取り直しても同じ)。

```
The sender must have authorized the app with the user:write:chat and user:bot scopes.
```

送る人と配信者が同じだと、B は通らないと判断した。配信者の user access token (`user:write:chat` だけ) で送る。同じ日に A で 1 通目 (日向灘の再現動画の新着案内) を送り、`is_sent: true` で届いた。

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
3. `POST https://id.twitch.tv/oauth2/token` に `client_id`・`client_secret`・`scopes`・`device_code`・`grant_type=urn:ietf:params:oauth:grant-type:device_code` を `interval` 秒おきに送る。許可される前は `authorization_pending` (400) が返る。アプリは confidential (2026-10-25 に dev.twitch.tv で確認) なので `client_secret` を付ける。

3 で返ったアクセストークンとリフレッシュトークンを保存して、送るときに使う。

Authorization Code Grant でも取れるが、アプリにリダイレクト URI (`http://localhost:3000` など) を登録し、コードと `client_secret` を交換する手順が要るので、Device Code より手数が多い。

### リフレッシュトークンの寿命

- アクセストークンは約 4 時間 (応答の `expires_in` を正とする)。
- confidential クライアント (このアプリ) のリフレッシュトークンは期限なし。public クライアントのものは 30 日 (Device Code の public では使い捨てで、30 日使わないと切れる)。
- 更新 (`grant_type=refresh_token`) のたびに新しいリフレッシュトークンが返ることがあるので、毎回保存し直す。
- 本人がパスワードを変えた・アプリの連携を外したときに無効になる。そのときは 401 が返るので、Device Code の手順をやり直す。Client Secret を作り直したときに無効になるかは公式の文書に書かれていない。

### スパム扱い・制限の注意

- 1 通 500 文字まで。
- レート制限を超えると 1 時間無視される (配信者・モデレーター・VIP のチャンネルを除く)。この仕組みは 1 時間に 1〜2 通なので届かない。429 が返ったら、その回は送らずに次の回を待つ (すぐに再試行しない)。
- AutoMod やチャンネルの設定 (リンクのブロック・フォロワー限定など) で落とされると、200 で `is_sent: false` が返る。`drop_reason` をログに出し、3 回続けて落とされたら Discord に 1 度知らせる (`deploy/notify-discord.sh`)。
- チャンネルのモデレーション (2026-10-25 時点): AutoMod はレベル 2 相当 (嫌がらせ・差別・性的な内容 2、冒涜的な言葉 0)、チャットはメール認証のみ必須、フォロワー限定などはオフ。いずれも配信者本人の投稿には効かないので変えない。
- 同じ文面を短い間隔で繰り返さない。60 分おきの案内も、本数が増えれば「最新の記録」の中身が変わる。中身が同じでも 60 分空ければ、連投扱いにはならない見込み。

## 2. 案内の中身と頻度

### いつ流すか

5 分ごとのタイマーで動かし、毎回次を順に調べる。

1. Twitch が配信中か (Helix の Get Streams。`twitch_live_check.py` の `streams()`・`judge()` を使い回す)。配信中でなければ何もしない。
2. 配信が平時か。`~/.local/state/eq-broadcast/state.json` の `calm` が true で、`updated_ms` が 120 秒以内 (`calm_state.rs` の `STALE_MS` と同じ)。古い・読めないときは「わからない」として流さない。
3. 最新の再現動画 (下の 3 章の方法) が、まだ案内していないものなら、今すぐ 1 回流す。
4. そうでなければ、最後に流してから 60 分たっていれば、最新の 1 本を流す。
5. ただし、上げてから 7 日を過ぎた動画は流さない (新着としても)。案内する動画が無い間は何も流さない。地震の無い週に同じ古い動画を 1 時間ごとに流し続けないため。

地震の画面の間に上がった動画は、平時に戻ってから最初の見回り (5 分以内) で案内される。

### 文面の例

```
【地震の記録】10月25日 8時59分ごろ 日向灘 最大震度3 の再現動画を上げました https://youtu.be/<youtube_id>
【地震の記録】最新の再現動画: 10月25日 8時59分ごろ 日向灘 最大震度3 https://youtu.be/<youtube_id> 地震アーカイブ https://www.youtube.com/playlist?list=PLeTi1hiiSBto
```

- 日時は最初の地震の `origin_ms` を JST で。
- 地名は `quakes[0].name`。複数の地震を 1 本にまとめた動画は「日向灘 ほか 2 件」とする。
- 最大震度は `quakes[].max_scale` の最大を、`crates/eq-server/src/quake/scale.rs` の `label()` と同じ対応で文字にする (10→1、20→2、30→3、40→4、45→5弱、46→5弱以上(推定)、50→5強、55→6弱、60→6強、70→7、ほかは出さない)。
- URL は `https://youtu.be/<youtube_id>`。
- 平時の 60 分おきの案内は「【地震の記録】最新の再現動画: …」と頭を変え、新着の案内と見分けられるようにする。再生リスト「地震アーカイブ」の URL は定期の案内にだけ添える (新着の案内は動画 1 本だけ)。500 文字を超えるときは地名の側を切る。

## 3. どこで動かすか

### 案: Pi の小さな Python スクリプト + systemd のユーザータイマー

`twitch_live_check.py` と同じ流儀にする。

- `deploy/twitch_chat_archive.py` (本体)・`deploy/twitch-chat-archive.timer`・`deploy/pi/user/twitch-chat-archive.service`。Pi では他の見張りと同じく `~/work/eq-e2cast/tools/` に置く。
- service は `Type=oneshot`・`Slice=eq.slice`。timer は 5 分ごと。
- 認証情報は見張りと同じ `~/.config/twitch-live-watch` (`TWITCH_CLIENT_ID`・`TWITCH_CLIENT_SECRET`)。配信中かを調べるのは app token で、`read_env()`・`app_token()`・`streams()`・`judge()` は `twitch_live_check.py` から import して使い回す (トークンのキャッシュも共有。401 なら 1 回だけ取り直す)。
- 送るのは配信者の user token (`~/.config/twitch-chat-archive/token.json`)。送り先と送る人のユーザー ID は、許可のときに一緒に置く。
- 状態は `~/.local/state/twitch-chat-archive/state.json` に `{"announced_youtube_id": ..., "last_post_ms": ...}`。送れた (`is_sent: true`) ときに書き直す。落とされたときは続けて落とされた回数 (`drops`) だけを足す。
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
- app token は既存のキャッシュ (`~/.local/state/twitch-live-watch/token.json`、600) を共有する。
- 配信者のアクセストークンとリフレッシュトークン (と、ユーザー ID・ログイン名) は `~/.config/twitch-chat-archive/token.json` (600) に置く。git に入れない。期限まで 5 分を切ったか 401 なら `grant_type=refresh_token` で更新し、返った新しいリフレッシュトークンで置き直す。スクリプトはトークンの値をログ・標準出力に出さない (見張りと同じく、例外の文には URL と状態だけ)。

### 許可の手順 (配信者本人がブラウザで行う)

1. Pi で `python3 ~/work/eq-e2cast/tools/twitch_chat_authorize.py ~/.config/twitch-live-watch` を実行する。表示されるのは `verification_uri` と `user_code` だけ。
2. 手元のブラウザで `verification_uri` を開き、fugahogeds でログインして `user_code` を確かめ、`user:write:chat` を許可する。
3. スクリプトがトークンを置き、`OK fugahogeds が ... を許可した` と出して終わる。
4. テスト: 平時・配信中に `twitch_chat_archive.py ... --dry-run` で文面を確かめ、続けて `--dry-run` なしで 1 回手で動かし、チャットに 1 通出ること (`OK new ...`) を確かめる。

許可を取り消したくなったら、Twitch の設定の「接続」からアプリの連携を外す。

許可をやり直すのは、パスワードを変えた・連携を外したなどでリフレッシュトークンが無効になったとき (送るたびに `ERR HTTP 400` か `401` が出る)。B を試したときに許可した `user:bot`・`channel:bot` は使わないので、連携を外して許可し直してもよい。

## 決めたこと (2026-10-25)

- 新着の動画は 1 回だけ案内する。平時の定期案内は 60 分おき。上げてから 7 日を過ぎた動画は流さない。
- 再生リスト「地震アーカイブ」の URL は定期案内にだけ添える。
- `is_sent: false` が 3 回続いたら Discord に知らせる。落とされている間は、状態の `announced_youtube_id` を変えないので、5 分ごとに同じ動画を "new" として送り直す。
- アプリは confidential。Device Code の交換で `client_secret` を付ける。リフレッシュトークンは期限なし。

## 残している限界

- リフレッシュトークンが無効になったときは、5 分ごとに `ERR` をジャーナルに出すだけで、Discord には知らせない。
