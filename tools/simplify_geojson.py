#!/usr/bin/env python3
"""都道府県境界 GeoJSON をブラウザ表示向けに軽量化する。

入力: dataofjapan/land の japan.geojson (地球地図日本 由来)
  https://github.com/dataofjapan/land
出力: web/public/japan.geojson (都道府県名 name のみ、座標は小数 3 桁)

使い方:
  curl -LO https://raw.githubusercontent.com/dataofjapan/land/master/japan.geojson
  python3 tools/simplify_geojson.py japan.geojson web/public/japan.geojson
"""

import json
import sys

TOLERANCE = 0.008  # 度 (約 800m)。大きくするほど軽くなる
MIN_AREA = 0.0004  # 度^2。これより小さい島は捨てる (約 2km 四方)
DIGITS = 3
# 南西諸島 (北緯 31.5 度より南かつ東経 132 度より西) の輪だけ、別枠で点として見せたいので基準を緩める
NANSEI_MIN_AREA = 0.00005  # 度^2 (約 0.6km²)。鳩間島・小宝島が元データで約 0.83km² なので 0.9km² 相当より少し下げた
NANSEI_LAT, NANSEI_LON = 31.5, 132.0


def perp_dist(p, a, b):
    (x, y), (x1, y1), (x2, y2) = p, a, b
    dx, dy = x2 - x1, y2 - y1
    if dx == 0 and dy == 0:
        return ((x - x1) ** 2 + (y - y1) ** 2) ** 0.5
    t = ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy)
    t = max(0.0, min(1.0, t))
    return ((x - x1 - t * dx) ** 2 + (y - y1 - t * dy) ** 2) ** 0.5


def douglas_peucker(pts, tol):
    # 再帰だと長い海岸線でスタックが深くなるので明示スタックで
    keep = [False] * len(pts)
    keep[0] = keep[-1] = True
    stack = [(0, len(pts) - 1)]
    while stack:
        s, e = stack.pop()
        best, idx = 0.0, -1
        for i in range(s + 1, e):
            d = perp_dist(pts[i], pts[s], pts[e])
            if d > best:
                best, idx = d, i
        if best > tol:
            keep[idx] = True
            stack.append((s, idx))
            stack.append((idx, e))
    return [p for p, k in zip(pts, keep) if k]


def area(ring):
    return abs(sum(x1 * y2 - x2 * y1 for (x1, y1), (x2, y2) in zip(ring, ring[1:]))) / 2


def in_nansei(ring):
    xs = [x for x, _ in ring]
    ys = [y for _, y in ring]
    return (min(ys) + max(ys)) / 2 < NANSEI_LAT and (min(xs) + max(xs)) / 2 < NANSEI_LON


def diamond(ring):
    """DP でも 4 点に満たない小さな輪を、外接の菱形 (最低限の形) にする"""
    xs = [x for x, _ in ring]
    ys = [y for _, y in ring]
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    pts = [(x0, cy), (cx, y0), (x1, cy), (cx, y1), (x0, cy)]
    return [[round(x, DIGITS), round(y, DIGITS)] for x, y in pts]


def dedup_ring(pts, tol):
    out = [[round(x, DIGITS), round(y, DIGITS)] for x, y in douglas_peucker(pts, tol)]
    dedup = [out[0]]
    for p in out[1:]:
        if p != dedup[-1]:
            dedup.append(p)
    if dedup[0] != dedup[-1]:
        dedup.append(dedup[0])
    return dedup


def simplify_ring(ring):
    nansei = in_nansei(ring)
    if area(ring) < (NANSEI_MIN_AREA if nansei else MIN_AREA):
        return None
    out = dedup_ring(ring, TOLERANCE)
    if len(out) >= 4:
        return out
    if not nansei:
        return None
    out = dedup_ring(ring, TOLERANCE / 4)
    return out if len(out) >= 4 else diamond(ring)


def simplify_polygon(poly):
    outer = simplify_ring(poly[0])
    if outer is None:
        return None
    holes = [h for h in (simplify_ring(r) for r in poly[1:]) if h]
    return [outer] + holes


def main(src, dst):
    with open(src, encoding="utf-8") as f:
        data = json.load(f)
    features = []
    for feat in data["features"]:
        geom = feat["geometry"]
        polys = geom["coordinates"] if geom["type"] == "MultiPolygon" else [geom["coordinates"]]
        out = [p for p in (simplify_polygon(p) for p in polys) if p]
        features.append(
            {
                "type": "Feature",
                "properties": {"name": feat["properties"]["nam_ja"]},
                "geometry": {"type": "MultiPolygon", "coordinates": out},
            }
        )
    features.sort(key=lambda f: f["properties"]["name"])
    result = {
        "type": "FeatureCollection",
        "attribution": "地球地図日本 (国土地理院) / dataofjapan/land を加工",
        "features": features,
    }
    with open(dst, "w", encoding="utf-8") as f:
        json.dump(result, f, ensure_ascii=False, separators=(",", ":"))


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
