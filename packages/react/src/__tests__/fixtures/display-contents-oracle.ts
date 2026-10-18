// Drives headless Chrome over CDP: the DOM facts display-contents-oracle.html collects, the
// accessibility tree, and the Tab order. Prints the oracle JSON.
const chrome =
  process.env.CHROME ??
  `${process.env.HOME}/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`
const port = 9400 + Math.floor(Math.random() * 500)
const process_ = Bun.spawn([chrome, "--headless", "--window-size=1280,800", `--remote-debugging-port=${port}`, "about:blank"], {
  stdout: "ignore",
  stderr: "ignore",
})

async function connect() {
  for (let attempt = 0; attempt < 50; attempt++) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json()
      return new WebSocket(targets.find((target: { type: string }) => target.type === "page").webSocketDebuggerUrl)
    } catch {
      await Bun.sleep(100)
    }
  }
  throw new Error("Chrome did not start")
}

const socket = await connect()
await new Promise((resolve) => (socket.onopen = resolve))
let next = 0
const pending = new Map<number, (message: { result: any }) => void>()
socket.onmessage = (message) => {
  const data = JSON.parse(message.data as string)
  pending.get(data.id)?.(data)
}
const send = (method: string, params: object = {}) =>
  new Promise<any>((resolve) => {
    const id = ++next
    pending.set(id, (message) => resolve(message.result))
    socket.send(JSON.stringify({ id, method, params }))
  })
const evaluate = async (expression: string) => (await send("Runtime.evaluate", { expression, returnByValue: true })).result.value

const url = `file://${import.meta.dir}/display-contents-oracle.html`
await send("Page.enable")
await send("Page.navigate", { url })
for (let attempt = 0; attempt < 50; attempt++) {
  await Bun.sleep(100)
  if (await evaluate('document.documentElement.hasAttribute("data-oracle")')) break
}
const dom = JSON.parse(await evaluate('document.documentElement.getAttribute("data-oracle")'))

await send("Accessibility.enable")
const tree = await send("Accessibility.getFullAXTree")
const ax = tree.nodes
  .filter((node: any) => !node.ignored && node.name?.value && !["StaticText", "InlineTextBox", "RootWebArea"].includes(node.role.value))
  .map((node: any) => node.name.value)

// A fresh page: Tab starts from the top, not from the element focused above.
await send("Page.navigate", { url: `${url}?tab` })
await Bun.sleep(500)
const tab: string[] = []
for (let press = 0; press < 5; press++) {
  for (const type of ["rawKeyDown", "keyUp"]) {
    await send("Input.dispatchKeyEvent", { type, key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 })
  }
  await Bun.sleep(50)
  tab.push(await evaluate('document.activeElement.getAttribute("aria-label") ?? document.activeElement.tagName'))
}

console.log(JSON.stringify({ ...dom, ax, tab }, null, 2))
process_.kill()
process.exit(0)
