#!/usr/bin/env bash
#
# scripts/hero-video.sh: render the opening clip for lib-visits' page on apps.csarko.sh,
# docs/images/hero.mp4 (2880 by 1600, 30 fps), with its last frame as docs/images/hero.jpg (the
# page's poster and share card), from scripts/hero-video/hero.html: the README's sample visit
# happening, the camera close on each step, then pulling back into the whole window. The visit is
# journey.html's; change it in both.
#
# Usage:
#   scripts/hero-video.sh              into docs/images/
#   scripts/hero-video.sh <out dir>    somewhere else, to look before replacing
#   scripts/hero-video.sh --preview    a quarter of the size at 15 fps into /tmp, in seconds
#
# Needs Node 22, Google Chrome and ffmpeg. Every frame is drawn for its own time, so the clip is
# the same on every run whatever the machine is doing.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
die() { echo "error: $*" >&2; exit 1; }
command -v node >/dev/null || die "Node 22 is required"
command -v ffmpeg >/dev/null || die "ffmpeg is required"
export CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[[ -x "$CHROME" ]] || die "Google Chrome not found at $CHROME (set CHROME)"

if [[ "${1:-}" == --preview ]]; then
  OUT="${TMPDIR:-/tmp}/lib-visits-hero"
  mkdir -p "$OUT"
  node "$ROOT/scripts/hero-video/render.mjs" "$OUT/hero.mp4" --poster "$OUT/hero.jpg" --preview
  exit 0
fi
OUT="${1:-$ROOT/docs/images}"
mkdir -p "$OUT"
node "$ROOT/scripts/hero-video/render.mjs" "$OUT/hero.mp4" --poster "$OUT/hero.jpg"
