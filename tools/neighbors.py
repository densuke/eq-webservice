#!/usr/bin/env python3
"""日本の周辺国の陸地 (朝鮮半島・中国東部・ロシア極東など) を、地図の背景用に切り出して軽量化する。

入力: Natural Earth 1:50m Admin 0 Countries (パブリックドメイン)
  https://github.com/nvkelso/natural-earth-vector (geojson/ne_50m_admin_0_countries.geojson)
出力: web/public/neighbors.geojson (日本以外の陸地。範囲の外は切り落とす。座標は小数 2 桁)

使い方:
  curl -LO https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_admin_0_countries.geojson
  cd tools && python3 neighbors.py ../ne_50m_admin_0_countries.geojson ../web/public/neighbors.geojson
"""

import json
import sys

from simplify_geojson import area, douglas_peucker

# 切り出す範囲 (経度・緯度)。地図を日本全体より少し広く見ても背景が切れない程度
LON_MIN, LON_MAX, LAT_MIN, LAT_MAX = 110.0, 160.0, 18.0, 56.0
TOLERANCE = 0.03  # 度。背景なので粗くてよい
MIN_AREA = 0.02  # 度^2
DIGITS = 2


def clip(ring, inside, cross):
    """Sutherland-Hodgman で 1 辺ずつ切る"""
    out = []
    for i, cur in enumerate(ring):
        prev = ring[i - 1]
        if inside(cur):
            if not inside(prev):
                out.append(cross(prev, cur))
            out.append(cur)
        elif inside(prev):
            out.append(cross(prev, cur))
    return out


def at_x(x):
    return lambda a, b: (x, a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0]))


def at_y(y):
    return lambda a, b: (a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]), y)


def clip_rect(ring):
    ring = [tuple(p) for p in ring[:-1]]
    for inside, cross in (
        (lambda p: p[0] >= LON_MIN, at_x(LON_MIN)),
        (lambda p: p[0] <= LON_MAX, at_x(LON_MAX)),
        (lambda p: p[1] >= LAT_MIN, at_y(LAT_MIN)),
        (lambda p: p[1] <= LAT_MAX, at_y(LAT_MAX)),
    ):
        if not ring:
            return None
        ring = clip(ring, inside, cross)
    if len(ring) < 3:
        return None
    return ring + [ring[0]]


def simplify(ring):
    if area(ring) < MIN_AREA:
        return None
    out = [[round(x, DIGITS), round(y, DIGITS)] for x, y in douglas_peucker(ring, TOLERANCE)]
    return out if len(out) >= 4 else None


def main(src, dst):
    features = []
    for f in json.load(open(src, encoding="utf-8"))["features"]:
        if f["properties"].get("ISO_A3") == "JPN" or f["properties"].get("ADMIN") == "Japan":
            continue
        g = f["geometry"]
        polys = g["coordinates"] if g["type"] == "MultiPolygon" else [g["coordinates"]]
        rings = [r for r in (clip_rect(ring) for poly in polys for ring in poly) if r]
        rings = [r for r in (simplify(r) for r in rings) if r]
        if rings:
            features.append(
                {
                    "type": "Feature",
                    "properties": {"name": f["properties"].get("NAME_JA") or f["properties"]["NAME"]},
                    "geometry": {"type": "MultiPolygon", "coordinates": [[r] for r in rings]},
                }
            )
    with open(dst, "w", encoding="utf-8") as out:
        json.dump({"type": "FeatureCollection", "features": features}, out, ensure_ascii=False, separators=(",", ":"))
    print(f"{len(features)} countries -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
