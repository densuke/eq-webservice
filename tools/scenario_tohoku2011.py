#!/usr/bin/env python3
"""デモの場面「東北地方太平洋沖地震 (2011-03-11) 本震」を当時の発表から作る。

P2P地震情報の履歴 API は 2015 年以降しか無いので、気象庁の資料から組み立てる。
  - 緊急地震速報: 気象庁「緊急地震速報（警報）発表状況」の各報 (jma_eew.py)
  - 震源と M: 「災害時地震・津波速報 平成23年東北地方太平洋沖地震」の発表の経過
    (14:49 M7.9 速報値 → 16:00 M8.4 → 17:30 M8.8 → 13日12:55 M9.0)。震源の位置・深さは確定値を使う
  - 各地の震度: 気象庁「震度データベース」の観測値 (確定値。当時の発表の順・時刻ではないので、16:00 の発表にまとめる)
  - 津波警報・注意報: 同速報の表「発表時刻と予想される津波の高さ」(14:49〜12日03:20 の 8 回) と 13日17:58 の解除。
    12日13:50〜13日07:30 の切り下げは内訳が表に無いので省く
出典: 気象庁ホームページのデータを加工して作成
出力: samples/scenarios/tohoku2011.jsonl

使い方: cd tools && uv run --with pypdf python scenario_tohoku2011.py ../web/public/stations.json ../samples/scenarios/tohoku2011.jsonl
"""

import io
import json
import math
import re
import sys
import urllib.request

from area_pref import area_pref
from jma_eew import to_wolfx

EQDB = "https://www.data.jma.go.jp/eqdb/data/shindo/api/"
SAIGAIJI = "https://www.jma.go.jp/jma/kishou/books/saigaiji/saigaiji_201101/saigaiji_201101_01.pdf"
# 震度データベースの地震の ID と、緊急地震速報の発表状況のページの ID
EVENT = "20110311144618"
EEW_ID = "20110311144640"
NAME = "三陸沖"
ORIGIN = "2011/03/11 14:46:18"
LAT, LON, DEPTH = 38.1, 142.9, 24
# (発表時刻, M, 各地の震度を付けるか)
REPORTS = [
    ("2011/03/11 14:49:00", 7.9, False),
    ("2011/03/11 16:00:00", 8.4, True),
    ("2011/03/11 17:30:00", 8.8, False),
    ("2011/03/13 12:55:00", 9.0, False),
]
TSUNAMI_TIMES = [
    "2011/03/11 14:49:00",
    "2011/03/11 15:14:00",
    "2011/03/11 15:30:00",
    "2011/03/11 16:08:00",
    "2011/03/11 18:47:00",
    "2011/03/11 21:35:00",
    "2011/03/11 22:53:00",
    "2011/03/12 03:20:00",
]
TSUNAMI_CLEAR = "2011/03/13 17:58:00"
# 震度データベースの震度の記号 -> P2P地震情報の震度
SCALE = {"1": 10, "2": 20, "3": 30, "4": 40, "A": 45, "B": 50, "C": 55, "D": 60, "7": 70}
UA = {"User-Agent": "eq-webservice scenario builder"}
Z2H = str.maketrans("0123456789.m", "０１２３４５６７８９．ｍ")


def fetch(url: str, data: bytes | None = None, headers: dict | None = None) -> bytes:
    req = urllib.request.Request(url, data=data, headers={**UA, **(headers or {})})
    with urllib.request.urlopen(req, timeout=120) as res:
        return res.read()


