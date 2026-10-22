// Regenerates content-visibility-a11y-oracle.json from content-visibility-a11y-oracle.html with headless Chrome:
//   bun content-visibility-a11y-oracle.ts
// The tab walks start at the last button the page builds and press the keys the test presses. The `ax` entry is the
// accessibility tree of the page with a screen reader on (`--force-renderer-accessibility`): the headings and buttons that
// are in it, by name.
import fs from "fs"
import path from "path"

const here = import.meta.dir
const chrome =
  process.env.CHROME ??
  `${process.env.HOME}/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`
const SIZE = { width: 900, height: 780 }
const LAST_BUILT = 19

const SCENES: Record<string, { hidden: number[]; keys: string[] }> = {
  forward: { hidden: [], keys: ["Tab", "Tab", "Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab"] },
  hidden: { hidden: [LAST_BUILT + 1, LAST_BUILT + 2], keys: ["Tab", "Tab", "Shift+Tab", "Shift+Tab"] },
}

async function open(query: string, flags: string[] = []) {
  const port = 9300 + Math.floor(Math.random() * 600)
  const process_ = Bun.spawn(
    [chrome, `--remote-debugging-port=${port}`, `--window-size=${SIZE.width},${SIZE.height}`, "--force-device-scale-factor=1", "--allow-file-access-from-files", "--no-sandbox", ...flags, "about:blank"],
    { stdout: "ignore", stderr: "ignore" }
  )
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
  await send("Emulation.setFocusEmulationEnabled", { enabled: true })
  await send("Page.navigate", { url: `file://${path.join(here, "content-visibility-a11y-oracle.html")}${query}` })
  await Bun.sleep(800)
  const evaluate = async (expression: string) => {
    const result = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })
    if (!result.result?.result || result.result.exceptionDetails) throw new Error(JSON.stringify(result))
    return result.result.result.value
  }
  return { send, evaluate, close: () => (socket.close(), process_.kill()) }
}

async function press(session: Awaited<ReturnType<typeof open>>, key: string) {
  const shift = key.startsWith("Shift+")
  const modifiers = shift ? 8 : 0
  const event = { key: "Tab", code: "Tab", windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9, modifiers }
  await session.send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...event })
  await session.send("Input.dispatchKeyEvent", { type: "keyUp", ...event })
}

const found: Record<string, unknown> = {}
for (const [name, scene] of Object.entries(SCENES)) {
  const session = await open(scene.hidden.length ? `?hidden=${scene.hidden.join(",")}` : "")
  try {
    await session.evaluate("window.__ready")
    await session.evaluate(`window.__focus('btn-${LAST_BUILT}')`)
    const steps = [{ step: `focused btn-${LAST_BUILT}`, ...(await session.evaluate("window.__state()")) }]
    for (const key of scene.keys) {
      await press(session, key)
      steps.push({ step: key, ...(await session.evaluate("window.__state()")) })
    }
    found[name] = steps.map((step) => ({ ...step, focused: step.focused?.replace(/^btn-/, "btn-") ?? null }))
  } finally {
    session.close()
  }
}

const ax = await open("?hidden=31", ["--force-renderer-accessibility"])
try {
  await ax.evaluate("window.__ready")
  await ax.send("Accessibility.enable")
  const tree = await ax.send("Accessibility.getFullAXTree")
  const nodes: { role?: { value: string }; name?: { value: string }; ignored?: boolean }[] = tree.result.nodes
  const named = (role: string) => nodes.filter((node) => !node.ignored && node.role?.value === role).map((node) => node.name?.value ?? "")
  found.ax = { headings: named("heading"), buttons: named("button") }
} finally {
  ax.close()
}

fs.writeFileSync(path.join(here, "content-visibility-a11y-oracle.json"), JSON.stringify(found, null, 1))
console.log(`wrote ${Object.keys(found).length} entries`)
