#!/usr/bin/env python3
"""津波予報区の沿岸線をブラウザ表示向けに軽量化して GeoJSON にする。

入力: 気象庁「予報区等GISデータ」の津波予報区 (シェープファイル, 属性は UTF-8)
  https://www.data.jma.go.jp/developer/gis.html
  出典: 気象庁ホームページ (https://www.data.jma.go.jp/developer/gis.html) のデータを加工して作成
出力: web/public/tsunami.geojson (予報区名 name のみの MultiLineString、座標は小数 3 桁)

使い方 (zip 内のファイル名は Shift_JIS なので、取り出して area.* に改名しておく):
  curl -LO https://www.data.jma.go.jp/developer/gis/20240520_AreaTsunami_GIS.zip
  python3 -c "import zipfile; z = zipfile.ZipFile('20240520_AreaTsunami_GIS.zip'); \\
    [open('area.' + i.filename.rsplit('.', 1)[-1], 'wb').write(z.read(i)) for i in z.infolist()]"
  uv run --with pyshp python tools/tsunami_areas.py area web/public/tsunami.geojson
"""

import json
import sys

import shapefile

from simplify_geojson import DIGITS, TOLERANCE, douglas_peucker

MIN_LENGTH = 0.03  # 度。これより短い線 (小さな島の海岸など) は捨てる


def thin(pts, tol):
    """近すぎる点を先に間引く (878 万点を Douglas-Peucker に直接かけると遅いため)"""
    out = [pts[0]]
    for p in pts[1:-1]:
        q = out[-1]
        if abs(p[0] - q[0]) + abs(p[1] - q[1]) >= tol:
            out.append(p)
    out.append(pts[-1])
    return out


def length(pts):
    return sum(((x2 - x1) ** 2 + (y2 - y1) ** 2) ** 0.5 for (x1, y1), (x2, y2) in zip(pts, pts[1:]))


def simplify_line(pts):
    if len(pts) < 2 or length(pts) < MIN_LENGTH:
        return None
    out = douglas_peucker(thin(pts, TOLERANCE / 4), TOLERANCE)
    return [[round(x, DIGITS), round(y, DIGITS)] for x, y in out]


def main(src, dst):
    r = shapefile.Reader(src, encoding="utf-8")
    features = []
    for shape, rec in zip(r.shapes(), r.records()):
        name = rec["name"]
        if name.startswith("帰属未定"):
            continue
        bounds = list(shape.parts) + [len(shape.points)]
        lines = [simplify_line(shape.points[s:e]) for s, e in zip(bounds, bounds[1:])]
        lines = [ln for ln in lines if ln]
        if not lines:
            continue
        features.append(
            {
                "type": "Feature",
                "properties": {"name": name},
                "geometry": {"type": "MultiLineString", "coordinates": lines},
            }
        )
    with open(dst, "w", encoding="utf-8") as f:
        json.dump({"type": "FeatureCollection", "features": features}, f, ensure_ascii=False, separators=(",", ":"))
    print(f"{len(features)} areas -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
