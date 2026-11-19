# YouTube のライブ配信のチャットに再現動画を案内する

2026-11-19。Twitch のチャットへの案内 (docs/twitch-chat-archive.md) と同じことを、YouTube のライブ配信のチャットでも行う。実装は `deploy/youtube_chat_archive.py`・`deploy/pi/user/youtube-chat-archive.service`・`deploy/youtube-chat-archive.timer`。置き方は docs/broadcast-pi.md。

## いつ流すか・文面

Twitch と同じ (`twitch_chat_archive` の `decide`・`calm`・`message` を使う)。

- 配信中かつ平時 (eq-broadcast の state.json) のときだけ。新しく上がった動画は 1 回、それ以外は 60 分おきに最新の 1 本。上げてから 7 日を過ぎた動画は流さない。
- 文面も同じ。ただし YouTube のチャットは 1 通 200 文字までなので、地名を 200 文字に収まるよう切る (Twitch は 500 文字)。
- 状態のファイルは Twitch と別 (`~/.local/state/youtube-chat-archive/state.json`)。片方が送れなくても、もう片方は流す。

## API (公式ドキュメントで確認、2026-11-19)

- 投稿は `liveChatMessages.insert` (`POST https://www.googleapis.com/youtube/v3/liveChat/messages?part=snippet`)。本文は `snippet.liveChatId`・`snippet.type = textMessageEvent`・`snippet.textMessageDetails.messageText`。
- 許可の範囲は `youtube` か `youtube.force-ssl`。YouTube の見張りと同じ `~/.config/pd2/youtube-upload-token.json` (範囲は `youtube.upload` と `youtube`) を読むだけで使う。投稿者はチャンネル本人 (配信者のバッジが付く)。
- チャット ID は、名前の合う受け口 (`liveStreams.list`) が受信中で、そこにひもづいた live の枠 (`liveBroadcasts.list`) の `snippet.liveChatId`。判定は見張り (`youtube_live_check.judge`) と同じ。チャットを切っている枠には流さない。
- 割り当て: `liveChatMessages.insert` は 20 単位、list は各 1 単位。1 回の案内で 22 単位、1 日 (1 時間おき) で約 530 単位。流さないときは API を呼ばない。1 日の割り当ては 10,000 単位 (pd2 の OAuth クライアントと共有。動画の投稿 `videos.insert` は別枠)。
- 速すぎると `rateLimitExceeded` が返る (数値の上限は公開されていない)。1 時間に 1〜2 通なので当たらない見込み。

## 失敗したとき

- 送れない (HTTP の失敗) のが 3 回続いたら、`notify-discord.sh` で 1 度知らせる。送れたら数え直す。
- 許可が切れた (リフレッシュトークンが無効) ときも 1 度知らせる。pd2 と同じトークンなので、pd2 の許可を取り直せば両方直る。
- 確かめ方: `python3 youtube_chat_archive.py <認証情報> 地震モニター用 /opt/eq-cast/replay/queue --dry-run` で、送らずに枠と文面を出す (状態も書かない)。
