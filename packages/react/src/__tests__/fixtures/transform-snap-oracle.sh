#!/bin/sh
# Regenerates transform-snap-oracle.json from transform-snap-oracle.html with headless Chrome: the vertical edges
# (transform-snap-oracle.py) of 528 boxes (a background, a border, both) moved by a translation (a `left`, a `transform`,
# a `will-change: transform` one and a `translateX() translateZ(0)` one) inside parents at fractional positions, at device
# scale 1 and 2, as {"x1": {...}, "x2": {...}}.
set -eu
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
for scale in 1 2; do
  flags="--headless --force-device-scale-factor=$scale --window-size=1280,5900 --virtual-time-budget=3000 --allow-file-access-from-files --hide-scrollbars --user-data-dir=$WORK/profile$scale"
  "$CHROME" $flags --dump-dom "file://$PWD/transform-snap-oracle.html" > "$WORK/dump$scale.html" 2>/dev/null
  "$CHROME" $flags "--screenshot=$WORK/shot$scale.png" "file://$PWD/transform-snap-oracle.html" > /dev/null 2>&1
  /usr/bin/python3 -I transform-snap-oracle.py "$WORK/dump$scale.html" "$WORK/shot$scale.png" "$scale" "$WORK/oracle$scale.json"
done
/usr/bin/python3 -I - "$WORK" transform-snap-oracle.json <<'PY'
import json, sys
work, out = sys.argv[1:3]
json.dump({f"x{scale}": json.load(open(f"{work}/oracle{scale}.json")) for scale in (1, 2)}, open(out, "w"), separators=(",", ":"))
PY
