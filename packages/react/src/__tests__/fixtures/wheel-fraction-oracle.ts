// Regenerates wheel-fraction-oracle.json (device scale 1) and wheel-fraction-oracle-x2.json (scale 2) with headless Chrome:
//   bun wheel-fraction-oracle.ts
// A page scroller holds a scroller scrolled to its end by wheel events over it, then more wheel events (CDP
// `Input.dispatchMouseEvent` `mouseWheel`, one every 650 ms so that each is a gesture of its own and not latched to the
// scroller under the pointer) of the deltas of a case, over the content or over the scrollbar track. Each row is
// [deltaY, page scrollTop, inner scrollTop] read after the event. A case is keyed `<rows> <over> <deltas>`.
import fs from "fs"
import path from "path"

const here = import.meta.dir
const chrome =
  process.env.CHROME ??
  `${process.env.HOME}/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`
const GAP_MS = 650
const DELTAS = [[40], [2.5], [0.5, 1, 1.5, 2, 3, 0.7]]
const ROWS = [28, 29]
const OVER = ["content", "track"]

const html = (rows: number) =>
  `<!doctype html><body style="margin:0"><div id=page style="box-sizing:border-box;width:400px;height:400px;overflow-y:scroll;padding:20px"><div id=inner style="box-sizing:border-box;width:300px;height:163px;overflow-y:scroll"><div style="padding:8px">${Array.from({ length: rows }, () => `<div style="height:19.5px"></div>`).join("")}</div></div><div style="height:3000px"></div></div></body>`

async function run(scale: number) {
  const port = 9300 + Math.floor(Math.random() * 600)
  const process_ = Bun.spawn(
    [chrome, `--remote-debugging-port=${port}`, "--window-size=500,500", `--force-device-scale-factor=${scale}`, "--no-sandbox", "about:blank"],
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
    const evaluate = async (expression: string) => {
      const result = await send("Runtime.evaluate", { expression, returnByValue: true })
      if (result.result.exceptionDetails) throw new Error(JSON.stringify(result))
      return result.result.result.value
    }
    await send("Page.enable")
    const read = (): Promise<number[]> =>
      evaluate(`(() => { const inner = document.getElementById('inner'); return [document.getElementById('page').scrollTop, inner.scrollTop, inner.scrollHeight, inner.clientHeight] })()`)
    const wheel = async (x: number, y: number, deltaY: number) => {
      await send("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY })
      await Bun.sleep(GAP_MS)
    }

    const cases: Record<string, unknown> = {}
    for (const rows of ROWS) {
      for (const over of OVER) {
        for (const deltas of DELTAS) {
          const events = deltas.length > 1 ? 40 : deltas[0] === 40 ? 6 : 24
          await send("Page.navigate", { url: "about:blank" })
          await send("Page.navigate", { url: "data:text/html," + encodeURIComponent(html(rows)) })
          await Bun.sleep(500)
          const pointer = over === "track" ? { x: 316, y: 100 } : { x: 170, y: 100 }
          await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: pointer.x, y: pointer.y })
          await Bun.sleep(100)
          for (let tick = 0; tick < 100 && (await read())[0] === 0; tick++) await wheel(70, 70, 40)
          const start = await read()
          const steps: number[][] = []
          for (let tick = 0; tick < events; tick++) {
            const deltaY = deltas[tick % deltas.length] as number
            await wheel(pointer.x, pointer.y, deltaY)
            const now = await read()
            steps.push([deltaY, now[0] as number, now[1] as number])
          }
          cases[`${rows} ${over} ${deltas.join(",")}`] = { start, steps }
        }
      }
    }
    socket.close()
    return cases
  } finally {
    process_.kill()
  }
}

for (const [scale, file] of [[1, "wheel-fraction-oracle.json"], [2, "wheel-fraction-oracle-x2.json"]] as const) {
  fs.writeFileSync(path.join(here, file), JSON.stringify(await run(scale), null, 1) + "\n")
}
