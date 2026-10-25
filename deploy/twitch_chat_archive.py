#!/usr/bin/env python3
"""Twitch のチャットに、最新の再現動画 (YouTube) を案内する (docs/twitch-chat-archive.md)。

使い方: twitch_chat_archive.py <認証情報のファイル> <チャンネルのログイン名> <queue のディレクトリ> [--dry-run]
- 配信中かつ平時 (eq-broadcast の state.json) のときだけ流す。新しく上がった動画は 1 回、それ以外は 60 分おきに最新の 1 本。
  上げてから 7 日を過ぎた動画は流さない。
- 配信中かは app token (twitch_live_check と共有) で調べ、送るのは配信者の user token
  (twitch_chat_authorize.py が TOKEN に置く。期限が近いか 401 なら更新して置き直す)。トークンの値は出さない。
- 落とされた (is_sent: false) のが DROPS_TO_NOTIFY 回続いたら、notify-discord.sh で 1 度知らせる。
  許可が切れた (リフレッシュできない・トークンが無い) ときも 1 度知らせる (送れたら数え直す)。
- --dry-run は送らずに、流すときの文面を出すだけ (状態も書かない)。
- 1 行目に結果。終了コードは 送った・流さない=0、送れなかった=1、調べられない=2。
"""
from __future__ import annotations

import datetime
import glob
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from twitch_live_check import app_token, judge, read_env, streams

STATE_HOME = os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state")
STATE = os.path.join(STATE_HOME, "twitch-chat-archive", "state.json")
BROADCAST_STATE = os.path.join(STATE_HOME, "eq-broadcast", "state.json")
TOKEN = os.path.join(os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config"), "twitch-chat-archive", "token.json")
# 平時の案内の間隔 (ミリ秒)
EVERY_MS = 60 * 60_000
# 上げてからこれを過ぎた動画は流さない (地震の無い週に同じ動画を流し続けないため)
MAX_AGE_MS = 7 * 86_400_000
# これより古い配信の状態は「わからない」とみなす (calm_state.rs の STALE_MS と同じ)
STALE_MS = 120_000
# 落とされたのがこの回数続いたら知らせる
DROPS_TO_NOTIFY = 3
# 期限までこれを切った user token は更新してから使う (秒)
REFRESH_MARGIN_SEC = 300
# 再生リスト「地震アーカイブ」(replay.toml の youtube_playlist)
PLAYLIST_URL = "https://www.youtube.com/playlist?list=PLeTi1hiiSBto"
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


def decide(state: dict | None, job: dict | None, now_ms: int) -> str | None:
    """"new" (まだ案内していない動画)・"again" (前回から EVERY_MS たった)・None (流さない) (純粋な関数)"""
    if not job or now_ms - job["uploaded_ms"] > MAX_AGE_MS:
        return None
    if not state or state.get("announced_youtube_id") != job["youtube_id"]:
        return "new"
    return "again" if now_ms - state.get("last_post_ms", 0) >= EVERY_MS else None


def after_drop(state: dict | None) -> tuple[dict, bool]:
    """落とされた回数を 1 つ増やした状態と、ちょうど知らせる回数になったか (純粋な関数)"""
    drops = (state or {}).get("drops", 0) + 1
    return {**(state or {}), "drops": drops}, drops == DROPS_TO_NOTIFY


def after_auth_failure(state: dict | None) -> tuple[dict, bool]:
    """許可が切れた印を付けた状態と、今回はじめて切れたか (純粋な関数)"""
    return {**(state or {}), "auth_failed": True}, not (state or {}).get("auth_failed")


class AuthError(Exception):
    """配信者の許可が切れた (twitch_chat_authorize.py でやり直す)"""


def needs_refresh(tok: dict, now: float) -> bool:
    return tok.get("expires_at", 0) - now <= REFRESH_MARGIN_SEC


def message(job: dict, kind: str) -> str:
    """チャットの文面。再生リストは定期の案内 (again) にだけ添える (純粋な関数)"""
    quakes = sorted(job.get("quakes") or [], key=lambda q: q.get("origin_ms", 0))
    first = quakes[0] if quakes else {}
    t = datetime.datetime.fromtimestamp(first.get("origin_ms", 0) / 1000, JST)
    where = first.get("name", "")
    if len(quakes) > 1:
        where += f" ほか {len(quakes) - 1} 件"
    scale = SCALE.get(max((q.get("max_scale", -1) for q in quakes), default=-1))
    what = f"{t.month}月{t.day}日 {t.hour}時{t.minute:02d}分ごろ {where}" + (f" 最大震度{scale}" if scale else "")
    url = "https://youtu.be/" + job["youtube_id"]
    if kind == "new":
        head, tail = "【地震の記録】", " の再現動画を上げました " + url
    else:
        head, tail = "【地震の記録】最新の再現動画: ", f" {url} 地震アーカイブ {PLAYLIST_URL}"
    return head + what[: MAX_LEN - len(head) - len(tail)] + tail


def read_json(path: str) -> dict | None:
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def write_json(path: str, data: dict, mode: int = 0o644) -> None:
    """置き換えは一度に (途中で落ちても前のものが残る)"""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, mode)
    with os.fdopen(fd, "w") as f:
        json.dump(data, f)
    os.replace(tmp, path)


