#!/usr/bin/env python3
"""twitch_chat_archive.py が配信者としてチャットに送れるよう、配信者に許可してもらう (Device Code Grant)。

使い方: twitch_chat_authorize.py <認証情報のファイル>
- 表示された URL を手元のブラウザで開き、コードを確かめて配信者のアカウントで許可する。
- トークン (リフレッシュトークンを含む) は twitch_chat_archive.TOKEN (600) に置く。値は表示しない (許可した人のログイン名だけ出す)。
- アプリは confidential なので、交換に client_secret を付ける。リフレッシュトークンに期限は無いが、
  パスワードを変えた・連携を外したときは無効になるので、これをやり直す。
- 許可を取り消すときは、Twitch の設定の「接続」からこのアプリを外す。
"""
from __future__ import annotations

import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from twitch_chat_archive import TOKEN, token_response, write_json
from twitch_live_check import read_env

SCOPES = "user:write:chat"


def post(url: str, fields: dict) -> dict:
    with urllib.request.urlopen(urllib.request.Request(url, data=urllib.parse.urlencode(fields).encode()), timeout=20) as r:
        return json.load(r)


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    env = read_env(sys.argv[1])
    cid = env["TWITCH_CLIENT_ID"]
    d = post("https://id.twitch.tv/oauth2/device", {"client_id": cid, "scopes": SCOPES})
    print(f"ブラウザで開く: {d['verification_uri']}\nコード: {d['user_code']}  ({d['expires_in'] // 60} 分以内に許可する)", flush=True)
    fields = {"client_id": cid, "client_secret": env["TWITCH_CLIENT_SECRET"], "scopes": SCOPES,
              "device_code": d["device_code"], "grant_type": "urn:ietf:params:oauth:grant-type:device_code"}
    deadline = time.time() + d["expires_in"]
    while time.time() < deadline:
        time.sleep(d.get("interval", 5))
        try:
            tok = post("https://id.twitch.tv/oauth2/token", fields)
            break
        except urllib.error.HTTPError as e:
            reason = e.read().decode(errors="replace")
            if e.code == 400 and "authorization_pending" in reason:
                continue
            print(f"NG 許可されなかった: HTTP {e.code} {reason}"[:300])
            return 1
    else:
        print("NG 時間切れ")
        return 1
    req = urllib.request.Request("https://api.twitch.tv/helix/users",
                                 headers={"Client-Id": cid, "Authorization": "Bearer " + tok["access_token"]})
    with urllib.request.urlopen(req, timeout=20) as r:
        user = json.load(r)["data"][0]
    write_json(TOKEN, token_response(tok, user["id"], user["login"]), 0o600)
    print(f"OK {user['login']} が {SCOPES} を許可した (トークンは {TOKEN})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
