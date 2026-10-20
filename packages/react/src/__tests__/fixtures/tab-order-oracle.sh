#!/bin/sh
# Regenerates tab-order-oracle.json from tab-order-oracle.html with headless Chrome driven by playwright (Tab needs a real
# key press). Needs uv, and the headless shell that playwright cached (no download here).
set -e
cd "$(dirname "$0")"
CHROME=${CHROME:-$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell}
HTML="$PWD/tab-order-oracle.html" CHROME="$CHROME" uv run --quiet --with playwright python - > tab-order-oracle.json <<'PY'
import json, os
from playwright.sync_api import sync_playwright

with sync_playwright() as playwright:
    browser = playwright.chromium.launch(executable_path=os.environ['CHROME'])
    page = browser.new_page(viewport={'width': 800, 'height': 600}, device_scale_factor=1)
    page.goto('file://' + os.environ['HTML'])
    cases = page.evaluate('window.CASES')
    out = []
    for index, case in enumerate(cases):
        # A fresh page per run: after a scene is replaced, Chrome starts from where the old one was.
        def open_case():
            page.goto('file://' + os.environ['HTML'])
            page.evaluate('(index) => window.mount(index)', index)
        open_case()
        count = page.evaluate('Array.from(document.querySelectorAll("[tabindex]")).filter(e => e.tabIndex >= 0 && getComputedStyle(e).display !== "none").length')
        tab = []
        for _ in range(count):
            page.keyboard.press('Tab')
            tab.append(page.evaluate('document.activeElement.id'))
        # Shift+Tab from the last stop back to the first.
        shift_tab = [tab[-1]]
        for _ in range(count - 1):
            page.keyboard.press('Shift+Tab')
            shift_tab.append(page.evaluate('document.activeElement.id'))
        out.append({'name': case['name'], 'tree': case['tree'], 'tab': tab, 'shiftTab': shift_tab})
    browser.close()
print(json.dumps(out, indent=1))
PY
