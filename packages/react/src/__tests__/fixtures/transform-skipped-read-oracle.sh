#!/bin/sh
# Regenerates transform-skipped-read-oracle.json from transform-skipped-read-oracle.html with headless Chrome at device
# scale 1: what getBoundingClientRect answers inside content-visibility-skipped content right after a transform change.
# The page awaits rAFs, and `--dump-dom` (even with --virtual-time-budget) gives a page one frame, so the script starts the
# cached chrome-headless-shell (fresh temporary profile) through playwright (uv supplies it), as the animation oracle does.
# CHROME=<path> picks another binary.
set -eu
cd "$(dirname "$0")"
export CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
export PAGE="file://$PWD/transform-skipped-read-oracle.html"
uv run --quiet --with playwright python - transform-skipped-read-oracle.json <<'PY'
import json, os, sys
from playwright.sync_api import sync_playwright

with sync_playwright() as p:
    browser = p.chromium.launch(executable_path=os.environ["CHROME"], args=["--allow-file-access-from-files", "--no-sandbox"])
    try:
        page = browser.new_page(viewport={"width": 1280, "height": 800}, device_scale_factor=1)
        page.goto(os.environ["PAGE"])
        # polling must be an interval: the default polls from requestAnimationFrame, which adds frames of its own
        page.wait_for_function('document.body.hasAttribute("data-oracle") || document.body.hasAttribute("data-error")', timeout=300000, polling=100)
        error = page.evaluate('document.body.getAttribute("data-error")')
        if error:
            raise SystemExit(error)
        data = json.loads(page.evaluate('document.body.getAttribute("data-oracle")'))
        data["chromeVersion"] = browser.version
    finally:
        browser.close()
json.dump(data, open(sys.argv[1], "w"), separators=(",", ":"))
PY
