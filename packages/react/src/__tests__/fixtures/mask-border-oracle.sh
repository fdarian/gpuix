#!/bin/sh
# Regenerates mask-border-oracle.json from mask-border-oracle.html with headless Chrome.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
SHOT=$(mktemp -d)/shot.png
ROWS=$(( ($("$CHROME" --headless --dump-dom "file://$PWD/mask-border-oracle.html" 2>/dev/null \
  | grep -o 'data-oracle="[^"]*"' \
  | python3 -I -c 'import html, json, re, sys; print(len(json.loads(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))))') + 4) / 5 ))
"$CHROME" --headless --window-size=1000,$((ROWS * 200)) --hide-scrollbars --screenshot="$SHOT" "file://$PWD/mask-border-oracle.html" 2>/dev/null
"$CHROME" --headless --dump-dom "file://$PWD/mask-border-oracle.html" 2>/dev/null \
  | grep -o 'data-oracle="[^"]*"' \
  | python3 -I mask-border-oracle.py "$SHOT" > mask-border-oracle.json
