#!/usr/bin/env python3
"""デモの場面「松代群発地震 (1965〜)」を、気象庁の震度データベースの記録だけから作る。

長野県北部 (松代) の群発地震は 1965 年 8 月に始まり数年続き、有感地震は数万回に及ぶ。全部は入れられないので、
震度データベースの検索 (震源の名前が「長野県北部」、震度3以上、1965-08〜1971-12。1 年ごとに検索して上限の 1000 件に収まる) で
出した 464 件のうち、次の 12 件を選ぶ。
  - 最大震度の大きいもの (当時の震度5。10 件。うち 1966-04-05 の M5.4 は期間中の最大)
  - 震度4 のうち M の大きいもの (M5.3 の 2 件)
実際の発生は 1965 年 11 月〜1967 年 10 月に散らばっているが、そのまま再生すると 1 件ごとに揺れを描く 3 分ほどがかかり
何十分にもなる。そこで時系列の順は保ったまま、発生を 30 秒おきに詰める (実際の日付の間隔は再現しない。
時計は最初の地震の発生の日時から進むので、2 件目以降の日時は実際のものではない)。
各地の震度は当時の階級 (5・6 に弱・強の区別なし)。震度が「不明」の観測点は使わない。
緊急地震速報は無く、当時の発表の時刻は再現しない: 発生の 5 秒後に震源、25 秒後に各地の震度を出す。
出典: 気象庁ホームページのデータを加工して作成

使い方: cd tools && python3 scenario_matsushiro.py ../web/public/stations.json ../samples/scenarios/matsushiro1965.jsonl
その後 `cargo run -p eq-server -- convert samples/scenarios web/public/demo`
"""

import json
import sys
import time
from datetime import datetime, timedelta

from eqdb import event, points
from scenario_eqdb_only import FMT, SOURCE, p2p_quake

SID = "matsushiro1965"
NAME = "記録: 松代群発地震 (1965〜1967年から 12 件)"
DESCRIPTION = (
    "長野県北部 (松代) の群発地震。1965 年 8 月から数年続き、有感地震は数万回。ここでは震度データベースの 464 件 (震度3以上) から、"
    "最大震度5 の 10 件 (最大は M5.4) と M5.3 の震度4 の 2 件を選び、時系列の順に 30 秒おきに詰めて再生する"
    " (発生は実際には 1965〜1967 年にまたがり、日時は実際のものではない)。"
    "震度は当時の階級 (5・6 に弱・強の区別なし)。緊急地震速報は無く、発表の時刻は再現していない"
)
# 震度データベースの地震の ID (発生時刻)。時系列順
EVENTS = [
    "19660123201557",  # M5.1 震度5
    "19660207040513",  # M4.9 震度5
    "19660405175117",  # M5.4 震度5 (期間中の最大)
    "19660411060615",  # M4.7 震度5
    "19660417102134",  # M4.7 震度5
    "19660417154655",  # M4.7 震度5
    "19660417202818",  # M4.7 震度5
    "19660528142121",  # M5.3 震度5
    "19660803034834",  # M5.3 震度5
    "19660828130921",  # M5.3 震度4
    "19661026030409",  # M5.3 震度4
    "19671014044846",  # M5.3 震度5
]
STEP = timedelta(seconds=30)
HYPO_AFTER = timedelta(seconds=5)
DETAIL_AFTER = timedelta(seconds=25)


def build(current: dict[str, list]) -> list[dict]:
    t0 = None
    out = []
    for i, eid in enumerate(EVENTS):
        if i:
            time.sleep(2)  # 震度データベースに負荷をかけない
        hyp = event(eid)["hyp"][0]
        pts = points(eid, current)
        # 発生の時刻だけを詰めた時刻に差し替える
        t0 = t0 or datetime.strptime(hyp["ot"][:19], FMT)
        origin = t0 + STEP * i
        hyp = {**hyp, "ot": origin.strftime(FMT)}
        out.append(p2p_quake(f"{SID}-{i + 1}-hypo", origin + HYPO_AFTER, "Destination", hyp, []))
        out.append(p2p_quake(f"{SID}-{i + 1}-detail", origin + DETAIL_AFTER, "DetailScale", hyp, pts))
    return out


def main(stations_path: str, dst: str) -> None:
    current = {s[0]: s for s in json.load(open(stations_path, encoding="utf-8"))}
    events = build(current)
    with open(dst, "w", encoding="utf-8") as f:
        f.write(f"# name: {NAME}\n# description: {DESCRIPTION}\n# source: {SOURCE}\n")
        f.writelines(json.dumps(o, ensure_ascii=False, separators=(",", ":")) + "\n" for o in events)
    print(f"{len(EVENTS)} quakes, {sum(len(e['points']) for e in events)} points -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
