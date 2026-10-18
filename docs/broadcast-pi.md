# Raspberry Pi 5 から配信する (2026-10-17〜)

YouTube のライブ配信 (`eq-server broadcast`、native) を、n2 (GCE) から家の Raspberry Pi 5 に移した。Pi はもう 1 人のユーザーの学習用デスクトップを兼ねる。ここでは、動いている構成を例として残す。ファイルは deploy/ にあり、秘密情報 (ストリームキー・認証情報) は書いていない。

- 本体: Raspberry Pi 5 (8GB)・アクティブクーラー・有線 LAN・Ubuntu 26.04 (ffmpeg 8.0.1)。録画と再現動画の作業場所は SSD (`/opt/eq-cast`) に置き、microSD には書かない。
- pi では eq-server (サイト) を動かさない。データは eq.fuga.jp から取る。

## 1. ファイル

- `deploy/broadcast.pi.toml` → `~/work/eq-e2cast/cast.toml`。broadcast.e2.toml との違いは、`server = "https://eq.fuga.jp"`、`fps = 15` (地震)・`fps_calm = 10`、`zoom = true`、`encode` の `-threads 2`、`mixer = true`・`voice = true`、`audio_bitrate = "64k"`、`bgm_url = ""`、`[record]` を SSD に、`-progress` を tmpfs (`/run/user/<uid>`) に書くこと。
- `deploy/replay.pi.toml` → `~/work/eq-e2cast/replay.toml`。replay.e2.toml との違いは、`events = "https://eq.fuga.jp"`、`cpu_quota = "100%"`、`memory_max = "1G"`、作業場所を SSD に置くこと。
- `deploy/pi/user/` → `~/.config/systemd/user/`。
  - `eq-broadcast.service`: `Slice=eq.slice`・`CPUWeight=1000`。CPU とメモリの上限は付けない。`After=network-online.target` だけで、`PartOf` / `After=eq-server.service` は付けない (eq-server を動かさないため)。
  - `youtube-live-watch.service`: `Slice=eq.slice`。タイマーは `deploy/youtube-live-watch.timer` のまま。
  - `eq-replay-worker.service`: `Slice=eq.slice`・`CPUQuota=10%`・`MemoryMax=100M`。
  - `eq.slice.d/50-eq.conf`: `CPUWeight=1000`・`MemoryLow=512M`。
  - `app.slice.d/50-desktop-cap.conf`: 配信するユーザー自身のデスクトップの上限。`session.slice.d/` にも同じものを置く。
- `deploy/pi/etc/` → `/etc/` (同じ道筋に置く。UID は環境に合わせる)。
  - `systemd/system/user-1001.slice.d/50-desktop-cap.conf`: もう 1 人のユーザーのデスクトップの上限。
  - `systemd/system/user@1000.service.d/50-memory-low.conf`: 配信するユーザーの user@.service の `MemoryLow`。
  - `systemd/system.conf.d/50-watchdog.conf`: ハードウェアの watchdog。

ユーザーユニットの置き方は README の「e2 から常時配信する」と同じ。`/etc/` のファイルは置いたあと `sudo systemctl daemon-reload` し、watchdog は本体の再起動で効かせる。

## 2. デスクトップと同居させる考え方

配信は上限を付けずに優先し、デスクトップの側を絞る。配信が詰まると視聴者に届かなくなるが、デスクトップは遅くなっても使える。

- CPU: 配信の取り分はコア 0・1。もう 1 人のユーザーのデスクトップ (`user-<uid>.slice`) は `AllowedCPUs=2-3` で 2 コアに絞る。配信は `CPUWeight=1000` で、同じユーザーのほかの処理より先に CPU をもらう。
- メモリ: デスクトップは `MemoryHigh=4G` (超えると遅くなる)・`MemoryMax=5G` (強制終了)。配信は eq.slice の `MemoryLow=512M` で守り、足りなくなったときはデスクトップのメモリから先にスワップへ出す。systemd は cgroup v2 を memory_recursiveprot で使うので、子の `MemoryLow` は親の階層にも保護がないと効かない。そのため `user@<uid>.service` にも `MemoryLow` を付ける。
- ディスク: デスクトップは `IOWeight=50`。

デスクトップが自分の上限で絞られると、本体全体の PSI (/proc/pressure) は上がる (memory full 57 を見た)。配信は遅れていないので、配信の混雑中の札と見張りのつなぎ直しの見送りは、配信自身の cgroup の PSI を見る (docs/broadcast-status.md)。

## 3. 測定 (2026-10-17〜18)

- 平時 10fps: CPU は 1 コアの 21〜24%、送信は約 340kbps、温度は 52〜56℃。
- 地震の画面 15fps: 1 コアの 52〜72% (寄りなし)。寄りあり (`zoom = true`) ではデモで 111% (最大 155%)。
- デスクトップに 4 コアの負荷をかけても、配信は 15fps を保った (CPUWeight)。
- 24 時間で、配信の再起動は 0 回、見張りは 272 回とも OK。

## 4. ハードウェアの watchdog

`/etc/systemd/system.conf.d/50-watchdog.conf` (deploy/pi/etc/systemd/system.conf.d) で、systemd が BCM2835 の watchdog (最大 15 秒) を叩く (`RuntimeWatchdogSec=15s`)。カーネルや systemd が固まると、ハードウェアが本体をリセットする。固まってから約 30 秒で再起動し、配信が戻ることを確かめた。再起動の途中で固まったときも `RebootWatchdogSec=2min` でリセットする。

## 5. ffmpeg 8

Ubuntu 26.04 の ffmpeg 8.0.1 では、`[record]` で分けた送り出し (mpegts → flv、`-c copy`) が `-analyzeduration 3000000` では映像の寸法を掴めず (`Could not find codec parameters for stream 0 ... unspecified size`)、何も送らなかった。n2 の ffmpeg 6 では起きない。v0.48.0 のあとで 15 秒 (`15000000`) にした。それより前の版を pi で使うときは、送り出しの引数だけを書き換えるラッパーを `ffmpeg` に指定して回避する。

## 6. YouTube の超低遅延

- 配信中の放送の `latencyPreference` は ultraLow に変えられない。API が `MODIFICATION_NOT_ALLOWED` / `invalidLatencyPreferenceOptions` を返す。放送を作り直す必要がある。
- 作り直した放送は ready のまま自動では始まらない。送り出しをつなぎ直すと live になる。
- 遅延は約 3 秒になった。
