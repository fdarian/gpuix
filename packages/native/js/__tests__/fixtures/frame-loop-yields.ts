// Spawned by frame-loop-yields.test.ts: runs the real frame loop against a hidden
// window, wakes it from a Rust thread, and prints how much JS work got through.
import { readFile } from "node:fs/promises"
import { GpuixRenderer } from "@ghia/native"
import { startFrameLoop } from "../../runtime.js"

const mode = process.argv[2]
if (mode !== "slow" && mode !== "fast") throw new Error(`unknown mode ${mode}`)
const runMs = mode === "slow" ? 1200 : 500
const wakeIntervalMs = 8

const renderer = new GpuixRenderer(() => {})
renderer.init({ show: false, focus: false })
if (mode === "slow") renderer.setTickDelay(25)

// The first tick is synchronous and includes window setup, so timing starts after it.
let startedAt = performance.now()
let ticks = 0
let firstTickDone = false
let backToBackTicks = 0
let tasksSinceTick = 0
let timeouts = 0
let maxTimeoutGapMs = 0
let intervals = 0
let reads = 0
let lastTimeoutAt = startedAt

const finish = () => {
  const now = performance.now()
  console.log(
    JSON.stringify({
      elapsedMs: now - startedAt,
      ticks,
      backToBackTicks,
      timeouts,
      maxTimeoutGapMs: Math.max(maxTimeoutGapMs, now - lastTimeoutAt),
      intervals,
      reads,
    }),
  )
  process.exit(0)
}

const host = {
  requiresTick: () => renderer.requiresTick(),
  setFrameWakeCallback: (callback: (() => void) | null) => renderer.setFrameWakeCallback(callback),
  tick: () => {
    if (firstTickDone && tasksSinceTick === 0) backToBackTicks += 1
    ticks += 1
    const running = renderer.tick()
    tasksSinceTick = 0
    if (performance.now() - startedAt >= runMs) finish()
    return running
  },
}

startFrameLoop(host)
startedAt = performance.now()
lastTimeoutAt = startedAt
ticks = 0
firstTickDone = true
renderer.simulateFrameWakes(wakeIntervalMs)

const timeoutChain = () => {
  const now = performance.now()
  maxTimeoutGapMs = Math.max(maxTimeoutGapMs, now - lastTimeoutAt)
  lastTimeoutAt = now
  timeouts += 1
  tasksSinceTick += 1
  setTimeout(timeoutChain, 0)
}
setTimeout(timeoutChain, 0)

setInterval(() => {
  intervals += 1
  tasksSinceTick += 1
}, 20)

while (true) {
  await readFile(new URL(import.meta.url))
  reads += 1
  tasksSinceTick += 1
}
