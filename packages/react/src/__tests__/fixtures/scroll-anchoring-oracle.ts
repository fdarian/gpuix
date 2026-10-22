// Regenerates scroll-anchoring-oracle.json from scroll-anchoring-oracle.html with headless Chrome:
//   bun scroll-anchoring-oracle.ts [scene...]
// Each scene of scroll-anchoring-scenes.ts runs in a page of its own. With scene names, only those are run again and the
// other scenes of the file are kept.
import fs from "fs"
import path from "path"
import { SCENES } from "./scroll-anchoring-scenes"

const here = import.meta.dir
const chrome =
  process.env.CHROME ??
  `${process.env.HOME}/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`
const SIZE = { width: 600, height: 500 }

async function run(scene: string) {
  const definition = SCENES[scene]
  if (!definition) throw new Error(`no scene ${scene}`)
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
    await send("Page.navigate", { url: `file://${path.join(here, "scroll-anchoring-oracle.html")}` })
    await Bun.sleep(800)
    const result = await send("Runtime.evaluate", { expression: `window.__run(${JSON.stringify(scene)}, ${JSON.stringify(definition)})`, awaitPromise: true, returnByValue: true })
    socket.close()
    if (!result.result?.result || result.result.exceptionDetails) throw new Error(JSON.stringify(result))
    return result.result.result.value
  } finally {
    process_.kill()
  }
}

const file = path.join(here, "scroll-anchoring-oracle.json")
const named = process.argv.slice(2)
const found: Record<string, unknown> = named.length > 0 && fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : {}
for (const scene of named.length > 0 ? named : Object.keys(SCENES)) found[scene] = await run(scene)
fs.writeFileSync(file, JSON.stringify(found))
console.log(`wrote ${Object.keys(found).length} scenes`)
