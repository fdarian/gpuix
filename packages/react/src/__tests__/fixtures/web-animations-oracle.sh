#!/bin/sh
# Regenerates web-animations-oracle.json from web-animations-oracle.html with headless Chrome.
# The virtual time budget lets the page wait for frames and promises.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
"$CHROME" --headless --window-size=1280,800 --virtual-time-budget=20000 --dump-dom "file://$PWD/web-animations-oracle.html" 2>/dev/null \
  | grep -o 'data-oracle="[^"]*"' \
  | python3 -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' \
  > web-animations-oracle.json
