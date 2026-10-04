#!/usr/bin/env python3
"""音声アナウンス (TTS) で事前合成する地名リストを作る。

入力:
  - 気象庁「防災情報XML フォーマット コード表」(地震火山関連コード表.xlsx) の
    シート 41 AreaEpicenter (震央地名)
    https://xml.kishou.go.jp/tec_material.html  (jmaxml_YYYYMMDD_Code.zip, 更新のたびに日付が変わる)
    出典: 気象庁ホームページ (https://xml.kishou.go.jp/) のデータを加工して作成
  - web/public/tsunami.geojson (tools/tsunami_areas.py の出力) の津波予報区名
出力:
  - crates/eq-server/src/tts/epicenters.txt     「震源は{name}。」用 (JMA の並び順、重複なし)
  - crates/eq-server/src/tts/tsunami_areas.txt  「{area}。」用 (geojson の並び順、重複なし)
  どちらも 1 行 1 件の UTF-8。

使い方 (標準ライブラリのみ):
  curl -LO https://xml.kishou.go.jp/jmaxml_20260917_Code.zip
  python3 tools/tts_places.py jmaxml_20260917_Code.zip
"""

import json
import sys
import xml.etree.ElementTree as ET
import zipfile

NS = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
REL = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"
OUT = "crates/eq-server/src/tts"


def shared_strings(z):
    # 読み仮名 (rPh) は除き、本文 (si/t と si/r/t) だけ集める
    out = []
    for si in ET.fromstring(z.read("xl/sharedStrings.xml")).iter(NS + "si"):
        out.append("".join(t.text or "" for t in list(si.findall(NS + "t")) + [t for r in si.findall(NS + "r") for t in r.findall(NS + "t")]))
    return out


def sheet_rows(z, name):
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = {r.get("Id"): r.get("Target") for r in ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))}
    sid = next(s.get(REL) for s in wb.iter(NS + "sheet") if s.get("name") == name)
    path = rels[sid].lstrip("/")
    path = path if path.startswith("xl/") else "xl/" + path
    ss = shared_strings(z)
    for row in ET.fromstring(z.read(path)).iter(NS + "row"):
        cells = {}
        for c in row.iter(NS + "c"):
            v = c.find(NS + "v")
            if v is not None:
                col = "".join(ch for ch in c.get("r") if ch.isalpha())
                cells[col] = ss[int(v.text)] if c.get("t") == "s" else v.text
        yield cells


def epicenters(zip_path):
    with zipfile.ZipFile(zip_path) as outer:
        book = next(n for n in outer.namelist() if "地震火山関連コード表" in n)
        with zipfile.ZipFile(__import__("io").BytesIO(outer.read(book))) as z:
            # A=Code (3 桁数字), B=Name
            return [r["B"].strip() for r in sheet_rows(z, "41") if r.get("A", "").isdigit() and r.get("B", "").strip()]


def tsunami_areas(path):
    with open(path, encoding="utf-8") as f:
        return [ft["properties"]["name"] for ft in json.load(f)["features"]]


def write(path, names):
    uniq = list(dict.fromkeys(names))
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(uniq) + "\n")
    print(f"{path}: {len(uniq)}")


if __name__ == "__main__":
    write(f"{OUT}/epicenters.txt", epicenters(sys.argv[1]))
    write(f"{OUT}/tsunami_areas.txt", tsunami_areas("web/public/tsunami.geojson"))
