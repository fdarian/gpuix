#!/bin/sh
# Regenerates sticky-padding-oracle.json from sticky-padding-oracle.html with headless Chrome.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
"$CHROME" --headless --window-size=1280,800 --allow-file-access-from-files --virtual-time-budget=3000 --dump-dom "file://$PWD/sticky-padding-oracle.html" 2>/dev/null \
  | grep -o 'data-oracle="[^"]*"' \
  | python3 -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' \
  > sticky-padding-oracle.json
