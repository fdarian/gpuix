import { createTestRoot } from "@ghia/react/testing"
import { ChatApp } from "./chat"

const turnCount = Number(process.env.TURNS ?? "1000")
const root = createTestRoot()
const t = performance.now()
root.render(<ChatApp turnCount={turnCount} />)
const mountMs = +(performance.now() - t).toFixed(1)
Bun.gc(true)
const m = process.memoryUsage()
console.log(
  JSON.stringify({
    framework: "react",
    turnCount,
    mountMs,
    rssMB: +(m.rss / 1048576).toFixed(1),
    heapUsedMB: +(m.heapUsed / 1048576).toFixed(1),
  }),
)
root.unmount()
process.exit(0)
