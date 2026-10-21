#!/bin/bash
# 配信 (eq-broadcast) をつなぎ直し、Discord に知らせる。youtube-live-watch.sh と twitch-live-watch.sh から呼ぶ。
# YouTube と Twitch は同じ ffmpeg (tee) で送っているので、どちらの見張りがつなぎ直しても両方が数秒途切れる。
# つなぎ直しは 15 分に 1 回まで (両方の見張りで記録を共有する。両方が同時に落ちても 1 回で済む)。
# 使い方: restart-broadcast.sh <理由>。終了コードは つなぎ直した = 0・見送った = 3
set -u
here=$(dirname "$0")
state=${XDG_STATE_HOME:-$HOME/.local/state}
mkdir -p "$state"
mark="$state/eq-broadcast-restarted"
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
  exit 3
fi
last=$(cat "$mark" 2>/dev/null || echo 0)
if [ $(( $(date +%s) - last )) -lt 900 ]; then
  echo "つなぎ直しは 15 分以内に済ませたので見送る"
  exit 3
fi
echo "$1ので、配信をつなぎ直す"
date +%s > "$mark"
"$here/notify-discord.sh" "配信 (eq-broadcast) をつなぎ直します: $1 (送り先のすべてが数秒途切れます)"
systemctl --user restart eq-broadcast
