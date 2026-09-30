#!/usr/bin/env python3
"""配信 (native) の負荷の試験: 記録の地震を流すサーバ (replay) を立て、native の server をそこへ向けて、
CPU・メモリ・コマの速さ・送信量を 5 秒ごとに測る (docs/broadcast-native.md 付録)。

使い方 (リポジトリのルートで。cargo build --release のあと):
  tools/broadcast_load.py samples/scenarios/noto2024.jsonl --duration 120 \
      --toml 'fps = 10' --toml 'fps_calm = 2' --toml 'label = "配信元: test"'

- 出力は一時ディレクトリのファイル (既定は mpegts、--toml 'encoder = "builtin"' のときは flv) だけ (外には送らない)。終わったら自分で起動したプロセスを止める。
- replay のサーバは別のポート (既定 18099) で立てる。本番や手元の他のサーバとは無関係。
- 測るもの: eq-server (broadcast) と ffmpeg の CPU (1 コアを 100% として)・最大メモリ・コマ数・送信量。
  Mac と Linux (e2) の両方で動く (Linux は /proc、Mac は ps で読む)。数値は環境で違う。
- 地震の画面か平時かは、コマ数の列 (fps_calm を付けたとき) で分かる。
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

WINDOW = 5.0


def cpu_rss(pid):
    """(CPU の秒, 使用メモリ KB)。プロセスが無ければ None"""
    try:
        if os.path.exists(f"/proc/{pid}/stat"):
            f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
            hz = os.sysconf("SC_CLK_TCK")
            rss = int(open(f"/proc/{pid}/statm").read().split()[1]) * os.sysconf("SC_PAGE_SIZE") // 1024
            return (int(f[11]) + int(f[12])) / hz, rss
        out = subprocess.run(["ps", "-o", "cputime=,rss=", "-p", str(pid)], capture_output=True, text=True).stdout.split()
        if len(out) < 2:
            return None
        t = 0.0
        for part in out[0].split(":"):
            t = t * 60 + float(part)
        return t, int(out[1])
    except (OSError, ValueError, IndexError):
        return None


def child_pid(pid, name):
    r = subprocess.run(["pgrep", "-P", str(pid), name], capture_output=True, text=True).stdout.split()
    return int(r[0]) if r else None


def video_packets(path):
    """[(pts 秒, バイト数)]"""
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "packet=pts_time,size,flags", "-of", "csv=p=0", path],
        capture_output=True, text=True).stdout
    rows = []
    for line in out.splitlines():
        p = line.split(",")
        if len(p) >= 3 and p[0] not in ("", "N/A"):
            rows.append((float(p[0]), int(p[1]), "K" in p[2]))
    return sorted(rows)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scenario", help="記録の JSON Lines (samples/scenarios/*.jsonl)。--server を使うときは無視")
    ap.add_argument("--server", help="replay を立てず、このサーバ (例 https://eq.fuga.jp。読むだけ) の平時の画面を測る")
    ap.add_argument("--duration", type=float, default=120, help="測る秒数 (既定 120)")
    ap.add_argument("--speed", type=float, default=1.0, help="記録を何倍速で流すか (既定 1)")
    ap.add_argument("--port", type=int, default=18099)
    ap.add_argument("--bin", default="target/release/eq-server")
    ap.add_argument("--map-dir", default="web/public")
    ap.add_argument("--font", help="フォント (省くと既定)")
    ap.add_argument("--ffmpeg", default="ffmpeg")
    ap.add_argument("--time-l", action="store_true",
                    help="eq-server を time (Mac: /usr/bin/time -l、Linux: -v) で包み、最大メモリを正確に出す (ffmpeg は標本)")
    ap.add_argument("--keep", action="store_true", help="一時ディレクトリを残す")
    ap.add_argument("--toml", action="append", default=[], help="broadcast.toml に足す行 (例 'fps = 10')。何度でも")
    a = ap.parse_args()

    # ツールが足す行と重なる key は、設定の読み込みで落ちる (表が 0 だけになる) ので、起動前にエラーにする
    own = {"source", "server", "map_dir", "font", "ffmpeg", "test", "output"}
    dup = sorted({m.group(1) for l in a.toml if (m := re.match(r"\s*(\w+)\s*=", l)) and m.group(1) in own})
    if dup:
        ap.error(f"--toml に書けない key: {', '.join(dup)} (map_dir・font は --map-dir・--font を使う)")
    builtin = any(re.match(r'\s*encoder\s*=\s*"builtin"', l) for l in a.toml)

    work = tempfile.mkdtemp(prefix="eq-load-")
    out_ts = os.path.join(work, "out.flv" if builtin else "out.ts")
    open(os.path.join(work, "server.toml"), "w").write(
        f'[server]\nlisten = "127.0.0.1:{a.port}"\nstatic_dir = ""\n\n'
        f'[source]\ntype = "replay"\npath = {json.dumps(os.path.abspath(a.scenario))}\nspeed = {a.speed}\nrebase_time = true\nloop = true\n')
    ffmpeg = a.ffmpeg
    lines = ['source = "native"', f'server = "{a.server or f"http://127.0.0.1:{a.port}"}"', f"map_dir = {json.dumps(os.path.abspath(a.map_dir))}",
             f"ffmpeg = {json.dumps(ffmpeg)}"]
    if not a.server:
        lines.append("test = true")  # replay を流すので、テスト配信の表示にする (server が replay だと、無いと配信が始まらない)
    if a.font:
        lines.append(f"font = {json.dumps(a.font)}")
    out = [out_ts] if builtin else ["-f", "mpegts", out_ts]
    lines += a.toml + [f"output = {json.dumps(out)}"]
    open(os.path.join(work, "broadcast.toml"), "w").write("\n".join(lines) + "\n")

    procs = []
    bc = None
    try:
        if not a.server:
            srv = subprocess.Popen([a.bin, "--config", os.path.join(work, "server.toml")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            procs.append(srv)
            time.sleep(3)  # サーバが待ち受けるまで
        log = open(os.path.join(work, "broadcast.log"), "w")
        time_cmd = ["/usr/bin/time", "-l" if sys.platform == "darwin" else "-v"] if a.time_l else []
        bc = subprocess.Popen(time_cmd + [a.bin, "broadcast", os.path.join(work, "broadcast.toml")], stdout=log, stderr=log)
        procs.append(bc)
        t0 = time.time()
        peak = {"eq-server": 0, "ffmpeg": 0}
        peak_sum = 0
        last = {}
        rows = []
        next_win = t0 + WINDOW
        while time.time() - t0 < a.duration:
            time.sleep(0.1)
            if bc.poll() is not None:  # 配信側が落ちた (設定の誤りなど)。0 の表を出し続けない
                log.flush()
                tail = open(os.path.join(work, "broadcast.log"), errors="replace").read().splitlines()[-10:]
                sys.exit("配信が途中で終わりました (broadcast.log の最後):\n" + "\n".join(tail))
            eq = child_pid(bc.pid, "eq-server") if a.time_l else bc.pid  # time の子が本体
            ff = child_pid(eq, "ffmpeg") if eq else None
            cur = {"eq-server": cpu_rss(eq) if eq else None, "ffmpeg": cpu_rss(ff) if ff else None}
            rss = {k: (v[1] if v else 0) for k, v in cur.items()}
            for k in peak:
                peak[k] = max(peak[k], rss[k])
            peak_sum = max(peak_sum, sum(rss.values()))
            if time.time() >= next_win:
                cpu = {k: ((cur[k][0] - last[k][0]) / WINDOW * 100 if cur[k] and last.get(k) else 0) for k in cur}
                rows.append((time.time() - t0, cpu, rss))
                last = cur
                next_win += WINDOW
            elif not last:
                last = cur
        total_cpu = {k: (cur[k][0] if cur[k] else 0) for k in cur}
    finally:
        for p in procs[::-1]:
            # time で包んだときは、time ではなく中の eq-server に止める合図を送る (time が最大メモリを出して終わる)
            real = child_pid(p.pid, "eq-server") if a.time_l and p is bc else None
            (os.kill(real, 15) if real else p.terminate())
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()

    pk = video_packets(out_ts) if os.path.exists(out_ts) else []
    size = os.path.getsize(out_ts) if os.path.exists(out_ts) else 0
    print(f"scenario={os.path.basename(a.scenario)} duration={a.duration:.0f}s extra={a.toml}")
    print(f"{'t(s)':>5} {'eq cpu%':>8} {'ffmpeg%':>8} {'sum%':>6} {'eq MB':>6} {'ff MB':>6} {'fps':>5} {'video kbps':>10}")
    for t, cpu, rss in rows:
        w = [p for p in pk if t - WINDOW <= p[0] < t]
        fps = len(w) / WINDOW
        kbps = sum(p[1] for p in w) * 8 / WINDOW / 1000
        print(f"{t:5.0f} {cpu['eq-server']:8.1f} {cpu['ffmpeg']:8.1f} {sum(cpu.values()):6.1f} "
              f"{rss['eq-server'] / 1024:6.0f} {rss['ffmpeg'] / 1024:6.0f} {fps:5.1f} {kbps:10.0f}")
    dur = a.duration
    print(f"平均 CPU: eq-server {total_cpu['eq-server'] / dur * 100:.1f}% ffmpeg {total_cpu['ffmpeg'] / dur * 100:.1f}% "
          f"(合計 {sum(total_cpu.values()) / dur * 100:.1f}%)")
    print(f"最大メモリ (0.1 秒ごとの標本): eq-server {peak['eq-server'] / 1024:.0f}MB ffmpeg {peak['ffmpeg'] / 1024:.0f}MB "
          f"合計の最大 {peak_sum / 1024:.0f}MB")
    # エンコーダが先読みで抱えているコマは出力に出ていないので、出た分の長さ (ファイルの長さ) で割る
    media = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", out_ts],
                           capture_output=True, text=True).stdout.strip() if size else ""
    media = float(media) if media not in ("", "N/A") else dur
    print(f"平均の送信量 (映像 + 音の全部): {size * 8 / media / 1000:.0f}kbps (出力の長さ {media:.0f}s / 測った {dur:.0f}s)")
    if pk:
        keys = [p[0] for p in pk if p[2]]
        gaps = [round(b - a_, 1) for a_, b in zip(keys, keys[1:])]
        print(f"キーフレームの間隔 (秒): 最小 {min(gaps, default=0)} 最大 {max(gaps, default=0)}")
    if a.time_l:
        for l in open(os.path.join(work, "broadcast.log"), errors="replace"):
            if "maximum resident" in l or "Maximum resident" in l:
                print(f"eq-server の最大メモリ (time): {l.strip()}  (Mac は bytes、Linux は kbytes)")
    print(f"ログ: {work}/broadcast.log" if a.keep else "")
    if not a.keep:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
