#!/bin/sh
# Regenerates scroll-extent-oracle.json from scroll-extent-oracle.html with headless Chrome: the scroll extents of every
# case at device scale 1 and 2, as {"x1": [case, ...], "x2": [...]}.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
PAGE="file://$PWD/scroll-extent-oracle.html"
for scale in 1 2; do
  "$CHROME" --headless --window-size=1280,800 --force-device-scale-factor=$scale --virtual-time-budget=3000 \
    --allow-file-access-from-files --dump-dom "$PAGE" 2>/dev/null | grep -o 'data-rects="[^"]*"' \
    | python3 -I -c 'import html, re, sys; print(html.unescape(re.sub(r"^data-rects=\"|\"$", "", sys.stdin.read().strip())))' > /tmp/scroll-extent-oracle.$scale.json
done
python3 -I - scroll-extent-oracle.json <<'PY'
import json, sys
out = {}
for scale in (1, 2):
    page = json.load(open(f"/tmp/scroll-extent-oracle.{scale}.json"))
    assert page["scale"] == scale, page["scale"]
    out[f"x{scale}"] = page["results"]
json.dump(out, open(sys.argv[1], "w"), separators=(",", ":"))
PY
