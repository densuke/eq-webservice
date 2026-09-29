#!/usr/bin/env python3
"""気象庁「緊急地震速報（警報）発表状況」の「緊急地震速報の内容」ページを読み、各報を Wolfx 形式の JSON にする。

ページ: https://www.data.jma.go.jp/eew/data/nc/pub_hist/<年>/<月>/<ID>/content/content_out.html
出典: 気象庁ホームページのデータを加工して作成

各報の提供時刻・震源 (緯度・経度・深さ・M)・地域ごとの予測震度を取り出す。発生時刻は各報に無いので呼び出し側が与える。
警報は、ページに「背景が灰色 [予報の第 N 報]」と書かれた報で発表される。発表後の続報も警報が続いているものとして isWarn を立てる。
"""

import html
import re
import urllib.request

BASE = "https://www.data.jma.go.jp/eew/data/nc/pub_hist"
Z2H = str.maketrans("０１２３４５６７８９", "0123456789")
SHINDO = {"1": "1", "2": "2", "3": "3", "4": "4", "5弱": "5-", "5強": "5+", "6弱": "6-", "6強": "6+", "7": "7"}
ORDER = ["1", "2", "3", "4", "5-", "5+", "6-", "6+", "7"]


def fetch_text(event_id: str) -> str:
    """ページの文字だけを 1 行に (年によって表の 1 マスごとに改行があるので、空白と改行を 1 つの空白にそろえる)"""
    url = f"{BASE}/{event_id[:4]}/{event_id[4:6]}/{event_id}/content/content_out.html"
    with urllib.request.urlopen(url, timeout=30) as res:
        s = res.read().decode("utf-8", "replace")
    s = re.sub(r"<(script|style)[\s\S]*?</\1>", "", s)
    s = html.unescape(re.sub(r"<[^>]+>", " ", s))
    return re.sub(r"\s+", " ", s).translate(Z2H)


def parse_notes(text: str) -> dict[str, list[dict]]:
    """※番号 -> [{Chiiki, Shindo1 (上限), Shindo2 (下限)}]。長周期地震動は使わない"""
    start = re.search(r"※1 (震度|長周期)", text)
    if not start:
        return {}
    notes: dict[str, list[dict]] = {}
    for n, body in re.findall(r"※(\d+) (.*?)(?= ※\d+ |$)", text[start.start():]):
        notes[n] = [
            {"Chiiki": a, "Shindo1": SHINDO[hi or lo], "Shindo2": SHINDO[lo]}
            for lo, hi, _, areas in re.findall(r"(?<!最大)震度(\d[弱強]?)(?:から(\d[弱強]?))?程度(以上)? (\S+)", body)
            for a in areas.split("、")
        ]
    return notes


def parse_reports(text: str) -> list[dict]:
    """[{serial, time (HH:MM:SS), lat, lon, depth, mag, note (※番号 or None), top (「最大震度X程度以上」の X or None)}]"""
    rows = re.findall(
        r"(\d+) (\d+)時(\d+)分([\d.]+)秒 [\d.]+ ([\d.]+) ([\d.]+) (\d+)km ([\d.]+|不明) (※(\d+)|予測震度なし|最大震度(\d[弱強]?)程度以上)",
        text,
    )
    return [
        {
            "serial": int(n),
            "time": f"{int(h):02d}:{int(mi):02d}:{int(float(sec)):02d}",
            "lat": float(lat),
            "lon": float(lon),
            "depth": int(dep),
            "mag": None if mag == "不明" else float(mag),
            "note": note or None,
            "top": SHINDO[top] if top else None,
        }
        for n, h, mi, sec, lat, lon, dep, mag, _, note, top in rows
    ]


def warning_serials(text: str) -> list[int]:
    m = re.search(r"背景が灰色\[(.*?)\]", text.replace(" ", ""))
    return [int(x) for x in re.findall(r"第(\d+)報", m.group(1))] if m else []


def to_wolfx(event_id: str, date: str, origin: str, hypocenter: str) -> list[dict]:
    """date は "YYYY/MM/DD"、origin は発生時刻 "YYYY/MM/DD HH:MM:SS"、hypocenter は震央地名"""
    text = fetch_text(event_id)
    notes = parse_notes(text)
    reports = parse_reports(text)
    first_warn = min(warning_serials(text) or [10**9])
    out = []
    for i, r in enumerate(reports):
        areas = notes.get(r["note"], []) if r["note"] else []
        top = max((a["Shindo1"] for a in areas), key=ORDER.index, default=r["top"] or "不明")
        warn = r["serial"] >= first_warn
        out.append(
            {
                "type": "jma_eew",
                "Title": "緊急地震速報（警報）" if warn else "緊急地震速報（予報）",
                "EventID": event_id,
                "Serial": r["serial"],
                "AnnouncedTime": f"{date} {r['time']}",
                "OriginTime": origin,
                "Hypocenter": hypocenter,
                "Latitude": r["lat"],
                "Longitude": r["lon"],
                "Magnitude": r["mag"],
                "Depth": r["depth"],
                "MaxIntensity": top,
                "WarnArea": [{**a, "Time": "", "Type": "警報" if warn else "予報", "Arrive": ""} for a in areas],
                "isSea": False,
                "isTraining": False,
                "isAssumption": False,
                "isWarn": warn,
                "isFinal": i == len(reports) - 1,
                "isCancel": False,
            }
        )
    return out


def issued(o: dict) -> str:
    """記録の発表時刻 (Wolfx の緊急地震速報と P2P地震情報のどちらも)"""
    return o.get("AnnouncedTime") or o["issue"]["time"]


if __name__ == "__main__":
    # 読み取りの確認: python3 jma_eew.py 20240101161010
    import sys

    t = fetch_text(sys.argv[1])
    rs = parse_reports(t)
    ns = parse_notes(t)
    assert rs and rs[0]["serial"] == 1, "報の表を読めません"
    assert all(r["note"] is None or r["note"] in ns for r in rs), "※の注記を読めません"
    print(f"{len(rs)} reports, {len(ns)} notes, warnings at {warning_serials(t)}")
