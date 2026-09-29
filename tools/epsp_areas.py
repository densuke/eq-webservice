#!/usr/bin/env python3
"""P2P地震情報の地域コード (地震感知情報で使う) の名前と位置を、画面で使う JSON にする。

入力: epsp-area.csv (https://github.com/p2pquake/epsp-specifications, MIT License)
出力: web/public/userquake-areas.json  {"地域コード": ["地域名", 緯度, 経度], ...} (位置の無い地域は除く)

使い方:
  curl -LO https://raw.githubusercontent.com/p2pquake/epsp-specifications/master/epsp-area.csv
  cd tools && python3 epsp_areas.py ../epsp-area.csv ../web/public/userquake-areas.json
"""

import csv
import json
import sys


def main(src: str, dst: str) -> None:
    out = {}
    with open(src, encoding="utf-8") as f:
        for row in csv.DictReader(f):
            lat, lon = row["緯度"], row["経度"]
            if lat and lon:
                out[row["地域コード(数値型)"]] = [row["地域"], float(lat), float(lon)]
    with open(dst, "w", encoding="utf-8") as f:
        json.dump(out, f, ensure_ascii=False, separators=(",", ":"))
    print(f"{len(out)} areas -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
