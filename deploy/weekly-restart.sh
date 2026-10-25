#!/bin/bash
# 週 1 回、メンテナンスの時間 (n2 のお断り: 毎日 04:00〜04:15) に配信 (eq-broadcast) をつなぎ直す。
# 土曜に地震の対応中なら日曜に回し、日曜も対応中ならその週はあきらめる。
# 「対応中でない」= 配信の状態のファイル (broadcast/calm_state.rs) が新しく (2 分以内)、平時になってから 30 分以上たっている。
# つなぎ直しと Discord への知らせは restart-broadcast.sh (配信が詰まっている間は見送る・15 分に 1 回まで)。
# 使い方: weekly-restart.sh  (タイマーから土曜・日曜の 04:05 に)
set -u
here=$(dirname "$0")
state=${XDG_STATE_HOME:-$HOME/.local/state}
done_file="$state/weekly-restart.week"
week=$(date +%G-W%V)
if [ "$(cat "$done_file" 2>/dev/null)" = "$week" ]; then
  echo "今週 ($week) は済んでいる"
  exit 0
fi
# ミリ秒。%3N は使わない (Ubuntu 26.04 の uutils の date では効かず、ナノ秒の 19 桁が出る)
now=$(( $(date +%s%N) / 1000000 ))
read -r calm since updated < <(jq -r '"\(.calm) \(.since_ms) \(.updated_ms)"' "$state/eq-broadcast/state.json" 2>/dev/null)
quiet=0
if [ "${calm:-}" = true ] && [ $((now - updated)) -le 120000 ] && [ $((now - since)) -ge 1800000 ]; then
  quiet=1
fi
if [ $quiet = 0 ]; then
  if [ "$(date +%u)" = 7 ]; then
    echo "日曜も地震の対応中 (または配信の状態がわからない) なので、今週 ($week) はあきらめる"
    echo "$week" > "$done_file"
    "$here/notify-discord.sh" "週 1 回の配信のつなぎ直しは、今週は見送りました (土日とも地震の対応中、または配信の状態がわからない)"
  else
    echo "地震の対応中 (または配信の状態がわからない) なので、日曜に回す"
  fi
  exit 0
fi
"$here/restart-broadcast.sh" "週 1 回の定期のつなぎ直し (メンテナンスの時間)" && echo "$week" > "$done_file"
