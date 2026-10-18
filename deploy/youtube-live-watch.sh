#!/bin/bash
# YouTube のライブ配信が視聴者に届いているかを見張り、2 回続けて届いていなければ配信をつなぎ直す (docs/quake-archive.md 3.7)。
# 1 回の見張りでは、NG なら 1 分おきに 3 回まで確かめ、3 回とも NG のときだけ NG と数える。
# つなぎ直すと、待機中 (ready) で自動開始がオンの枠は live に戻る。つなぎ直しは 15 分に 1 回まで。
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
# 配信が詰まっている (メモリかディスクの待ちが多い) 間は、つなぎ直しても戻らず負荷を足すだけなので見送る
# (2026-10-01 07:00 の e2 の詰まりで、つなぎ直しの連発が悪化させた)。値は PSI の full の 60 秒平均 (%)。
# 配信 (eq-broadcast) の cgroup の PSI を見る。本体全体の PSI だと、同じ本体のデスクトップが上限で絞られている間
# (pi で 2026-10-18 に memory full 57) ずっと見送り、止まった配信を戻せない。
# cgroup v2 で配信の cgroup が読めない (配信が止まっている・再起動を繰り返している) ときは、つなぎ直しが要る時なので見送らない。
# cgroup v1 (cgroup.controllers が無い) のときだけ、今までどおり本体全体を見る
cg=$(systemctl --user show -p ControlGroup --value eq-broadcast 2>/dev/null)
psi=()
if [ -n "$cg" ] && [ -r "/sys/fs/cgroup$cg/io.pressure" ]; then
  psi=("/sys/fs/cgroup$cg/io.pressure" "/sys/fs/cgroup$cg/memory.pressure")
elif [ ! -e /sys/fs/cgroup/cgroup.controllers ]; then
  psi=(/proc/pressure/io /proc/pressure/memory)
fi
busy=0
if [ ${#psi[@]} -gt 0 ]; then
  busy=$(awk '/^full/{split($3,a,"="); if (a[2]+0 > 20) b=1} END{print b+0}' "${psi[@]}" 2>/dev/null)
fi
if [ "$busy" = 1 ]; then
  echo "配信が詰まっている (${psi[*]}) ので、つなぎ直しを見送る"
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
