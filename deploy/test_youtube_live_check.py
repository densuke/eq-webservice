# youtube_live_check.judge の確かめ (python3 deploy/test_youtube_live_check.py)
from youtube_live_check import judge

def stream(i, title, status, health):
    return {"id": i, "snippet": {"title": title}, "status": {"streamStatus": status, "healthStatus": {"status": health}}}

def bc(title, life, sid):
    return {"snippet": {"title": title}, "status": {"lifeCycleStatus": life}, "contentDetails": {"boundStreamId": sid}}

s = [stream("a", "地震モニター用", "active", "good"), stream("b", "Default", "inactive", "noData")]
assert judge(s, [bc("x", "live", "a")], "地震モニター用")[0]
assert not judge(s, [bc("x", "complete", "a"), bc("y", "ready", "a")], "地震モニター用")[0]  # 2026-10-01 01:15 の状態
assert not judge(s, [bc("x", "live", "b")], "地震モニター用")[0]  # 別の受け口の枠
assert not judge([stream("a", "地震モニター用", "inactive", "noData")], [bc("x", "live", "a")], "地震モニター用")[0]
assert not judge([stream("a", "地震モニター用", "active", "bad")], [bc("x", "live", "a")], "地震モニター用")[0]
print("ok")
