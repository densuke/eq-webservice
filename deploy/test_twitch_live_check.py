# twitch_live_check.judge の確かめ (python3 deploy/test_twitch_live_check.py)
from twitch_live_check import MARGIN_SEC, fresh, judge

assert judge([{"type": "live", "title": "地震モニター", "viewer_count": 2}]) == (True, "live「地震モニター」 視聴者 2")
assert not judge([])[0]
assert not judge([{"type": ""}])[0]  # 配信中でない (type が空) ものは数えない
# 取っておいたトークン: 同じアプリで、期限まで 1 日より余裕があるときだけ使う
c = {"client_id": "a", "access_token": "t", "expires_at": 1_000_000}
assert fresh(c, "a", 1_000_000 - MARGIN_SEC - 1) == "t"
assert fresh(c, "a", 1_000_000 - MARGIN_SEC) is None
assert fresh(c, "b", 0) is None  # Client ID を替えたら取り直す
assert fresh(None, "a", 0) is None
print("ok")
