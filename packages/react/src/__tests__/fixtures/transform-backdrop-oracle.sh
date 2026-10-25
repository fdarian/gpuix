#!/bin/sh
# Regenerates transform-backdrop-oracle.json from transform-backdrop-oracle.html with headless Chrome: the shape of a
# backdrop-filter box under CSS transforms, and the blur of a backdrop-filter and a filter under them
# (transform-backdrop-oracle.py), at device scale 1.
set -eu
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
flags="--headless --force-device-scale-factor=1 --window-size=1600,1650 --virtual-time-budget=3000 --allow-file-access-from-files --hide-scrollbars --user-data-dir=$WORK/profile"
"$CHROME" $flags --dump-dom "file://$PWD/transform-backdrop-oracle.html" > "$WORK/dump.html" 2>/dev/null
"$CHROME" $flags "--screenshot=$WORK/shot.png" "file://$PWD/transform-backdrop-oracle.html" > /dev/null 2>&1
/usr/bin/python3 -I transform-backdrop-oracle.py "$WORK/dump.html" "$WORK/shot.png" transform-backdrop-oracle.json
