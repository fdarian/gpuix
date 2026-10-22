// Regenerates content-visibility-observers-oracle.json from content-visibility-observers-oracle.html with headless Chrome:
//   bun content-visibility-observers-oracle.ts [scene...]
// Scenes named on the command line are run alone and replace their entries in the file. Each scene runs in a page of its
// own, in a Chrome of its own.
import fs from "fs"
import path from "path"

const here = import.meta.dir
const chrome =
  process.env.CHROME ??
  `${process.env.HOME}/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`
const SIZE = { width: 1280, height: 800 }
const page = path.join(here, "content-visibility-observers-oracle.html")
const SCENES = [...fs.readFileSync(page, "utf8").matchAll(/^  ((?:ro|io)\d\w*): \{/gm)].map((match) => match[1]!)

async function run(scene: string) {
  const port = 9300 + Math.floor(Math.random() * 600)
  const process_ = Bun.spawn(
    [chrome, `--remote-debugging-port=${port}`, `--window-size=${SIZE.width},${SIZE.height}`, "--force-device-scale-factor=1", "--allow-file-access-from-files", "--no-sandbox", "about:blank"],
    { stdout: "ignore", stderr: "ignore" }
  )
  try {
    let target: { webSocketDebuggerUrl: string } | undefined
    for (let attempt = 0; attempt < 50 && !target; attempt++) {
      try {
        const targets = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()) as { type: string; webSocketDebuggerUrl: string }[]
        target = targets.find((candidate) => candidate.type === "page")
      } catch {}
      if (!target) await Bun.sleep(100)
    }
    if (!target) throw new Error("Chrome did not start")
    const socket = new WebSocket(target.webSocketDebuggerUrl)
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
    await send("Page.navigate", { url: `file://${page}#${scene}` })
    for (let attempt = 0; attempt < 200; attempt++) {
      const result = await send("Runtime.evaluate", { expression: "document.body && document.body.dataset.log || ''", returnByValue: true })
      const log = result.result?.result?.value
      if (log) {
        socket.close()
        return JSON.parse(log)
      }
      await Bun.sleep(50)
    }
    throw new Error(`${scene} did not finish`)
  } finally {
    process_.kill()
  }
}

const only = process.argv.slice(2)
const file = path.join(here, "content-visibility-observers-oracle.json")
const found: Record<string, unknown> = only.length ? JSON.parse(fs.readFileSync(file, "utf8")) : {}
for (const scene of only.length ? only : SCENES) found[scene] = await run(scene)
fs.writeFileSync(file, JSON.stringify(found))
console.log(`wrote ${only.length || SCENES.length} scenes`)
