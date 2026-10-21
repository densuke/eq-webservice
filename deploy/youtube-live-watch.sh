#!/bin/bash
# YouTube のライブ配信が視聴者に届いているかを見張り、2 回続けて届いていなければ配信をつなぎ直す (docs/quake-archive.md 3.7)。
# 1 回の見張りでは、NG なら 1 分おきに 3 回まで確かめ、3 回とも NG のときだけ NG と数える。
# つなぎ直すと、待機中 (ready) で自動開始がオンの枠は live に戻る。つなぎ直し (と Discord への知らせ) は restart-broadcast.sh。
# 使い方: youtube-live-watch.sh <認証情報の JSON> <受け口の名前>  (systemd のタイマーから 5 分ごと)
set -u
here=$(dirname "$0")
state=${XDG_STATE_HOME:-$HOME/.local/state}/youtube-live-watch
mkdir -p "$state"
res=$(python3 "$here/youtube_live_check.py" "$1" "$2"); rc=$?
# YouTube の状態は、こちらが送り続けていても数十秒だけ「届いていない」に揺れることがある (2026-10-05 n2 で確認)。
# NG のときは 1 分おきにあと 2 回確かめ、一度でも OK なら届いているとみなす
for _ in 1 2; do
  [ $rc -eq 1 ] || break
  echo "$res (1 分後に確かめ直す)"
  sleep 60
  res=$(python3 "$here/youtube_live_check.py" "$1" "$2"); rc=$?
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
"$here/restart-broadcast.sh" "YouTube に 2 回続けて届いていない" && rm -f "$state/ng"
