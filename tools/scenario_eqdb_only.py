#!/usr/bin/env python3
"""緊急地震速報や今の形の地震情報が無かった時代の地震を、震度データベースの記録だけでデモの場面にする。

  - kobe1995: 兵庫県南部地震 (1995-01-17)。震度7 は観測点ではなく後日の現地調査で決まった地域なので、
    気象庁が震度7 とした市町を「(現地調査)」の点として足す (位置は市町の中心付近の概算)
  - kanto1923: 大正関東地震 (1923-09-01)
震度は当時の階級 (5・6 に弱・強の区別なし)。震度が「不明」の観測点は使わない。
当時の発表の時刻は再現しない: 揺れの広がりを見せるため、発生の 5 秒後に震源、1 分後に各地の震度を出す。
出典: 気象庁ホームページのデータを加工して作成

使い方: cd tools && python3 scenario_eqdb_only.py ../web/public/stations.json ../samples/scenarios
"""

import json
import sys
from datetime import datetime, timedelta

from eqdb import SCALE, event, point, points

FMT = "%Y/%m/%d %H:%M:%S"
SCENES = {
    "kobe1995": {
        "event": "19950117054651",
        "name": "記録: 兵庫県南部地震 (1995年1月17日)",
        "description": "阪神・淡路大震災。M7.3。震度は当時の階級 (5・6 に弱・強の区別なし)。"
        "震度7 は観測ではなく後日の現地調査で決まった地域で、市町ごとの点 (位置は概算) で示す。緊急地震速報は無く、発表の時刻は再現していない",
        # 気象庁が現地調査で震度7 とした市町 (中心付近の概算の位置)
        "extra": [
            ("神戸市 (現地調査)", 34.690, 135.196),
            ("芦屋市 (現地調査)", 34.727, 135.304),
            ("西宮市 (現地調査)", 34.738, 135.342),
            ("宝塚市 (現地調査)", 34.800, 135.360),
            ("北淡町 (現地調査)", 34.560, 134.930),
            ("一宮町 (現地調査)", 34.470, 134.850),
            ("津名町 (現地調査)", 34.430, 134.910),
        ],
    },
    "kanto1923": {
        "event": "19230901115831",
        "name": "記録: 関東大震災 (1923年9月1日)",
        "description": "大正関東地震。M7.9。震度は当時の階級 (5・6 に弱・強の区別なし、最大は震度6)。"
        "震度が不明の観測点は除く。緊急地震速報や津波の予報は無く、発表の時刻は再現していない",
        "extra": [],
    },
}
SOURCE = "気象庁 (震度データベース)"


def p2p_quake(qid: str, issued: datetime, kind: str, hyp: dict, pts: list[dict], comment: str = "") -> dict:
    q = {
        "code": 551,
        "id": qid,
        "issue": {"source": "気象庁", "time": issued.strftime(FMT), "type": kind, "correct": "None"},
        "earthquake": {
            # 発生時刻は秒まで (P2P地震情報は分までだが、波の描画を発生に合わせるため)
            "time": hyp["ot"][:19],
            "maxScale": max((p["scale"] for p in pts), default=-1),
            "domesticTsunami": "Unknown",
            "foreignTsunami": "Unknown",
            "hypocenter": {
                "name": hyp["name"],
                "latitude": float(hyp["lat"]),
                "longitude": float(hyp["lon"]),
                "depth": int(hyp["dep"].split()[0]),
                "magnitude": float(hyp["mag"]),
            },
        },
        "points": pts,
    }
    # 画面の詳細に出る欄 (空なら付けず、既存の場面のデータは変わらない)
    return {**q, "comments": {"freeFormComment": comment}} if comment else q


def build(sid: str, scene: dict, current: dict[str, list]) -> list[dict]:
    hyp = event(scene["event"])["hyp"][0]
    origin = datetime.strptime(hyp["ot"][:19], FMT)
    extra = [point(current, name, lat, lon, SCALE["7"]) for name, lat, lon in scene["extra"]]
    pts = extra + points(scene["event"], current)
    return [
        p2p_quake(f"{sid}-hypo", origin + timedelta(seconds=5), "Destination", hyp, []),
        p2p_quake(f"{sid}-detail", origin + timedelta(minutes=1), "DetailScale", hyp, pts),
    ]


def main(stations_path: str, out_dir: str) -> None:
    current = {s[0]: s for s in json.load(open(stations_path, encoding="utf-8"))}
    for sid, scene in SCENES.items():
        events = build(sid, scene, current)
        dst = f"{out_dir}/{sid}.jsonl"
        with open(dst, "w", encoding="utf-8") as f:
            f.write(f"# name: {scene['name']}\n# description: {scene['description']}\n# source: {SOURCE}\n")
            f.writelines(json.dumps(o, ensure_ascii=False, separators=(",", ":")) + "\n" for o in events)
        print(f"{sid}: {len(events[-1]['points'])} points -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
