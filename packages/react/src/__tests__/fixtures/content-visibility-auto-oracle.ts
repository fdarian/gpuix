// Regenerates content-visibility-auto-oracle.json from content-visibility-auto-oracle.html with headless Chrome:
//   bun content-visibility-auto-oracle.ts [scene...]
// Scenes named on the command line are run alone and replace their entries in the file.
// Each scene runs in a page of its own, since only the first thing a page adds is given a rendering update with the
// state of the page before it.
import fs from "fs"
import path from "path"

const here = import.meta.dir
const chrome =
  process.env.CHROME ??
  `${process.env.HOME}/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`
/** The scene of the first frame of a page: the page itself is the scene, and it is read, not driven. */
const LOAD = "load"
const SCENES = ["list", "exact", "clipped", "horizontal", "nested", "focus", "bare", "remember", "forced", "forcedNested", "forcedScroller", "firstReadInside", "firstReadTurns", "firstReadInserted", "firstReadNested", "firstReadAuto", "firstReadScroller", LOAD]
const SIZE = { width: 900, height: 780 }

async function run(scene: string) {
  const port = 9300 + Math.floor(Math.random() * 600)
  const process_ = Bun.spawn(
    [chrome, `--remote-debugging-port=${port}`, `--window-size=${SIZE.width},${SIZE.height}`, "--force-device-scale-factor=1", "--allow-file-access-from-files", "--no-sandbox", "about:blank"],
    { stdout: "ignore", stderr: "ignore" }
  )
  try {
    let page: { webSocketDebuggerUrl: string } | undefined
    for (let attempt = 0; attempt < 50 && !page; attempt++) {
      try {
        const targets = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()) as { type: string; webSocketDebuggerUrl: string }[]
        page = targets.find((target) => target.type === "page")
      } catch {}
      if (!page) await Bun.sleep(100)
    }
    if (!page) throw new Error("Chrome did not start")
    const socket = new WebSocket(page.webSocketDebuggerUrl)
    await new Promise((resolve) => (socket.onopen = resolve))
    let next = 0
    const waiting = new Map<number, (message: any) => void>()
    socket.onmessage = (message) => {
      const data = JSON.parse(message.data as string)
      if (data.id) waiting.get(data.id)?.(data)
    }
    const send = (method: string, params: object = {}) =>
      new Promise<any>((resolve) => {
        const id = ++next
        waiting.set(id, resolve)
        socket.send(JSON.stringify({ id, method, params }))
      })
    await send("Page.enable")
    await send("Emulation.setDeviceMetricsOverride", { ...SIZE, deviceScaleFactor: 1, mobile: false })
    const hash = scene === LOAD ? `#${LOAD}` : ""
    await send("Page.navigate", { url: `file://${path.join(here, "content-visibility-auto-oracle.html")}${hash}` })
    // The scene `load` records itself as the page loads, from the markup on; the page is only read when it is over.
    await Bun.sleep(scene === LOAD ? 1500 : 800)
    const expression = scene === LOAD ? "window.__loaded" : `window.__run(${JSON.stringify(scene)})`
    const result = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })
    socket.close()
    if (!result.result?.result || result.result.exceptionDetails) throw new Error(JSON.stringify(result))
    return result.result.result.value
  } finally {
    process_.kill()
  }
}

const only = process.argv.slice(2)
const file = path.join(here, "content-visibility-auto-oracle.json")
const found: Record<string, unknown> = only.length ? JSON.parse(fs.readFileSync(file, "utf8")) : {}
const run_ = only.length ? only : SCENES
for (const scene of run_) found[scene] = await run(scene)
fs.writeFileSync(file, JSON.stringify(found))
console.log(`wrote ${run_.length} scenes`)
