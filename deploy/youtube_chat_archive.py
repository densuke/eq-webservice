#!/usr/bin/env python3
"""YouTube のライブ配信のチャットに、最新の再現動画を案内する (docs/youtube-chat-archive.md)。

使い方: youtube_chat_archive.py <認証情報の JSON> <受け口の名前> <queue のディレクトリ> [--dry-run]
- いつ流すか (配信中かつ平時、新しい動画は 1 回、ほかは 60 分おき、7 日を過ぎた動画は流さない) と文面は
  twitch_chat_archive と同じ。状態のファイルは別 (Twitch と YouTube は別々に数える)。
- 認証情報は youtube_live_check と同じ google-auth の authorized user 形式 (youtube か youtube.force-ssl の範囲)。
  読むだけで書き戻さない。値は出さない。投稿者はチャンネル本人。
- 使う割り当ては 1 回の案内で 22 単位 (liveStreams.list・liveBroadcasts.list 各 1 と liveChatMessages.insert 20)。
  流さないときは API を呼ばない。
- 送れないのが DROPS_TO_NOTIFY 回続いたら、notify-discord.sh で 1 度知らせる。許可が切れた (invalid_grant) ときも 1 度知らせる。
- --dry-run は送らずに、流すときの文面を出すだけ (状態も書かない)。
- 1 行目に結果。終了コードは 送った・流さない=0、送れなかった=1、調べられない=2。
"""
from __future__ import annotations

import json
import os
import sys
import time
import urllib.error
import urllib.request

import twitch_chat_archive as tca
from twitch_chat_archive import (BROADCAST_STATE, DROPS_TO_NOTIFY, after_auth_failure, after_drop, calm, decide,
                                 latest_upload, load_jobs, notify, read_json, write_json)
from youtube_live_check import API, access_token, get, judge

STATE = os.path.join(tca.STATE_HOME, "youtube-chat-archive", "state.json")
# YouTube のチャットは 1 通 200 文字まで
MAX_LEN = 200


def message(job: dict, kind: str) -> str:
    """文面は Twitch と同じで、200 文字に収める (純粋な関数)"""
    return tca.message(job, kind, MAX_LEN)


def live_chat(streams: list, broadcasts: list, name: str) -> tuple[str | None, str]:
    """視聴者に届いている枠のチャット ID と、その説明。届いていない・チャットが無いなら None と理由 (純粋な関数)"""
    ok, why = judge(streams, broadcasts, name)
    if not ok:
        return None, why
    sid = next(s["id"] for s in streams if s["status"]["streamStatus"] == "active" and s["snippet"]["title"] == name)
    b = next(b for b in broadcasts
             if b["status"]["lifeCycleStatus"] == "live" and b.get("contentDetails", {}).get("boundStreamId") == sid)
    chat = b["snippet"].get("liveChatId")
    title = f"live「{b['snippet']['title']}」"
    return (chat, title) if chat else (None, title + " にチャットが無い (チャットを切っている)")


def chat_body(chat_id: str, text: str) -> dict:
    return {"snippet": {"liveChatId": chat_id, "type": "textMessageEvent", "textMessageDetails": {"messageText": text}}}


def post(tok: str, chat_id: str, text: str) -> None:
    req = urllib.request.Request(
        API + "liveChat/messages?part=snippet", data=json.dumps(chat_body(chat_id, text)).encode(),
        headers={"Authorization": "Bearer " + tok, "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=20):
        pass


class AuthError(Exception):
    """許可が切れた (リフレッシュトークンが無効)"""


def token(cred_path: str) -> str:
    with open(cred_path) as f:
        cred = json.load(f)
    try:
        return access_token(cred)
    except urllib.error.HTTPError as e:
        if e.code in (400, 401):  # invalid_grant (許可を外した・期限切れ)
            raise AuthError(f"リフレッシュできない (HTTP {e.code})") from e
        raise


def run(cred_path: str, name: str, queue_dir: str, dry_run: bool) -> int:
    now = int(time.time() * 1000)
    state = read_json(STATE)
    job = latest_upload(load_jobs(queue_dir))
    kind = decide(state, job, now)
    if not kind:
        print("SKIP 流す時ではない")
        return 0
    if not calm(read_json(BROADCAST_STATE), now):
        print("SKIP 平時でない (地震の画面・配信の状態がわからない)")
        return 0
    try:
        tok = token(cred_path)
    except AuthError as e:
        state, tell = after_auth_failure(state)
        write_json(STATE, state)
        if tell:
            notify(f"YouTube のチャットへの案内が止まった: {e}。Pi の {cred_path} の許可を取り直す")
        print(f"NG 許可が切れた: {e}")
        return 1
    streams = get(tok, "liveStreams?part=snippet,status&mine=true").get("items", [])
    broadcasts = get(tok, "liveBroadcasts?part=snippet,status,contentDetails&mine=true&broadcastType=all&maxResults=10").get("items", [])
    chat, why = live_chat(streams, broadcasts, name)
    if not chat:
        print("SKIP " + why)
        return 0
    text = message(job, kind)
    if dry_run:
        print(f"DRY {why} {text}")
        return 0
    try:
        post(tok, chat, text)
    except urllib.error.HTTPError as e:
        # 応答の本文はトークンを含まない (理由の文だけ)
        reason = f"HTTP {e.code}: {e.read().decode(errors='replace')}"[:200]
        state, tell = after_drop(state)
        write_json(STATE, state)
        if tell:
            notify(f"YouTube のチャットへの案内が {DROPS_TO_NOTIFY} 回続けて送れなかった: {reason}")
        print(f"NG 送れなかった ({state['drops']} 回目): {reason}")
        return 1
    write_json(STATE, {"announced_youtube_id": job["youtube_id"], "last_post_ms": now})
    print(f"OK {kind} {text}")
    return 0


def main() -> int:
    args = [a for a in sys.argv[1:] if a != "--dry-run"]
    if len(args) < 3:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    try:
        return run(args[0], args[1], args[2], "--dry-run" in sys.argv)
    except urllib.error.HTTPError as e:
        print(f"ERR HTTP {e.code}: {e.read().decode(errors='replace')}"[:300])
        return 2
    except (OSError, KeyError, ValueError, StopIteration, urllib.error.URLError) as e:
        # 例外の文に認証情報は入らない (URL と応答の状態だけ)
        print(f"ERR 調べられない: {type(e).__name__}: {e}"[:200])
        return 2


if __name__ == "__main__":
    sys.exit(main())
