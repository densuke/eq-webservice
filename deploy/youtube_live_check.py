#!/usr/bin/env python3
"""YouTube のライブ配信が視聴者に届いているかを調べる (読み取りだけ。docs/quake-archive.md 3.7。deploy/ に置き、youtube-live-watch.sh から呼ぶ)。

使い方: youtube_live_check.py <認証情報の JSON> [受け口の名前]
- 認証情報は google-auth の authorized user 形式 (client_id・client_secret・refresh_token・token_uri)。
  読むだけで書き戻さない。値は出さない。
- 1 行目に結果 (OK / NG と理由)、終了コードは OK=0・NG=1・調べられない=2。
- 使う割り当ては 1 回 2 単位 (liveStreams.list と liveBroadcasts.list)。
"""
from __future__ import annotations

import json
import sys
import urllib.error
import urllib.parse
import urllib.request

API = "https://www.googleapis.com/youtube/v3/"


def access_token(cred: dict) -> str:
    body = urllib.parse.urlencode({
        "client_id": cred["client_id"],
        "client_secret": cred["client_secret"],
        "refresh_token": cred["refresh_token"],
        "grant_type": "refresh_token",
    }).encode()
    with urllib.request.urlopen(urllib.request.Request(cred["token_uri"], data=body), timeout=20) as r:
        return json.load(r)["access_token"]


def get(tok: str, path: str) -> dict:
    req = urllib.request.Request(API + path, headers={"Authorization": "Bearer " + tok})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r)


def judge(streams: list, broadcasts: list, name: str | None) -> tuple[bool, str]:
    """受け口 (stream) と枠 (broadcast) から、視聴者に届いているかを決める (純粋な関数)"""
    active = [s for s in streams if s["status"]["streamStatus"] == "active" and (name is None or s["snippet"]["title"] == name)]
    if not active:
        return False, "受け口に映像が届いていない (streamStatus が active でない)"
    s = active[0]
    health = s["status"].get("healthStatus", {}).get("status", "?")
    live = [b for b in broadcasts
            if b["status"]["lifeCycleStatus"] == "live" and b.get("contentDetails", {}).get("boundStreamId") == s["id"]]
    if not live:
        return False, f"受け口は受信中 ({health}) だが、ひもづいた live の枠が無い (枠が終了している)"
    if health in ("bad", "noData"):
        return False, f"枠は live だが受け口の健全性が {health}"
    return True, f"live「{live[0]['snippet']['title']}」 健全性 {health}"


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    name = sys.argv[2] if len(sys.argv) > 2 else None
    try:
        with open(sys.argv[1]) as f:
            tok = access_token(json.load(f))
        streams = get(tok, "liveStreams?part=snippet,status&mine=true").get("items", [])
        broadcasts = get(tok, "liveBroadcasts?part=snippet,status,contentDetails&mine=true&broadcastType=all&maxResults=10").get("items", [])
    except (OSError, KeyError, ValueError, urllib.error.URLError) as e:
        # 例外の文に認証情報は入らない (URL と応答の状態だけ)
        print(f"ERR 調べられない: {type(e).__name__}: {e}"[:200])
        return 2
    ok, why = judge(streams, broadcasts, name)
    print(("OK " if ok else "NG ") + why)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
