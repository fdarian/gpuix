#!/bin/sh
# Regenerates font-smoothing-oracle.json from font-smoothing-oracle.html with headless Chrome: the ink coverage of 48
# samples (24 with -webkit-font-smoothing: antialiased, 24 with auto) at device scale 1 and 2, as {"x1": {...}, "x2": {...}}.
# The font is inter-subset.ttf (Inter Variable cut down with pyftsubset to the glyphs of the samples).
set -eu
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
for scale in 1 2; do
  flags="--headless --force-device-scale-factor=$scale --window-size=1200,1160 --virtual-time-budget=3000 --allow-file-access-from-files --hide-scrollbars"
  "$CHROME" $flags --dump-dom "file://$PWD/font-smoothing-oracle.html" > "$WORK/dump$scale.html" 2>/dev/null
  "$CHROME" $flags "--screenshot=$WORK/shot$scale.png" "file://$PWD/font-smoothing-oracle.html" > /dev/null 2>&1
  /usr/bin/python3 -I font-smoothing-oracle.py "$WORK/dump$scale.html" "$WORK/shot$scale.png" "$scale" "$WORK/oracle$scale.json"
done
/usr/bin/python3 -I - "$WORK" font-smoothing-oracle.json <<'PY'
import json, sys
work, out = sys.argv[1:3]
json.dump({f"x{scale}": json.load(open(f"{work}/oracle{scale}.json")) for scale in (1, 2)}, open(out, "w"), separators=(",", ":"))
PY
