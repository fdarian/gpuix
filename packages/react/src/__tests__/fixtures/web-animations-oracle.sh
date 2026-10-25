#!/bin/sh
# Regenerates web-animations-oracle.json from web-animations-oracle.html with headless Chrome: the first kind of case
# with a virtual time budget, the other two with web-animations-oracle.mjs (node 22+) in real time. See the page.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
FIRST=$(mktemp)
SECOND=$(mktemp)
trap 'rm -f "$FIRST" "$SECOND"' EXIT
# The virtual time budget lets the page wait for frames and promises.
"$CHROME" --headless --window-size=1280,800 --virtual-time-budget=20000 --dump-dom "file://$PWD/web-animations-oracle.html" 2>/dev/null \
  | grep -o 'data-oracle="[^"]*"' \
  | python3 -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-oracle=\"|\"$", "", sys.stdin.read().strip())))' \
  > "$FIRST"
node web-animations-oracle.mjs "$CHROME" > "$SECOND"
# Both are one JSON object; the second's keys go after the first's, which stay as the first kind has always had them.
python3 -c 'import sys; first = open(sys.argv[1]).read().strip(); second = open(sys.argv[2]).read().strip(); print(first[:-1] + "," + second[1:])' "$FIRST" "$SECOND" \
  > web-animations-oracle.json
