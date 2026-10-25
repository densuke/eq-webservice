# twitch_chat_archive の判断と文面の確かめ (python3 deploy/test_twitch_chat_archive.py)
from twitch_chat_archive import EVERY_MS, STALE_MS, calm, decide, latest_upload, message

H = 3_600_000
q = lambda name, scale, origin=0: {"name": name, "max_scale": scale, "origin_ms": origin}
up = lambda yid, ms, quakes: {"state": "uploaded", "youtube_id": yid, "uploaded_ms": ms, "quakes": quakes}

# 最新の動画: 上げたものの中で uploaded_ms が最大。上げる前 (done・youtube_id なし) は除く
jobs = [up("a", 10, [q("x", 30)]), up("b", 30, [q("y", 30)]),
        {"state": "done", "youtube_id": None, "uploaded_ms": None, "quakes": [q("z", 30)]},
        {"state": "uploaded", "youtube_id": None, "uploaded_ms": 99, "quakes": []}]
assert latest_upload(jobs)["youtube_id"] == "b"
assert latest_upload([]) is None

# 平時か: calm が true で、書き込みが新しいときだけ
assert calm({"calm": True, "updated_ms": 1000}, 1000 + STALE_MS)
assert not calm({"calm": True, "updated_ms": 1000}, 1001 + STALE_MS)  # 古い = わからない
assert not calm({"calm": False, "updated_ms": 1000}, 1000)
assert not calm(None, 0)

# 流すか: 新着なら "new"、案内済みなら前回から EVERY_MS 以上で "again"
assert decide(None, "b", 0) == "new"
assert decide({"announced_youtube_id": "a", "last_post_ms": 0}, "b", 1) == "new"
assert decide({"announced_youtube_id": "b", "last_post_ms": 0}, "b", EVERY_MS - 1) is None
assert decide({"announced_youtube_id": "b", "last_post_ms": 0}, "b", EVERY_MS) == "again"
assert decide(None, None, 0) is None

# 文面: JST の日時・地名・最大震度・URL。複数なら「ほか N 件」、新着と再掲で頭を変える
origin = 1792886340000  # 2026-10-25 08:59 JST
m = message(up("ID", 1, [q("日向灘", 30, origin)]), "new")
assert m == "【地震の記録】10月25日 8時59分ごろ 日向灘 最大震度3 の再現動画を上げました https://youtu.be/ID", m
m = message(up("ID", 1, [q("日向灘", 30, origin), q("大隅半島東方沖", 45, origin + H)]), "again")
assert m.startswith("【地震の記録】最新の再現動画: 10月25日 8時59分ごろ 日向灘 ほか 1 件 最大震度5弱 "), m
assert "最大震度" not in message(up("ID", 1, [q("不明", -1, origin)]), "new")  # 震度がわからなければ出さない
assert len(message(up("ID", 1, [q("あ" * 600, 30, origin)]), "new")) <= 500
print("ok")
