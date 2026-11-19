# youtube_chat_archive の判断と文面の確かめ (python3 deploy/test_youtube_chat_archive.py)
# いつ流すか (decide・calm) と落とされた回数 (after_drop) は twitch_chat_archive と共通 (test_twitch_chat_archive.py)
from youtube_chat_archive import MAX_LEN, chat_body, live_chat, message

stream = lambda sid, title, status="active", health="good": {
    "id": sid, "snippet": {"title": title}, "status": {"streamStatus": status, "healthStatus": {"status": health}}}
cast = lambda title, bound, life="live", chat="CHAT": {
    "snippet": {"title": title, **({"liveChatId": chat} if chat else {})},
    "status": {"lifeCycleStatus": life}, "contentDetails": {"boundStreamId": bound}}

# チャット: 名前の合う受け口が受信中で、そこにひもづいた live の枠のチャット
streams = [stream("s1", "ほかの受け口"), stream("s2", "地震モニター用")]
assert live_chat(streams, [cast("別の枠", "s1", chat="OTHER"), cast("地震", "s2")], "地震モニター用") == ("CHAT", "live「地震」")
# 届いていない・枠が終わっている・チャットが無いときは流さない (理由は見張りと同じ文)
chat, why = live_chat([stream("s2", "地震モニター用", status="inactive")], [cast("地震", "s2")], "地震モニター用")
assert chat is None and "受け口に映像が届いていない" in why, why
chat, why = live_chat(streams, [cast("地震", "s2", life="complete")], "地震モニター用")
assert chat is None and "live の枠が無い" in why, why
chat, why = live_chat(streams, [cast("地震", "s2", chat=None)], "地震モニター用")
assert chat is None and "チャット" in why, why

# 送る本文: textMessageEvent
assert chat_body("CHAT", "こんにちは") == {
    "snippet": {"liveChatId": "CHAT", "type": "textMessageEvent", "textMessageDetails": {"messageText": "こんにちは"}}}

# 文面は Twitch と同じ。YouTube のチャットは 200 文字まで
assert MAX_LEN == 200
up = {"state": "uploaded", "youtube_id": "ID", "uploaded_ms": 1,
      "quakes": [{"name": "あ" * 600, "max_scale": 30, "origin_ms": 1792886340000}]}
for kind in ("new", "again"):
    m = message(up, kind)
    assert len(m) <= MAX_LEN and "https://youtu.be/ID" in m, m
up["quakes"][0]["name"] = "日向灘"
assert message(up, "new") == "【地震の記録】10月25日 8時59分ごろ 日向灘 最大震度3 の再現動画を上げました https://youtu.be/ID"
print("ok")
