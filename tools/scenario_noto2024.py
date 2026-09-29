#!/usr/bin/env python3
"""デモの場面「令和6年能登半島地震 (2024-01-01)」を当時の発表から作る。

  - 緊急地震速報: 気象庁「緊急地震速報（警報）発表状況」の各報 (jma_eew.py)
  - 震度・震源・各地の震度・津波予報: P2P地震情報 API v2 の履歴 (気象庁発表の電文)
出力: samples/scenarios/noto2024.jsonl (その後 `eq-server convert samples/scenarios web/public/demo`)

使い方: cd tools && python3 scenario_noto2024.py ../samples/scenarios/noto2024.jsonl
"""

import json
import sys
import urllib.request

from jma_eew import issued, to_wolfx

P2P = "https://api.p2pquake.net/v2/jma"
DATE = "2024/01/01"
# 16:06 の前震 (M5.5)、16:10 の本震 (M7.6)、16:18 の地震 (M6.1)。
# 本震の発生時刻は、緊急地震速報のきっかけになった最初の破壊 (16:10:09.5, M5.9) にする。
# 気象庁の発表の M7.6 の発生時刻 16:10:22.5 は第 1 報 (16:10:16) より後になり、波の描画と合わないため
EEW = [
    ("20240101160608", f"{DATE} 16:06:06"),
    ("20240101161010", f"{DATE} 16:10:09"),
    ("20240101161845", f"{DATE} 16:18:42"),
]
QUAKE_TIMES = {f"{DATE} 16:06:00", f"{DATE} 16:10:00", f"{DATE} 16:18:00"}
# P2P の履歴の付帯情報 (表示に使わないもの) は落とす
DROP = {"created_at", "time", "timestamp", "user_agent", "ver"}


def get(path: str) -> list[dict]:
    req = urllib.request.Request(f"{P2P}/{path}", headers={"User-Agent": "eq-webservice scenario builder"})
    with urllib.request.urlopen(req, timeout=30) as res:
        return json.load(res)


def main(dst: str) -> None:
    eews = [r for eid, origin in EEW for r in to_wolfx(eid, DATE, origin, "石川県能登地方")]
    quakes = [o for o in get("quake?since_date=20240101&until_date=20240101&limit=100&order=1") if o["earthquake"]["time"] in QUAKE_TIMES]
    tsunamis = get("tsunami?since_date=20240101&until_date=20240102&limit=100&order=1")
    p2p = [{k: v for k, v in o.items() if k not in DROP} for o in quakes + tsunamis]
    events = sorted(eews + p2p, key=issued)
    header = [
        "# name: 記録: 令和6年能登半島地震 (2024年1月1日)",
        "# description: 16:06 の前震 (最大震度5強) から 16:10 の本震 (M7.6, 最大震度7)、大津波警報とその解除まで。当時の発表の再生",
        "# source: 気象庁 (緊急地震速報の発表状況)、P2P地震情報 (気象庁発表の地震・津波情報)",
    ]
    with open(dst, "w", encoding="utf-8") as f:
        f.write("\n".join(header) + "\n")
        f.writelines(json.dumps(o, ensure_ascii=False, separators=(",", ":")) + "\n" for o in events)
    print(f"{len(eews)} EEW, {len(quakes)} quake, {len(tsunamis)} tsunami -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1])
