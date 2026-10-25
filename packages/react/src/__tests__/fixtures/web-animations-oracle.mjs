// Runs web-animations-oracle.html#api in a headless Chrome of its own, in real time, and prints what it resolves
// `apiOracle` to. The `--virtual-time-budget` run of the page cannot do this: it draws one frame and then none,
// so no event, no `ready` and no start time ever arrives there.
import { spawn } from "node:child_process"
import { mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"

const chromePath = process.argv[2]
const page = path.join(path.dirname(fileURLToPath(import.meta.url)), "web-animations-oracle.html")
const profile = mkdtempSync(path.join(tmpdir(), "waapi-oracle-"))
const chrome = spawn(
  chromePath,
  ["--headless", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--window-size=1280,800", "about:blank"],
  { stdio: ["ignore", "ignore", "pipe"] }
)
const finish = (code) => {
  chrome.once("exit", () => {
    rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 })
    process.exit(code)
  })
  chrome.kill()
}
setTimeout(() => {
  console.error("web-animations-oracle: gave up after 600s")
  finish(1)
}, 600_000)

const endpoint = await new Promise((resolve) => {
  let seen = ""
  chrome.stderr.on("data", (chunk) => {
    seen += chunk
    const match = /DevTools listening on (ws:\S+)/.exec(seen)
    if (match) resolve(match[1])
  })
})
const socket = new WebSocket(endpoint)
await new Promise((resolve) => socket.addEventListener("open", resolve))
let last = 0
const waiting = new Map()
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data)
  if (message.id && waiting.has(message.id)) {
    waiting.get(message.id)(message)
    waiting.delete(message.id)
  }
})
const send = (method, params, sessionId) =>
  new Promise((resolve, reject) => {
    const id = ++last
    waiting.set(id, (message) => (message.error ? reject(new Error(`${method}: ${JSON.stringify(message.error)}`)) : resolve(message.result)))
    socket.send(JSON.stringify({ id, method, params: params ?? {}, sessionId }))
  })

try {
  const target = await send("Target.createTarget", { url: "about:blank" })
  const attached = await send("Target.attachToTarget", { targetId: target.targetId, flatten: true })
  const session = attached.sessionId
  await send("Page.enable", {}, session)
  await send("Page.navigate", { url: `file://${page}#api` }, session)
  // The navigation is to a new document, so wait for it before asking for its promise.
  await new Promise((resolve) => setTimeout(resolve, 500))
  const result = await send(
    "Runtime.evaluate",
    { expression: "apiOracle", awaitPromise: true, returnByValue: true },
    session
  )
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails))
  process.stdout.write(result.result.value + "\n")
  finish(0)
} catch (error) {
  console.error(error)
  finish(1)
}
