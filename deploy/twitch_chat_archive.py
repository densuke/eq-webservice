#!/usr/bin/env python3
"""Twitch のチャットに、最新の再現動画 (YouTube) を案内する (docs/twitch-chat-archive.md)。

使い方: twitch_chat_archive.py <認証情報のファイル> <チャンネルのログイン名> <queue のディレクトリ> [--dry-run]
- 配信中かつ平時 (eq-broadcast の state.json) のときだけ流す。新しく上がった動画は 1 回、それ以外は 60 分おきに最新の 1 本。
- 送るのは app token (twitch_live_check と共有)。配信者から user:write:chat・user:bot・channel:bot の許可が要る
  (twitch_chat_authorize.py で 1 回だけ)。トークンの値は出さない。
- --dry-run は送らずに、流すときの文面を出すだけ (状態も書かない)。
- 1 行目に結果。終了コードは 送った・流さない=0、送れなかった=1、調べられない=2。
"""
from __future__ import annotations

import datetime
import glob
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from twitch_live_check import app_token, judge, read_env, streams

STATE_HOME = os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state")
STATE = os.path.join(STATE_HOME, "twitch-chat-archive", "state.json")
BROADCAST_STATE = os.path.join(STATE_HOME, "eq-broadcast", "state.json")
# 平時の案内の間隔 (ミリ秒)
EVERY_MS = 60 * 60_000
# これより古い配信の状態は「わからない」とみなす (calm_state.rs の STALE_MS と同じ)
STALE_MS = 120_000
# crates/eq-server/src/quake/scale.rs の label() と同じ対応
SCALE = {10: "1", 20: "2", 30: "3", 40: "4", 45: "5弱", 46: "5弱以上(推定)", 50: "5強", 55: "6弱", 60: "6強", 70: "7"}
JST = datetime.timezone(datetime.timedelta(hours=9))
MAX_LEN = 500


def latest_upload(jobs: list) -> dict | None:
    """上げ終えた動画のうち、いちばん新しく上げたもの (純粋な関数)"""
    done = [j for j in jobs if j.get("state") == "uploaded" and j.get("youtube_id") and j.get("uploaded_ms")]
    return max(done, key=lambda j: j["uploaded_ms"], default=None)


def calm(snap: dict | None, now_ms: int) -> bool:
    """配信が平時と言えるか。古い・読めないときはわからないので False (純粋な関数)"""
    return bool(snap and snap.get("calm") is True and now_ms - snap.get("updated_ms", 0) <= STALE_MS)


def decide(state: dict | None, youtube_id: str | None, now_ms: int) -> str | None:
    """"new" (まだ案内していない動画)・"again" (前回から EVERY_MS たった)・None (流さない) (純粋な関数)"""
    if not youtube_id:
        return None
    if not state or state.get("announced_youtube_id") != youtube_id:
        return "new"
    return "again" if now_ms - state.get("last_post_ms", 0) >= EVERY_MS else None


def message(job: dict, kind: str) -> str:
    """チャットの文面 (純粋な関数)"""
    quakes = sorted(job.get("quakes") or [], key=lambda q: q.get("origin_ms", 0))
    first = quakes[0] if quakes else {}
    t = datetime.datetime.fromtimestamp(first.get("origin_ms", 0) / 1000, JST)
    where = first.get("name", "")
    if len(quakes) > 1:
        where += f" ほか {len(quakes) - 1} 件"
    scale = SCALE.get(max((q.get("max_scale", -1) for q in quakes), default=-1))
    what = f"{t.month}月{t.day}日 {t.hour}時{t.minute:02d}分ごろ {where}" + (f" 最大震度{scale}" if scale else "")
    url = "https://youtu.be/" + job["youtube_id"]
    head, tail = ("【地震の記録】", " の再現動画を上げました ") if kind == "new" else ("【地震の記録】最新の再現動画: ", " ")
    room = MAX_LEN - len(head) - len(tail) - len(url)
    return head + what[:room] + tail + url


def read_json(path: str) -> dict | None:
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def load_jobs(queue_dir: str) -> list:
    return [j for j in (read_json(p) for p in glob.glob(os.path.join(queue_dir, "*.json"))) if j]


def helix(env: dict, tok: str, path: str, body: dict | None = None) -> dict:
    req = urllib.request.Request(
        "https://api.twitch.tv/helix/" + path,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Client-Id": env["TWITCH_CLIENT_ID"], "Authorization": "Bearer " + tok, "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r)


def send(env: dict, tok: str, login: str, text: str) -> dict:
    users = helix(env, tok, "users?" + urllib.parse.urlencode({"login": login})).get("data", [])
    if not users:
        raise ValueError(f"ユーザーが見つからない: {login}")
    uid = users[0]["id"]
    return helix(env, tok, "chat/messages", {"broadcaster_id": uid, "sender_id": uid, "message": text})["data"][0]


def save_state(state: dict) -> None:
    os.makedirs(os.path.dirname(STATE), exist_ok=True)
    tmp = STATE + ".tmp"
    with open(tmp, "w") as f:
        json.dump(state, f)
    os.replace(tmp, STATE)


def run(env: dict, tok: str, login: str, queue_dir: str, dry_run: bool) -> int:
    now = int(time.time() * 1000)
    job = latest_upload(load_jobs(queue_dir))
    kind = decide(read_json(STATE), job and job["youtube_id"], now)
    if not kind:
        print("SKIP 流す時ではない")
        return 0
    if not calm(read_json(BROADCAST_STATE), now):
        print("SKIP 平時でない (地震の画面・配信の状態がわからない)")
        return 0
    live, why = judge(streams(env, tok, login))
    if not live:
        print("SKIP " + why)
        return 0
    text = message(job, kind)
    if dry_run:
        print("DRY " + text)
        return 0
    res = send(env, tok, login, text)
    if not res.get("is_sent"):
        print(f"NG 落とされた: {res.get('drop_reason')}"[:300])
        return 1
    save_state({"announced_youtube_id": job["youtube_id"], "last_post_ms": now})
    print(f"OK {kind} {text}")
    return 0


def main() -> int:
    args = [a for a in sys.argv[1:] if a != "--dry-run"]
    if len(args) < 3:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    dry_run = "--dry-run" in sys.argv
    try:
        env = read_env(args[0])
        try:
            return run(env, app_token(env), args[1], args[2], dry_run)
        except urllib.error.HTTPError as e:
            if e.code != 401:
                raise
            return run(env, app_token(env, renew=True), args[1], args[2], dry_run)
    except urllib.error.HTTPError as e:
        # 応答の本文はトークンを含まない (理由の文だけ)
        print(f"ERR HTTP {e.code}: {e.read().decode(errors='replace')}"[:300])
        return 1 if e.code in (400, 401, 403, 422, 429) else 2
    except (OSError, KeyError, ValueError, urllib.error.URLError) as e:
        print(f"ERR 調べられない: {type(e).__name__}: {e}"[:200])
        return 2


if __name__ == "__main__":
    sys.exit(main())
