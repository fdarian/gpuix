#!/bin/sh
# Regenerates transform-geometry-oracle.json from transform-geometry-oracle.html with headless Chrome at device scale 1:
# for every scene its markup and what Chrome measures of it (see the page).
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
"$CHROME" --headless --window-size=1280,800 --force-device-scale-factor=1 --allow-file-access-from-files --virtual-time-budget=3000 \
  --dump-dom "file://$PWD/transform-geometry-oracle.html" 2>/dev/null \
  | grep -o 'data-oracle="[^"]*"' \
  | python3 -I -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' \
  > transform-geometry-oracle.json
