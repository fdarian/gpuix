#!/usr/bin/env -S uv run --with playwright python
"""Regenerates motion-waapi-oracle.json: which animations `element.getAnimations()` lists for the animations Motion 14.1.0
makes in headless Chrome, and what they look like. Vanilla cases are `Motion.animate(el, keyframes, options)` and React
cases a `motion.div`; motion-waapi-oracle.html holds the cases and the page records, with each sample, the calls the case
had made to Motion by then (`calls`), which is what a replay makes.

  MOTION_ROOT=<node_modules holding motion, framer-motion, react and react-dom, at motion 14.1.0> \\
    uv run --with playwright python motion-waapi-oracle.py

The page needs `vendor/motion.js` (motion's `dist/motion.js`, the UMD build, copied unchanged) and
`vendor/motion-react.js` (`bun build --production` of motion-waapi-oracle-react-entry.js, which imports framer-motion and
react). They are built into a temporary directory from MOTION_ROOT, next to a copy of the page, and are not kept.

Real time, not the virtual time of the other oracles: `--dump-dom` exits before the page's timers run. Every case runs in
its own page (`?case=<id>`) because Motion's keyframe-resolver queue is global and any `ctrl.state/time` read flushes
it. Only the headless shell Playwright installs is started (CHROME overrides its path); no browser of the user's is
touched.
"""
import asyncio, json, os, shutil, subprocess, sys, tempfile

here = os.path.dirname(os.path.abspath(__file__))
root = os.environ.get("MOTION_ROOT")
if not root:
    sys.exit("set MOTION_ROOT to the node_modules directory that holds motion 14.1.0")
exe = os.environ.get("CHROME", os.path.expanduser("~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell"))
WORKERS = int(os.environ.get("WORKERS", "6"))


def build(work):
    """The page and the two bundles it loads, in `work`."""
    os.makedirs(os.path.join(work, "vendor"))
    shutil.copy(os.path.join(root, "motion", "dist", "motion.js"), os.path.join(work, "vendor", "motion.js"))
    shutil.copy(os.path.join(here, "motion-waapi-oracle.html"), os.path.join(work, "oracle.html"))
    entry = os.path.join(work, "react-entry.js")
    shutil.copy(os.path.join(here, "motion-waapi-oracle-react-entry.js"), entry)
    # The bare imports of the entry resolve through a node_modules next to it.
    os.symlink(root, os.path.join(work, "node_modules"))
    subprocess.run(["bun", "build", entry, "--production", "--outfile", os.path.join(work, "vendor", "motion-react.js")], check=True, cwd=work)


async def fetch(browser, url, query):
    from playwright.async_api import Page  # noqa: F401  (the import is the dependency check)
    page = await browser.new_page(viewport={"width": 1280, "height": 800})
    page.on("pageerror", lambda e: print("pageerror:", query, e, file=sys.stderr))
    await page.goto(url + "?" + query)
    await page.wait_for_function("window.__oracle", timeout=120000)
    data = json.loads(await page.evaluate("document.documentElement.getAttribute('data-oracle')"))
    await page.close()
    return data


async def main(work):
    from playwright.async_api import async_playwright

    url = "file://" + os.path.join(work, "oracle.html")
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=exe)
        ids = (await fetch(browser, url, "list=1"))["ids"]
        sem = asyncio.Semaphore(WORKERS)

        async def one(i):
            async with sem:
                return await fetch(browser, url, "case=" + i)

        results = await asyncio.gather(*[one(i) for i in ids])
        await browser.close()
    data = {"meta": results[0]["meta"], "cases": [r["cases"][0] for r in results]}
    with open(os.path.join(here, "motion-waapi-oracle.json"), "w") as f:
        json.dump(data, f, indent=1)
    print(len(data["cases"]), "cases; errors:", [c["id"] for c in data["cases"] if c["error"]])


with tempfile.TemporaryDirectory(prefix="motion-waapi-oracle-") as work:
    build(work)
    asyncio.run(main(work))
