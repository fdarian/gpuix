#!/bin/sh
# Regenerates resolved-style-oracle.json from resolved-style-oracle.html with headless Chrome.
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
PAGE="file://$PWD/resolved-style-oracle.html"
dump() { "$CHROME" --headless --window-size=1280,800 --virtual-time-budget=5000 --dump-dom "$1" 2>/dev/null; }
unescape() { sed 's/^data-[a-z]*="//; s/"$//; s/&quot;/"/g; s/&amp;/\&/g'; }

dump "$PAGE" | grep -o 'data-oracle="[^"]*"' | unescape > /tmp/resolved-style-oracle.main.json

python3 - "$PAGE" /tmp/resolved-style-oracle.main.json resolved-style-oracle.json <<'PY'
import json, subprocess, sys, os, urllib.parse, re
page, source, target = sys.argv[1:]
chrome = os.environ.get("CHROME", os.path.expanduser("~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell"))
data = json.load(open(source))
for name, case in data["cases"].items():
    if case["kind"] != "intersection":
        continue
    for state in ("base", "after"):
        url = f"{page}#intersection={urllib.parse.quote(name + '|' + state)}"
        out = subprocess.run([chrome, "--headless", "--window-size=1280,800", "--virtual-time-budget=5000", "--dump-dom", url],
                             capture_output=True, text=True).stdout
        found = re.search(r'data-intersection="([^"]*)"', out)
        if not found:
            raise SystemExit(f"no report for {name} {state}")
        case[state] = json.loads(found.group(1).replace("&quot;", '"').replace("&amp;", "&"))
json.dump(data, open(target, "w"), separators=(",", ":"))
PY
