#!/usr/bin/env python3
"""気象警報・注意報の区域 (市町村等) の境界を、ブラウザ表示向けに軽くした GeoJSON にする。

入力: 気象庁「予報区等GISデータ」の「市町村等（気象警報等）」(シェープファイル, 属性は UTF-8)
  https://www.data.jma.go.jp/developer/gis.html (20260226_AreaInformationCity_weather_GIS.zip)
  出典: 気象庁ホームページのデータを加工して作成
出力: web/public/warning-areas.geojson  properties: code (市町村等のコード 7 桁), name
  小さな島だけの区域も消えないよう、どの輪も残らないときは一番大きい輪を残す

使い方 (zip 内のファイル名は日本語なので、取り出して city.* に改名しておく):
  curl -LO https://www.data.jma.go.jp/developer/gis/20260226_AreaInformationCity_weather_GIS.zip
  python3 -c "import zipfile; z = zipfile.ZipFile('20260226_AreaInformationCity_weather_GIS.zip'); \\
    [open('city.' + i.filename.rsplit('.', 1)[-1], 'wb').write(z.read(i)) for i in z.infolist() if '.' in i.filename]"
  cd tools && uv run --with pyshp python jma_warning_areas.py ../city ../web/public/warning-areas.geojson
"""

import json
import sys

import shapefile

from simplify_geojson import DIGITS, TOLERANCE, area, douglas_peucker, simplify_ring
from tsunami_areas import thin


def rings_of(shape):
    bounds = list(shape.parts) + [len(shape.points)]
    return [shape.points[s:e] for s, e in zip(bounds, bounds[1:])]


def main(src: str, dst: str) -> None:
    r = shapefile.Reader(src, encoding="utf-8")
    features = []
    for shape, rec in zip(r.shapes(), r.records()):
        rings = [thin(ring, TOLERANCE / 4) for ring in rings_of(shape) if len(ring) > 3]
        shown = [s for s in (simplify_ring(ring) for ring in rings) if s]
        if not shown and rings:
            biggest = max(rings, key=area)
            shown = [[[round(x, DIGITS), round(y, DIGITS)] for x, y in douglas_peucker(biggest, TOLERANCE / 4)]]
        if shown:
            features.append(
                {
                    "type": "Feature",
                    "properties": {"code": rec["regioncode"], "name": rec["regionname"]},
                    "geometry": {"type": "MultiPolygon", "coordinates": [[ring] for ring in shown]},
                }
            )
    with open(dst, "w", encoding="utf-8") as f:
        json.dump({"type": "FeatureCollection", "features": features}, f, ensure_ascii=False, separators=(",", ":"))
    print(f"{len(features)} of {len(r)} areas -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
