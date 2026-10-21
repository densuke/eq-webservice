#!/bin/bash
# Twitch のライブ配信が配信中かを見張り、2 回続けて止まっていたら配信をつなぎ直す (Discord にも知らせる。restart-broadcast.sh)。
# YouTube と同じ ffmpeg (tee) で送っているので、つなぎ直すと YouTube も数秒途切れる。
# 1 回の見張りでは、NG なら 1 分おきに 3 回まで確かめ、3 回とも NG のときだけ NG と数える (youtube-live-watch.sh と同じ)。
# 使い方: twitch-live-watch.sh <認証情報のファイル> <チャンネルのログイン名>  (systemd のタイマーから 5 分ごと)
set -u
here=$(dirname "$0")
state=${XDG_STATE_HOME:-$HOME/.local/state}/twitch-live-watch
mkdir -p "$state"
res=$(python3 "$here/twitch_live_check.py" "$1" "$2"); rc=$?
for _ in 1 2; do
  [ $rc -eq 1 ] || break
  echo "$res (1 分後に確かめ直す)"
  sleep 60
  res=$(python3 "$here/twitch_live_check.py" "$1" "$2"); rc=$?
done
echo "$res"
case $rc in
0) rm -f "$state/ng"; exit 0 ;;
2) exit 0 ;; # 調べられない (通信・API の不調) ときは何もしない
esac
if [ ! -f "$state/ng" ]; then
  touch "$state/ng"
  exit 0
fi
"$here/restart-broadcast.sh" "Twitch が 2 回続けて配信中でない" && rm -f "$state/ng"
