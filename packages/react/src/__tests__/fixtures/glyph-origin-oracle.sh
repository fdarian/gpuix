#!/bin/sh
# Regenerates glyph-origin-oracle.json from glyph-origin-oracle.html with headless Chrome: for every label of the page the
# boxes Chrome lays out and the first and last device pixel rows its glyphs (and its chip text's) are painted on, at device
# scale 1 and 2, as {"x1": {"scale": 1, "cases": [...]}, "x2": {...}}. The font is inter-subset.ttf.
set -eu
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
for scale in 1 2; do
  flags="--headless --force-device-scale-factor=$scale --window-size=1920,400 --virtual-time-budget=3000 --allow-file-access-from-files --hide-scrollbars"
  "$CHROME" $flags --dump-dom "file://$PWD/glyph-origin-oracle.html" > "$WORK/dump$scale.html" 2>/dev/null
  "$CHROME" $flags "--screenshot=$WORK/shot$scale.png" "file://$PWD/glyph-origin-oracle.html" > /dev/null 2>&1
  /usr/bin/python3 -I glyph-origin-oracle.py "$WORK/dump$scale.html" "$WORK/shot$scale.png" "$scale" "$WORK/oracle$scale.json"
done
/usr/bin/python3 -I - "$WORK" glyph-origin-oracle.json <<'PY'
import json, sys
work, out = sys.argv[1:3]
json.dump({f"x{scale}": json.load(open(f"{work}/oracle{scale}.json")) for scale in (1, 2)}, open(out, "w"), separators=(",", ":"))
PY
