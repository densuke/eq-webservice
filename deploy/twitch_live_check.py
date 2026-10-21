#!/usr/bin/env python3
"""Twitch のライブ配信が配信中かを調べる (読み取りだけ。deploy/ に置き、twitch-live-watch.sh から呼ぶ)。

使い方: twitch_live_check.py <認証情報のファイル> <チャンネルのログイン名>
- 認証情報は KEY=VALUE の行 (TWITCH_CLIENT_ID・TWITCH_CLIENT_SECRET)。アプリのトークン (client credentials) を毎回取る。
  値は出さない。
- 1 行目に結果 (OK / NG と理由)、終了コードは OK=0・NG=1・調べられない=2。
- Helix の Get Streams は、配信中なら 1 件、止まっていれば空を返す。
"""
from __future__ import annotations

import json
import sys
import urllib.error
import urllib.parse
import urllib.request


def read_env(path: str) -> dict:
    with open(path) as f:
        return dict(line.strip().split("=", 1) for line in f if "=" in line and not line.startswith("#"))


def app_token(env: dict) -> str:
    body = urllib.parse.urlencode({
        "client_id": env["TWITCH_CLIENT_ID"],
        "client_secret": env["TWITCH_CLIENT_SECRET"],
        "grant_type": "client_credentials",
    }).encode()
    with urllib.request.urlopen(urllib.request.Request("https://id.twitch.tv/oauth2/token", data=body), timeout=20) as r:
        return json.load(r)["access_token"]


def streams(env: dict, tok: str, login: str) -> list:
    url = "https://api.twitch.tv/helix/streams?" + urllib.parse.urlencode({"user_login": login})
    req = urllib.request.Request(url, headers={"Client-Id": env["TWITCH_CLIENT_ID"], "Authorization": "Bearer " + tok})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r).get("data", [])


def judge(data: list) -> tuple[bool, str]:
    """Get Streams の data から、配信中かを決める (純粋な関数)"""
    live = [s for s in data if s.get("type") == "live"]
    if not live:
        return False, "配信中でない (Get Streams が空)"
    s = live[0]
    return True, f"live「{s.get('title', '')}」 視聴者 {s.get('viewer_count', '?')}"


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    try:
        env = read_env(sys.argv[1])
        data = streams(env, app_token(env), sys.argv[2])
    except (OSError, KeyError, ValueError, urllib.error.URLError) as e:
        # 例外の文に認証情報は入らない (URL と応答の状態だけ)
        print(f"ERR 調べられない: {type(e).__name__}: {e}"[:200])
        return 2
    ok, why = judge(data)
    print(("OK " if ok else "NG ") + why)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
