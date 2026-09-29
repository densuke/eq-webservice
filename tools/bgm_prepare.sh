#!/bin/sh
# 平時の BGM の前処理 (本店の Mac など、ffmpeg のある手元で動かす)。
# 入力のディレクトリの曲 (m4a / mp3 / wav / flac / ogg など) を、配信用の MP3 (44.1kHz・ステレオ・160kbps 固定) にする。
# 曲の頭 1.5 秒と終わり 3 秒にフェードを付け、タイトル・アーティストのタグは引き継ぐ。
# 変換済みで元の曲より新しいものは変換し直さない。できた MP3 は rsync で e2 に送る (eq-server bgm-send が流す)。
#
# 使い方:
#   tools/bgm_prepare.sh <入力のディレクトリ> <出力のディレクトリ>
#   rsync -av --delete <出力のディレクトリ>/ e2:work/eq-webservice/data/bgm-mp3/
set -eu

if [ $# -ne 2 ]; then
  echo "usage: $0 <入力のディレクトリ> <出力のディレクトリ>" >&2
  exit 2
fi
src=$1
out=$2
mkdir -p "$out"

for f in "$src"/*; do
  [ -f "$f" ] || continue
  case "$(printf '%s' "${f##*.}" | tr '[:upper:]' '[:lower:]')" in
    m4a | mp3 | wav | flac | ogg | oga | opus | aac) ;;
    *) continue ;;
  esac
  name=$(basename "$f")
  dst="$out/${name%.*}.mp3"
  if [ -f "$dst" ] && [ "$dst" -nt "$f" ]; then
    continue
  fi
  dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$f")
  fade_out=$(awk -v d="$dur" 'BEGIN { s = d - 3; if (s < 0) s = 0; print s }')
  echo "変換: $name" >&2
  ffmpeg -nostdin -loglevel error -y -i "$f" -vn \
    -af "afade=t=in:d=1.5,afade=t=out:st=${fade_out}:d=3" \
    -ar 44100 -ac 2 -c:a libmp3lame -b:a 160k \
    -map_metadata 0 -id3v2_version 3 -write_xing 0 \
    "$dst"
done

# 元の曲が無くなった MP3 は消す
for d in "$out"/*.mp3; do
  [ -f "$d" ] || continue
  stem=$(basename "${d%.*}")
  found=
  for f in "$src/$stem".*; do
    [ -f "$f" ] && found=1
  done
  if [ -z "$found" ]; then
    echo "削除: $(basename "$d")" >&2
    rm -f "$d"
  fi
done
