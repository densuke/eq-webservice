#!/bin/bash
# YouTube のライブ配信が視聴者に届いているかを見張り、2 回続けて届いていなければ配信をつなぎ直す (docs/quake-archive.md 3.7)。
# つなぎ直すと、待機中 (ready) で自動開始がオンの枠は live に戻る。つなぎ直しは 15 分に 1 回まで。
# 使い方: youtube-live-watch.sh <認証情報の JSON> <受け口の名前>  (systemd のタイマーから 5 分ごと)
set -u
here=$(dirname "$0")
state=${XDG_STATE_HOME:-$HOME/.local/state}/youtube-live-watch
mkdir -p "$state"
res=$(python3 "$here/youtube_live_check.py" "$1" "$2"); rc=$?
echo "$res"
case $rc in
0) rm -f "$state/ng"; exit 0 ;;
2) exit 0 ;; # 調べられない (通信・API の不調) ときは何もしない
esac
if [ ! -f "$state/ng" ]; then
  touch "$state/ng"
  exit 0
fi
# e2 全体が詰まっている (メモリかディスクの待ちが多い) 間は、つなぎ直しても戻らず負荷を足すだけなので見送る
# (2026-10-01 07:00 の詰まりで、つなぎ直しの連発が悪化させた)。値は PSI の full の 60 秒平均 (%)
busy=$(awk '/^full/{split($3,a,"="); if (a[2]+0 > 20) b=1} END{print b+0}' /proc/pressure/io /proc/pressure/memory 2>/dev/null)
if [ "$busy" = 1 ]; then
  echo "e2 全体が詰まっているので、つなぎ直しを見送る"
  exit 0
fi
last=$(cat "$state/restarted" 2>/dev/null || echo 0)
if [ $(( $(date +%s) - last )) -lt 900 ]; then
  echo "つなぎ直しは 15 分以内に済ませたので見送る"
  exit 0
fi
echo "2 回続けて届いていないので、配信をつなぎ直す"
date +%s > "$state/restarted"
rm -f "$state/ng"
systemctl --user restart eq-broadcast