def stations_db() -> list[dict]:
    """震度データベースの観測点ごとの震度 [{name, lat, lon, char}]"""
    boundary = "eqwebservice"
    body = "".join(f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n' for k, v in {"mode": "event", "id": EVENT}.items())
    raw = fetch(EQDB, (body + f"--{boundary}--\r\n").encode(), {"Content-Type": f"multipart/form-data; boundary={boundary}"})
    return json.loads(raw)["res"]["int"]


def points(current: dict[str, list]) -> list[dict]:
    """今の観測点一覧にある名前はそのまま、無いもの (廃止・移設前など) は位置と最寄りの観測点の細分区域を付ける"""
    out = []
    for s in stations_db():
        name = s["name"].replace("＊", "")
        lat, lon = float(s["lat"]), float(s["lon"])
        known = current.get(name)
        # ponytail: 最寄りの観測点の細分区域を借りる (区域の境界付近では隣の区域になることがある)
        area = known[3] if known else min(current.values(), key=lambda c: math.hypot(c[1] - lat, (c[2] - lon) * math.cos(math.radians(lat))))[3]
        p = {"pref": area_pref(area), "addr": name, "isArea": False, "scale": SCALE[s["char"]]}
        if not known:
            p["station"] = {"lat": lat, "lon": lon, "area": area}
        out.append(p)
    return sorted(out, key=lambda p: -p["scale"])


def quake(issued: str, mag: float, pts: list[dict]) -> dict:
    return {
        "code": 551,
        "id": f"tohoku2011-{issued[-8:].replace(':', '')}-{issued[8:10]}",
        "issue": {"source": "気象庁", "time": issued, "type": "DetailScale" if pts else "Destination", "correct": "None"},
        "earthquake": {
            "time": "2011/03/11 14:46:00",
            "maxScale": max((p["scale"] for p in pts), default=-1),
            "domesticTsunami": "Warning",
            "foreignTsunami": "Unknown",
            "hypocenter": {"name": NAME, "latitude": LAT, "longitude": LON, "depth": DEPTH, "magnitude": mag},
        },
        "points": pts,
    }


def grade(h: str) -> str:
    v = float(h.removesuffix("以上").removesuffix("m"))
    return "Watch" if v < 1 else "Warning" if v < 3 else "MajorWarning"


def tsunami_table() -> dict[str, list[str | None]]:
    """津波予報区 -> 8 回の発表それぞれの予想の高さ (まだ発表していない回は None)。表は 12日03:20 の列で右に揃っている"""
    from pypdf import PdfReader

    text = "\n".join(p.extract_text() or "" for p in PdfReader(io.BytesIO(fetch(SAIGAIJI))).pages[:4])
    body = text[text.index("津波予報区\n") : text.index("13日\n17時58分")]
    table = {}
    for line in body.splitlines()[1:]:
        name, *cells = line.split()
        cells = [c for c in cells if c != "解除"]
        filled: list[str | None] = []
        for c in cells:
            filled.append(filled[-1] if c == "→" else c)
        table[name] = [None] * (len(TSUNAMI_TIMES) - len(filled)) + filled
    assert len(table) == 66 and all(len(v) == len(TSUNAMI_TIMES) for v in table.values()), "津波の表を読めません"
    return table


def tsunamis() -> list[dict]:
    table = tsunami_table()
    out = []
    for i, t in enumerate(TSUNAMI_TIMES):
        areas = [
            {"grade": grade(hs[i]), "immediate": False, "name": name, "maxHeight": {"description": hs[i].translate(Z2H), "value": float(hs[i].removesuffix("以上").removesuffix("m"))}}
            for name, hs in table.items()
            if hs[i]
        ]
        out.append({"code": 552, "id": f"tohoku2011-t{i + 1}", "cancelled": False, "issue": {"source": "気象庁", "time": t, "type": "Focus"}, "areas": areas})
    out.append({"code": 552, "id": "tohoku2011-t-clear", "cancelled": True, "issue": {"source": "気象庁", "time": TSUNAMI_CLEAR, "type": "Focus"}, "areas": []})
    return out


def issued(o: dict) -> str:
    return o.get("AnnouncedTime") or o["issue"]["time"]


def main(stations_path: str, dst: str) -> None:
    current = {s[0]: s for s in json.load(open(stations_path, encoding="utf-8"))}
    pts = points(current)
    eews = to_wolfx(EEW_ID, "2011/03/11", ORIGIN, NAME)
    quakes = [quake(t, m, pts if detail else []) for t, m, detail in REPORTS]
    # 同じ時刻の発表は 地震 -> 津波 の順
    events = sorted(eews + quakes + tsunamis(), key=lambda o: (issued(o), o.get("code", 0)))
    header = [
        "# name: 記録: 東北地方太平洋沖地震 (2011年3月11日) 本震",
        "# description: M9.0、最大震度7。緊急地震速報 (警報)、大津波警報の拡大と解除、M の更新 (7.9→9.0)。"
        "各地の震度は震度データベースの確定値で、当時の発表の時刻・順ではない。12日午後からの津波警報の切り下げは省略",
        "# source: 気象庁 (緊急地震速報の発表状況、震度データベース、災害時地震・津波速報)",
    ]
    with open(dst, "w", encoding="utf-8") as f:
        f.write("\n".join(header) + "\n")
        f.writelines(json.dumps(o, ensure_ascii=False, separators=(",", ":")) + "\n" for o in events)
    moved = sum(1 for p in pts if "station" in p)
    print(f"{len(eews)} EEW, {len(quakes)} quake ({len(pts)} points, {moved} with position), {len(events) - len(eews) - len(quakes)} tsunami -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
