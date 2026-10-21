#!/bin/bash
# Discord の Webhook に 1 行の知らせを送る。Webhook が設定されていなければ何もしない。失敗しても見張りは止めない。
# 使い方: notify-discord.sh <本文>。Webhook の URL は ~/.config/eq-notify (600) の DISCORD_WEBHOOK_URL (EQ_NOTIFY_FILE で差し替え可)
set -u
f=${EQ_NOTIFY_FILE:-$HOME/.config/eq-notify}
url=$(sed -n 's/^DISCORD_WEBHOOK_URL=//p' "$f" 2>/dev/null | tail -1)
[ -n "$url" ] || exit 0
body=$(jq -n --arg c "[$(hostname)] $1" '{content: $c}')
code=$(curl -s -o /dev/null -w '%{http_code}' -m 20 -H 'Content-Type: application/json' -d "$body" "$url")
case $code in
2??) ;;
*) echo "Discord に知らせられない (HTTP $code)" >&2 ;;
esac
exit 0