def load_jobs(queue_dir: str) -> list:
    return [j for j in (read_json(p) for p in glob.glob(os.path.join(queue_dir, "*.json"))) if j]


def token_response(tok: dict, user_id: str, login: str) -> dict:
    """id.twitch.tv の応答から、TOKEN に置く形を作る"""
    return {"access_token": tok["access_token"], "refresh_token": tok["refresh_token"],
            "expires_at": time.time() + tok["expires_in"], "user_id": user_id, "login": login}


def refresh(env: dict, tok: dict) -> dict:
    body = urllib.parse.urlencode({"grant_type": "refresh_token", "refresh_token": tok["refresh_token"],
                                   "client_id": env["TWITCH_CLIENT_ID"], "client_secret": env["TWITCH_CLIENT_SECRET"]}).encode()
    try:
        with urllib.request.urlopen(urllib.request.Request("https://id.twitch.tv/oauth2/token", data=body), timeout=20) as r:
            new = token_response(json.load(r), tok["user_id"], tok["login"])
    except urllib.error.HTTPError as e:
        if e.code in (400, 401):  # リフレッシュトークンが無効 (パスワードの変更・連携を外した)
            raise AuthError(f"リフレッシュできない (HTTP {e.code})") from e
        raise
    write_json(TOKEN, new, 0o600)  # 新しいリフレッシュトークンが返るので毎回置き直す
    return new


def send(env: dict, tok: dict, text: str) -> dict:
    body = {"broadcaster_id": tok["user_id"], "sender_id": tok["user_id"], "message": text}
    req = urllib.request.Request(
        "https://api.twitch.tv/helix/chat/messages", data=json.dumps(body).encode(),
        headers={"Client-Id": env["TWITCH_CLIENT_ID"], "Authorization": "Bearer " + tok["access_token"],
                 "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r)["data"][0]


def send_as_user(env: dict, text: str) -> dict:
    tok = read_json(TOKEN)
    if not tok:
        raise AuthError(f"配信者のトークンが無い ({TOKEN})")
    if needs_refresh(tok, time.time()):
        tok = refresh(env, tok)
    try:
        return send(env, tok, text)
    except urllib.error.HTTPError as e:
        if e.code != 401:
            raise
        return send(env, refresh(env, tok), text)


def live(env: dict, login: str) -> tuple[bool, str]:
    try:
        return judge(streams(env, app_token(env), login))
    except urllib.error.HTTPError as e:
        if e.code != 401:
            raise
        return judge(streams(env, app_token(env, renew=True), login))


def notify(text: str) -> None:
    subprocess.run([os.path.join(os.path.dirname(os.path.abspath(__file__)), "notify-discord.sh"), text], check=False)


def run(env: dict, login: str, queue_dir: str, dry_run: bool) -> int:
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
    ok, why = live(env, login)
    if not ok:
        print("SKIP " + why)
        return 0
    text = message(job, kind)
    if dry_run:
        print("DRY " + text)
        return 0
    try:
        res = send_as_user(env, text)
    except AuthError as e:
        state, tell = after_auth_failure(state)
        write_json(STATE, state)
        if tell:
            notify(f"Twitch のチャットへの案内が止まった: {e}。Pi で twitch_chat_authorize.py をやり直す")
        print(f"NG 許可が切れた: {e}。twitch_chat_authorize.py で許可し直す")
        return 1
    if not res.get("is_sent"):
        state, tell = after_drop(state)
        write_json(STATE, state)
        reason = f"{res.get('drop_reason')}"[:200]
        if tell:
            notify(f"Twitch のチャットへの案内が {DROPS_TO_NOTIFY} 回続けて落とされた: {reason}")
        print(f"NG 落とされた ({state['drops']} 回目): {reason}")
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
        return run(read_env(args[0]), args[1], args[2], "--dry-run" in sys.argv)
    except urllib.error.HTTPError as e:
        # 応答の本文はトークンを含まない (理由の文だけ)
        print(f"ERR HTTP {e.code}: {e.read().decode(errors='replace')}"[:300])
        return 1 if e.code in (400, 401, 403, 422, 429) else 2
    except (OSError, KeyError, ValueError, urllib.error.URLError) as e:
        print(f"ERR 調べられない: {type(e).__name__}: {e}"[:200])
        return 2


if __name__ == "__main__":
    sys.exit(main())
