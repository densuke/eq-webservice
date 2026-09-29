#!/usr/bin/env python3
"""気象庁の天気予報のページの TELOPS (天気コード -> アイコンの表) を抜き出し、
crates/eq-server/src/broadcast/native/telops.rs を作る。標準ライブラリだけで動く。

使い方 (リポジトリの直下で):
    python3 tools/jma_telops.py
    python3 tools/jma_telops.py --out /tmp/telops.rs     # 出力先を変える

出典: 気象庁ホームページ https://www.jma.go.jp/bosai/forecast/ (公共データ利用規約 第1.0版)
"""
import argparse
import re
import sys
import urllib.request

URL = "https://www.jma.go.jp/bosai/forecast/"
OUT = "crates/eq-server/src/broadcast/native/telops.rs"
# 例: 100:["100.svg","500.svg","100","晴","CLEAR"]
ENTRY = re.compile(r'(\d{3}):\["(\d+\.svg)","(\d+\.svg)"')


def fetch():
    req = urllib.request.Request(URL, headers={"User-Agent": "eq-webservice tools/jma_telops.py"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read().decode("utf-8")


def parse(html):
    start = html.find("TELOPS=")
    if start < 0:
        sys.exit("TELOPS が見つかりません (ページの作りが変わった?)")
    end = html.find("};", start)
    return sorted({int(c): (d, n) for c, d, n in ENTRY.findall(html[start:end])}.items())


def render(entries):
    lines = [
        "//! 天気コード -> (昼のアイコン, 夜のアイコン)。tools/jma_telops.py が気象庁の天気予報のページの TELOPS から作る。",
        "//! 作り直す: リポジトリの直下で python3 tools/jma_telops.py (手で直さない)",
        "//! 出典: 気象庁ホームページ https://www.jma.go.jp/bosai/forecast/ を加工",
        "",
        "/// コードの昇順 (二分探索で引く)",
        "pub const TELOPS: &[(u16, &str, &str)] = &[",
    ]
    lines += [f'    ({c}, "{d}", "{n}"),' for c, (d, n) in entries]
    lines.append("];")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=OUT)
    args = ap.parse_args()
    entries = parse(fetch())
    if len(entries) < 100:
        sys.exit(f"表が小さすぎます ({len(entries)} 件)")
    with open(args.out, "w", encoding="utf-8") as f:
        f.write(render(entries))
    print(f"{len(entries)} 件を {args.out} に書きました")


if __name__ == "__main__":
    main()
