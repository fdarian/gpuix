#!/usr/bin/env -S uv run --with playwright python
"""Regenerates motion-layout-oracle.json: how Motion 14.1.0 animates a layout change of a `motion.div` (`layout`,
`layout="position"`, `layout="size"`, `layoutId`) in headless Chrome. motion-layout-oracle.html holds the scenes: each lays
out a `before`, then on `window.__flip()` renders an `after` and records the `getBoundingClientRect()` of the tracked boxes
(and, for `radius`, their computed border radius and shadow) on every animation frame for 1600 ms.

  MOTION_ROOT=<node_modules holding motion, framer-motion, react and react-dom, at motion 14.1.0> \\
    uv run --with playwright python motion-layout-oracle.py

The page needs `vendor/motion-react.js` (`bun build --production` of motion-layout-oracle-react-entry.js, which imports
framer-motion and react). It is built into a temporary directory from MOTION_ROOT, next to a copy of the page, and is not
kept.

Every scene runs in its own page (`?scene=<name>`), one after the other: the timing is the point. The frames are rebased to
the start of the animation, which is not the instant of the flip (Motion starts it a frame or so later): the start is the
median of `t - 1000 * p` over the frames with 0.1 < p < 0.9, where p is the progress of the scene's primary quantity of its
primary box between its `before` and `after` rects. Only the headless shell Playwright installs is started (CHROME
overrides its path); no browser of the user's is touched.
"""
import asyncio, json, os, shutil, statistics, subprocess, sys, tempfile

here = os.path.dirname(os.path.abspath(__file__))
root = os.environ.get("MOTION_ROOT")
if not root:
    sys.exit("set MOTION_ROOT to the node_modules directory that holds motion 14.1.0")
exe = os.environ.get("CHROME", os.path.expanduser("~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell"))

# scene -> (primary box, primary quantity), or None when nothing animates.
SCENES = {
    "height": ("box", "height"),
    "widthMove": ("box", "width"),
    "position": ("box", "y"),
    "size": ("box", "height"),
    "moveOnlySize": None,
    "nested": ("parent", "height"),
    "radius": ("card", "width"),
    "layoutId": ("card", "width"),
}
KEEP = (-50, 1100)


def build(work):
    os.makedirs(os.path.join(work, "vendor"))
    shutil.copy(os.path.join(here, "motion-layout-oracle.html"), os.path.join(work, "oracle.html"))
    entry = os.path.join(work, "react-entry.js")
    shutil.copy(os.path.join(here, "motion-layout-oracle-react-entry.js"), entry)
    # The bare imports of the entry resolve through a node_modules next to it.
    os.symlink(root, os.path.join(work, "node_modules"))
    subprocess.run(["bun", "build", entry, "--production", "--outfile", os.path.join(work, "vendor", "motion-react.js")], check=True, cwd=work)


def progress(frame, primary, before, after):
    box = frame["boxes"][primary[0]]
    if box is None:
        return None
    start, end = before[primary[0]][primary[1]], after[primary[0]][primary[1]]
    return (box[primary[1]] - start) / (end - start)


def fit(name, data):
    """The scene as the JSON stores it, its frames rebased to the start of the animation."""
    primary = SCENES[name]
    before, after = data["before"], data["after"]
    if primary is None:
        start = 0.0
        # Nothing animates: every frame must already be the final rect.
        for f in data["frames"]:
            for id, box in f["boxes"].items():
                if box != after[id]:
                    print("  moveOnlySize: frame at", round(f["t"], 1), "has", id, box, "not", after[id], file=sys.stderr)
    else:
        samples = []
        for f in data["frames"]:
            p = progress(f, primary, before, after)
            if p is not None and 0.1 < p < 0.9:
                samples.append(f["t"] - 1000 * p)
        start = statistics.median(samples)
    frames = [dict(f, t=f["t"] - start) for f in data["frames"] if KEEP[0] <= f["t"] - start <= KEEP[1]]
    if primary is not None:
        deviation = max(
            abs(p - f["t"] / 1000)
            for f in frames
            if f["t"] >= 0 and (p := progress(f, primary, before, after)) is not None and 0 <= f["t"] <= 1000
        )
    else:
        deviation = 0.0
    print(f"{name}: {len(frames)} frames, start {start:.1f} ms, max |p - t/1000| = {deviation:.4f}")
    return {"start": start, "before": before, "after": after, "frames": frames}


async def record(browser, url, name):
    page = await browser.new_page(viewport={"width": 1280, "height": 800}, device_scale_factor=1)
    page.on("pageerror", lambda e: print("pageerror:", name, e, file=sys.stderr))
    await page.goto(url + "?scene=" + name)
    await page.wait_for_function("window.__ready", timeout=60000)
    await page.evaluate("window.__flip()")
    await page.wait_for_function("window.__oracle", timeout=60000)
    data = json.loads(await page.evaluate("document.documentElement.getAttribute('data-oracle')"))
    await page.close()
    return data


async def main(work):
    from playwright.async_api import async_playwright

    url = "file://" + os.path.join(work, "oracle.html")
    scenes = {}
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=exe)
        for name in SCENES:
            scenes[name] = fit(name, await record(browser, url, name))
        version = browser.version
        await browser.close()
    with open(os.path.join(root, "motion", "package.json")) as f:
        motion_version = json.load(f)["version"]
    out = {"meta": {"motion": motion_version, "chrome": version}, "scenes": scenes}
    with open(os.path.join(here, "motion-layout-oracle.json"), "w") as f:
        json.dump(out, f, indent=1)


with tempfile.TemporaryDirectory(prefix="motion-layout-oracle-") as work:
    build(work)
    asyncio.run(main(work))
