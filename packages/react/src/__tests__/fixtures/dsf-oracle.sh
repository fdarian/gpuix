#!/bin/sh
# Regenerates <name>-oracle.json (device scale 1) and <name>-oracle-x2.json (device scale 2) from <name>-oracle.html
# with headless Chrome, for the oracles named (default: line-box inline-strut text-baseline):
#
#   sh dsf-oracle.sh line-box
#
# Each file maps a case id to [x, y, width, height] relative to its `data-case` box, as the page reports it.
#
# Scale 2 is `--force-device-scale-factor=2`, not Playwright's `deviceScaleFactor: 2` (CDP
# `Emulation.setDeviceMetricsOverride`). The flag is the display's own scale, which is what a Retina screen is: Blink
# zooms the page by it (zoom-for-DSF), so a font is measured at its device size and its ascent, descent and line gap
# are rounded in device pixels (Menlo 20px has a content area of 23 CSS px, 37 + 9 device px, and a 24px line puts its
# text 0.5 px below the top). The emulation leaves layout at the CSS size and only scales the raster, so it measures
# every font as at scale 1 (content area 24, text at 0 in the same line) and says nothing about a 2x display.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
NAMES=${*:-line-box inline-strut text-baseline}
for name in $NAMES; do
  for scale in 1 2; do
    "$CHROME" --headless --window-size=1280,800 --force-device-scale-factor=$scale --virtual-time-budget=3000 \
      --allow-file-access-from-files --dump-dom "file://$PWD/$name-oracle.html" 2>/dev/null | grep -o 'data-rects="[^"]*"' \
      | python3 -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-rects=\"|\"$", "", sys.stdin.read().strip())))' > /tmp/$name-oracle.$scale.json
  done
  python3 - "$name" <<'PY'
import json, sys
name = sys.argv[1]
for scale, suffix in ((1, ""), (2, "-x2")):
    with open(f"{name}-oracle{suffix}.json", "w") as out:
        json.dump(json.load(open(f"/tmp/{name}-oracle.{scale}.json")), out, indent=1)
        out.write("\n")
PY
done
