import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import * as native from "@ghia/native"
import { describe, expect, it } from "vitest"

const nativePackageDir = fileURLToPath(new URL("../..", import.meta.url))
const fixture = fileURLToPath(new URL("./fixtures/frame-loop-yields.ts", import.meta.url))

const hasWakeSimulator =
  process.platform === "darwin" &&
  typeof native.GpuixRenderer?.prototype?.simulateFrameWakes === "function"

interface FrameLoopRun {
  elapsedMs: number
  ticks: number
  backToBackTicks: number
  timeouts: number
  maxTimeoutGapMs: number
  intervals: number
  reads: number
}

function runFixture(mode: "slow" | "fast"): FrameLoopRun {
  const result = spawnSync("bun", [fixture, mode], {
    cwd: nativePackageDir,
    encoding: "utf8",
    timeout: 15_000,
  })
  expect(result.status, result.stderr || result.error?.message).toBe(0)
  return JSON.parse(result.stdout.trim())
}

describe.skipIf(!hasWakeSimulator)("the frame loop under display-link wakes", () => {
  it("lets timers and I/O run between frames that take longer than a wake interval", () => {
    const run = runFixture("slow")
    console.log("slow frames", JSON.stringify(run))
    // The old loop gives ~1200 ms gaps and back-to-back on nearly every tick; the margins absorb a heavily loaded machine.
    expect(run.maxTimeoutGapMs).toBeLessThan(150)
    expect(run.intervals).toBeGreaterThanOrEqual(15)
    expect(run.reads).toBeGreaterThanOrEqual(5)
    expect(run.ticks).toBeGreaterThanOrEqual(15)
    expect(run.backToBackTicks).toBeLessThanOrEqual(1)
  }, 20_000)

  it("keeps up with the wakes when frames are fast", () => {
    const run = runFixture("fast")
    console.log("fast frames", JSON.stringify(run))
    expect(run.ticks).toBeGreaterThanOrEqual((0.75 * run.elapsedMs) / 8)
    expect(run.timeouts).toBeGreaterThan(0)
    expect(run.intervals).toBeGreaterThan(0)
  }, 20_000)
})
