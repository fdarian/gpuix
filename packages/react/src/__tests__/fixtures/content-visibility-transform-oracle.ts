// Regenerates content-visibility-transform-oracle.json from content-visibility-transform-oracle.html with headless Chrome:
//   bun content-visibility-transform-oracle.ts [case...]
// Cases named on the command line are run alone and replace their entries in the file. Each case runs in a page of its own.
import fs from "fs"
import path from "path"
import { CASES, PAGE } from "./content-visibility-transform-cases"

const here = import.meta.dir
const chrome =
  process.env.CHROME ??
  `${process.env.HOME}/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`

async function run(nodes: unknown) {
  const port = 9300 + Math.floor(Math.random() * 600)
  const profile = fs.mkdtempSync(path.join(process.env.TMPDIR ?? "/tmp", "cv-transform-"))
  const process_ = Bun.spawn(
    [chrome, `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, `--window-size=${PAGE.width},${PAGE.height}`, "--force-device-scale-factor=1", "--allow-file-access-from-files", "--no-sandbox", "about:blank"],
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
    await send("Emulation.setDeviceMetricsOverride", { ...PAGE, deviceScaleFactor: 1, mobile: false })
    await send("Page.navigate", { url: `file://${path.join(here, "content-visibility-transform-oracle.html")}` })
    await Bun.sleep(800)
    const result = await send("Runtime.evaluate", { expression: `window.__run(${JSON.stringify(nodes)})`, awaitPromise: true, returnByValue: true })
    socket.close()
    if (!result.result?.result || result.result.exceptionDetails) throw new Error(JSON.stringify(result))
    return result.result.result.value
  } finally {
    process_.kill()
    fs.rmSync(profile, { recursive: true, force: true })
  }
}

const only = process.argv.slice(2)
const file = path.join(here, "content-visibility-transform-oracle.json")
const found: Record<string, unknown> = only.length ? JSON.parse(fs.readFileSync(file, "utf8")) : {}
const cases = only.length ? CASES.filter((item) => only.includes(item.name)) : CASES
for (const item of cases) found[item.name] = await run(item.nodes)
fs.writeFileSync(file, JSON.stringify(found))
console.log(`wrote ${cases.length} cases`)
