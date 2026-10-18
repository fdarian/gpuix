#!/bin/sh
# Regenerates container-hosts-oracle.json from container-hosts-oracle.html with headless Chrome.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
"$CHROME" --headless --allow-file-access-from-files --window-size=1280,800 --virtual-time-budget=5000 --dump-dom "file://$PWD/container-hosts-oracle.html" 2>/dev/null \
  | grep -o 'data-rects="[^"]*"' | sed 's/^data-rects="//;s/"$//;s/&quot;/"/g;s/&amp;/\&/g' > container-hosts-oracle.json
