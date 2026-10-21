# twitch_live_check.judge の確かめ (python3 deploy/test_twitch_live_check.py)
from twitch_live_check import judge

assert judge([{"type": "live", "title": "地震モニター", "viewer_count": 2}]) == (True, "live「地震モニター」 視聴者 2")
assert not judge([])[0]
assert not judge([{"type": ""}])[0]  # 配信中でない (type が空) ものは数えない
print("ok")
