#!/usr/bin/env python3
"""地震情報細分区域の境界と、震度観測点の位置 (属する細分区域つき) をブラウザ表示向けに作る。

入力:
  - 気象庁「予報区等GISデータ」の「地震情報／細分区域」(シェープファイル, 属性は UTF-8)
    https://www.data.jma.go.jp/developer/gis.html (20240520_AreaForecastLocalE_GIS.zip)
  - 気象庁の震度観測点の一覧 (名前・緯度経度)
    https://www.data.jma.go.jp/eqev/data/intens-st/stations.json
  出典: 気象庁ホームページのデータを加工して作成
出力:
  - web/public/areas.geojson    細分区域名 name のみの MultiPolygon (座標は小数 3 桁)
  - web/public/stations.json    [[観測点名, 緯度, 経度, 細分区域名], ...]

使い方 (zip 内のファイル名は Shift_JIS なので、取り出して area.* に改名しておく):
  curl -LO https://www.data.jma.go.jp/developer/gis/20240520_AreaForecastLocalE_GIS.zip
  python3 -c "import zipfile; z = zipfile.ZipFile('20240520_AreaForecastLocalE_GIS.zip'); \\
    [open('area.' + i.filename.rsplit('.', 1)[-1], 'wb').write(z.read(i)) for i in z.infolist()]"
  curl -Lo stations.json https://www.data.jma.go.jp/eqev/data/intens-st/stations.json
  cd tools && uv run --with pyshp python jma_areas.py ../area ../stations.json ../web/public
"""

import json
import sys

import shapefile

from area_pref import PREFS, area_pref
from simplify_geojson import DIGITS, douglas_peucker, simplify_ring
from tsunami_areas import thin

# 点内判定に使う細かめの形 (表示用より細かくして、海岸近くの観測点を取りこぼさない)
PIP_TOLERANCE = 0.002

def rings_of(shape):
    bounds = list(shape.parts) + [len(shape.points)]
    return [shape.points[s:e] for s, e in zip(bounds, bounds[1:])]


def inside(x, y, rings):
    """偶奇規則 (穴も正しく扱える)"""
    c = False
    for ring in rings:
        for (x1, y1), (x2, y2) in zip(ring, ring[1:]):
            if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
                c = not c
    return c


def bbox(rings):
    xs = [x for r in rings for x, _ in r]
    ys = [y for r in rings for _, y in r]
    return min(xs), min(ys), max(xs), max(ys)


def nearest(x, y, areas):
    """どの区域にも入らない観測点 (海岸・島の簡略化で外れたもの) は、頂点が最も近い区域にする"""
    best, name = float("inf"), None
    for a in areas:
        for r in a["pip"]:
            for px, py in r:
                d = (px - x) ** 2 + (py - y) ** 2
                if d < best:
                    best, name = d, a["name"]
    return name


def main(src, stations_path, out_dir):
    r = shapefile.Reader(src, encoding="utf-8")
    areas, features = [], []
    for shape, rec in zip(r.shapes(), r.records()):
        name = rec["name"]
        rings = rings_of(shape)
        pip = [douglas_peucker(thin(ring, PIP_TOLERANCE / 4), PIP_TOLERANCE) for ring in rings if len(ring) > 3]
        areas.append({"name": name, "pref": area_pref(name), "pip": pip, "bbox": bbox(pip)})
        shown = [ring for ring in (simplify_ring(thin(ring, 0.002)) for ring in rings) if ring]
        if shown:
            # 穴も含めて 1 つの path に入れ、表示側の evenodd で抜く
            features.append(
                {
                    "type": "Feature",
                    "properties": {"name": name},
                    "geometry": {"type": "MultiPolygon", "coordinates": [[ring] for ring in shown]},
                }
            )
    with open(f"{out_dir}/areas.geojson", "w", encoding="utf-8") as f:
        json.dump({"type": "FeatureCollection", "features": features}, f, ensure_ascii=False, separators=(",", ":"))

    stations, outside = [], 0
    for s in json.load(open(stations_path, encoding="utf-8")):
        x, y = float(s["lon"]), float(s["lat"])
        # 県境の観測点を隣の県の区域に入れないよう、同じ都道府県の区域だけを候補にする
        same_pref = [a for a in areas if a["pref"] == PREFS[int(s["pref"]) - 1]] or areas
        hit = next(
            (a["name"] for a in same_pref
             if a["bbox"][0] <= x <= a["bbox"][2] and a["bbox"][1] <= y <= a["bbox"][3] and inside(x, y, a["pip"])),
            None,
        )
        if hit is None:
            outside += 1
            hit = nearest(x, y, same_pref)
        stations.append([s["name"], round(y, DIGITS), round(x, DIGITS), hit])
    with open(f"{out_dir}/stations.json", "w", encoding="utf-8") as f:
        json.dump(stations, f, ensure_ascii=False, separators=(",", ":"))
    print(f"{len(features)} areas, {len(stations)} stations ({outside} by nearest area)", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3])
