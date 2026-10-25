#!/bin/sh
# Regenerates scrollbar-gutter-oracle.json from scrollbar-gutter-oracle.html with headless Chrome. The page asks for classic
# bars with ::-webkit-scrollbar, so --hide-scrollbars is left off.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
"$CHROME" --headless --window-size=800,600 --force-device-scale-factor=1 --allow-file-access-from-files \
  --virtual-time-budget=2000 --dump-dom "file://$PWD/scrollbar-gutter-oracle.html" 2>/dev/null | grep -o 'data-oracle="[^"]*"' \
  | python3 -I -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' > scrollbar-gutter-oracle.json
