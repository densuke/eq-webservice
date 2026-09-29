#!/usr/bin/env python3
"""気象庁「震度データベース」から、地震ごとの観測点の震度を読み、P2P地震情報の観測点 (points) の形にする。

https://www.data.jma.go.jp/eqdb/data/shindo/ (画面が使う api/ に mode=event, id=<発生時刻 YYYYMMDDhhmmss> を送る)
出典: 気象庁ホームページのデータを加工して作成
"""

import json
import math
import urllib.request

from area_pref import area_pref

API = "https://www.data.jma.go.jp/eqdb/data/shindo/api/"
UA = {"User-Agent": "eq-webservice scenario builder"}
# 震度の記号 -> P2P地震情報の震度。5・6 は 1996 年 3 月までの階級 (弱・強の区別なし) で、画面では「5」「6」と出す。
# 9 (不明) は使わない
SCALE = {"1": 10, "2": 20, "3": 30, "4": 40, "A": 45, "B": 50, "C": 55, "D": 60, "7": 70, "5": 47, "6": 57}


def fetch(url: str, data: bytes | None = None, headers: dict | None = None) -> bytes:
    req = urllib.request.Request(url, data=data, headers={**UA, **(headers or {})})
    with urllib.request.urlopen(req, timeout=120) as res:
        return res.read()


def event(event_id: str) -> dict:
    """{hyp: [...], int: [{name, lat, lon, char, ...}]}"""
    boundary = "eqwebservice"
    body = "".join(f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n' for k, v in {"mode": "event", "id": event_id}.items())
    raw = fetch(API, (body + f"--{boundary}--\r\n").encode(), {"Content-Type": f"multipart/form-data; boundary={boundary}"})
    res = json.loads(raw)["res"]
    if not isinstance(res, dict):
        raise ValueError(f"震度データベースに {event_id} がありません: {res}")
    return res


def nearest_area(current: dict[str, list], lat: float, lon: float) -> str:
    # ponytail: 最寄りの観測点の細分区域を借りる (区域の境界付近では隣の区域になることがある)
    return min(current.values(), key=lambda c: math.hypot(c[1] - lat, (c[2] - lon) * math.cos(math.radians(lat))))[3]


def point(current: dict[str, list], name: str, lat: float, lon: float, scale: int) -> dict:
    """今の観測点一覧にある名前はそのまま、無いもの (廃止・移設前など) は位置と細分区域を付ける"""
    known = current.get(name)
    area = known[3] if known else nearest_area(current, lat, lon)
    p = {"pref": area_pref(area), "addr": name, "isArea": False, "scale": scale}
    if not known:
        p["station"] = {"lat": lat, "lon": lon, "area": area}
    return p


def points(event_id: str, current: dict[str, list]) -> list[dict]:
    """current は web/public/stations.json を観測点名で引けるようにしたもの。震度の大きい順"""
    out = [
        point(current, s["name"].replace("＊", ""), float(s["lat"]), float(s["lon"]), SCALE[s["char"]])
        for s in event(event_id)["int"]
        if s["char"] in SCALE
    ]
    return sorted(out, key=lambda p: -p["scale"])
