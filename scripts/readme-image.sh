#!/usr/bin/env bash
#
# scripts/readme-image.sh: render the README's hero, docs/images/journey-light.png and
# journey-dark.png, from scripts/readme-image/journey.html (a sample visit in a macOS-style
# window; edit VISIT there to change it).
#
# Usage:
#   scripts/readme-image.sh            both themes into docs/images/
#   scripts/readme-image.sh <out dir>  the same, written somewhere else (to look before replacing)
#
# Needs Google Chrome and Python 3 with Pillow. The page is drawn at 2x on a transparent
# background, so the window's rounded corners and shadow sit on whatever the README is read on;
# the image is cropped to the shadow's edge.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/scripts/readme-image"
OUT="${1:-$ROOT/docs/images}"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
die() { echo "error: $*" >&2; exit 1; }

[[ -x "$CHROME" ]] || die "Google Chrome not found at $CHROME (set CHROME)"
python3 -c 'import PIL' 2>/dev/null || die "Python's Pillow is required: pip install Pillow"
mkdir -p "$OUT"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

for theme in light dark; do
  "$CHROME" --headless=new --user-data-dir="$TMP/profile" --hide-scrollbars --allow-file-access-from-files \
    --force-device-scale-factor=2 --default-background-color=00000000 --window-size=1104,640 \
    --virtual-time-budget=5000 --screenshot="$TMP/$theme.png" "file://$SRC/journey.html?theme=$theme" >/dev/null 2>&1
  [[ -s "$TMP/$theme.png" ]] || die "Chrome did not write the $theme screenshot"
done

# Both themes are cropped to one box, the union of what each drew (the dark shadow reaches
# further), so the README's <picture> swaps between two images of the same size. They stay full
# RGBA: a palette (Pillow's quantize) bands the shadow and tints the window.
python3 - "$TMP" "$OUT" <<'PY'
import sys
from PIL import Image
tmp, out = sys.argv[1:]
ims = {t: Image.open(f"{tmp}/{t}.png").convert("RGBA") for t in ("light", "dark")}
boxes = [im.getchannel("A").point(lambda a: 255 if a > 3 else 0).getbbox() for im in ims.values()]
if not all(boxes):
    sys.exit("a screenshot has nothing drawn")
box = (min(b[0] for b in boxes), min(b[1] for b in boxes), max(b[2] for b in boxes), max(b[3] for b in boxes))
if box[3] >= ims["light"].height - 1:
    sys.exit("the window reaches the bottom of the screenshot; make --window-size taller")
for theme, im in ims.items():
    dest = f"{out}/journey-{theme}.png"
    im.crop(box).save(dest, optimize=True)
    print(f"{dest}: {box[2] - box[0]}x{box[3] - box[1]}")
PY
