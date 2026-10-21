#!/bin/sh
# Regenerates content-visibility-oracle.json from content-visibility-oracle.html and content-visibility-cases.json
# with headless Chrome: the cases of `content-visibility: hidden` in one page, and each `auto` case in a page of its
# own (only the first thing a page adds is given a rendering update, which decides whether an `auto` element is on
# screen).
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT
dump() {
  "$CHROME" --headless --allow-file-access-from-files --window-size=1280,800 --force-device-scale-factor=1 --virtual-time-budget=30000 \
    --dump-dom "file://$PWD/content-visibility-oracle.html$1" 2>/dev/null \
    | grep -o 'data-rects="[^"]*"' | sed 's/^data-rects="//;s/"$//;s/&quot;/"/g;s/&amp;/\&/g'
}
dump "" > "$OUT/hidden.json"
for name in $(python3 -c 'import json; print(*[c["name"] for c in json.load(open("content-visibility-cases.json"))["cases"] if c["target"].get("contentVisibility") == "auto"])'); do
  dump "?case=$name" > "$OUT/$name.json"
done
python3 - "$OUT" content-visibility-oracle.json <<'PY'
import json, sys
order = [c["name"] for c in json.load(open("content-visibility-cases.json"))["cases"]]
found = {}
for name in __import__("os").listdir(sys.argv[1]):
    found.update(json.load(open(f"{sys.argv[1]}/{name}")))
assert sorted(found) == sorted(order), set(order) ^ set(found)
json.dump({name: found[name] for name in order}, open(sys.argv[2], "w"), separators=(",", ":"))
PY
