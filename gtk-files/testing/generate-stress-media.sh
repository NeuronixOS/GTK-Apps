#!/usr/bin/env bash
# Generate black images + videos for gtk-files copy/paste stress tests.
#
# Usage:
#   ./generate-stress-media.sh
#   ./generate-stress-media.sh /path/to/output
#   COUNT=500 ./generate-stress-media.sh ~/SORT/stress
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

have magick || have convert || die "need ImageMagick (magick or convert)"
have ffmpeg || die "need ffmpeg"

im() {
  if have magick; then
    magick "$@"
  else
    convert "$@"
  fi
}

# --- output directory ---
OUT="${1:-}"
if [[ -z "$OUT" ]]; then
  read -r -p "Output directory: " OUT
fi
[[ -n "$OUT" ]] || die "output directory required"

# Expand ~ and relative paths
OUT="${OUT/#\~/$HOME}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

COUNT="${COUNT:-200}"          # many small stills
VIDEO_COUNT="${VIDEO_COUNT:-8}" # short black clips
BIG_BINS="${BIG_BINS:-1}"      # zero-filled blobs for throughput

echo "Writing stress media under: $OUT"
echo "  stills:      $COUNT"
echo "  videos:      $VIDEO_COUNT"
echo "  big bins:    $BIG_BINS"
echo

mkdir -p \
  "$OUT/images/small" \
  "$OUT/images/large" \
  "$OUT/videos" \
  "$OUT/bins"

# --- many small black JPEGs (folder paste volume) ---
echo "== small black JPEGs ($COUNT) =="
for i in $(seq -w 1 "$COUNT"); do
  im -size 1280x720 xc:black -quality 85 "$OUT/images/small/frame-$i.jpg"
  if (( 10#$i % 25 == 0 )) || [[ "$i" == "$(printf '%0*d' ${#COUNT} "$COUNT")" ]]; then
    echo "  … $i / $COUNT"
  fi
done

# --- assorted large stills ---
echo "== large black stills =="
im -size 4000x4000 xc:black -quality 95 "$OUT/images/large/huge-4k.jpg"
im -size 8000x8000 xc:black "$OUT/images/large/huge-8k.png"
im -size 12000x12000 xc:black "$OUT/images/large/huge-12k.png"
echo "  done large stills"

# --- black videos ---
echo "== black videos ($VIDEO_COUNT) =="
for i in $(seq -w 1 "$VIDEO_COUNT"); do
  # Vary resolution / length a bit so paste isn't one identical blob.
  case $((10#$i % 4)) in
    0) size=1280x720;  secs=15 ;;
    1) size=1920x1080; secs=30 ;;
    2) size=2560x1440; secs=20 ;;
    *) size=3840x2160; secs=10 ;;
  esac
  out="$OUT/videos/black-${size}-${secs}s-$i.mp4"
  ffmpeg -y -loglevel error -stats \
    -f lavfi -i "color=c=black:s=${size}:d=${secs}" \
    -c:v libx264 -pix_fmt yuv420p -t "$secs" \
    "$out"
done
echo "  done videos"

# --- zero-filled bins (copy throughput without decode) ---
if [[ "$BIG_BINS" -gt 0 ]]; then
  echo "== zero-filled bins =="
  dd if=/dev/zero of="$OUT/bins/big-256M.bin" bs=1M count=256 status=progress
  if [[ "$BIG_BINS" -ge 2 ]]; then
    dd if=/dev/zero of="$OUT/bins/big-1G.bin" bs=1M count=1024 status=progress
  fi
fi

echo
echo "Done."
du -sh "$OUT" "$OUT"/images/* "$OUT"/videos "$OUT"/bins 2>/dev/null || true
echo
echo "Example paste test:"
echo "  Select $OUT/images/small in gtk-files → Copy → Paste into another folder"
