import { describe, expect, it } from "vitest"
import { startFrameLoop } from "../runtime.js"

const wakeableRenderer = (tick: () => boolean) => {
  const renderer = {
    wake: null as (() => unknown) | null,
    requiresTick: () => true,
    setFrameWakeCallback: (callback?: (() => unknown) | null) => {
      renderer.wake = callback ?? null
      return true
    },
    tick,
  }
  return renderer
}

describe("startFrameLoop on a platform with display-link wakes", () => {
  it("ticks once at start and then on every wake", () => {
    let ticks = 0
    const renderer = wakeableRenderer(() => {
      ticks += 1
      return true
    })
    const loop = startFrameLoop(renderer)
    expect(ticks).toBe(1)
    renderer.wake?.()
    renderer.wake?.()
    expect(ticks).toBe(3)
    loop.stop()
    expect(renderer.wake).toBeNull()
  })

  it("falls back to a slow timer when no wakes arrive", async () => {
    let ticks = 0
    const loop = startFrameLoop(
      wakeableRenderer(() => {
        ticks += 1
        return true
      }),
    )
    await new Promise((resolve) => setTimeout(resolve, 250))
    loop.stop()
    expect(ticks).toBeGreaterThanOrEqual(2)
    expect(ticks).toBeLessThanOrEqual(4)
  })

  it("does not run the fallback while wakes keep arriving", async () => {
    let ticks = 0
    const renderer = wakeableRenderer(() => {
      ticks += 1
      return true
    })
    const loop = startFrameLoop(renderer)
    const wakes = setInterval(() => renderer.wake?.(), 10)
    await new Promise((resolve) => setTimeout(resolve, 250))
    clearInterval(wakes)
    loop.stop()
    const wakeCount = Math.floor(250 / 10)
    expect(ticks).toBeLessThanOrEqual(wakeCount + 2)
  })

  it("stops, clears the wake and reports termination when tick returns false", () => {
    let ticks = 0
    let terminated = 0
    const renderer = wakeableRenderer(() => {
      ticks += 1
      return false
    })
    startFrameLoop(renderer, { onTerminated: () => (terminated += 1) })
    expect(ticks).toBe(1)
    expect(terminated).toBe(1)
    expect(renderer.wake).toBeNull()
  })

  it("reports tick errors and keeps running", () => {
    const errors: unknown[] = []
    const renderer = wakeableRenderer(() => {
      throw new Error("boom")
    })
    const loop = startFrameLoop(renderer, { onError: (error) => errors.push(error) })
    renderer.wake?.()
    loop.stop()
    expect(errors.length).toBe(2)
  })

  it("keeps polling with a timer when frameMs is set", async () => {
    let ticks = 0
    const renderer = wakeableRenderer(() => {
      ticks += 1
      return true
    })
    const loop = startFrameLoop(renderer, { frameMs: 10 })
    await new Promise((resolve) => setTimeout(resolve, 25))
    loop.stop()
    expect(renderer.wake).toBeNull()
    expect(ticks).toBeGreaterThan(1)
    expect(ticks).toBeLessThanOrEqual(4)
  })
})

describe("startFrameLoop without display-link wakes", () => {
  it("polls with a timer when the renderer declines the wake callback", async () => {
    let ticks = 0
    const loop = startFrameLoop({
      requiresTick: () => true,
      setFrameWakeCallback: () => false,
      tick: () => {
        ticks += 1
        return true
      },
    })
    await new Promise((resolve) => setTimeout(resolve, 50))
    loop.stop()
    expect(ticks).toBeGreaterThan(2)
  })

  it("polls with a timer", async () => {
    let ticks = 0
    const loop = startFrameLoop(
      {
        requiresTick: () => true,
        tick: () => {
          ticks += 1
          return true
        },
      },
      { frameMs: 10 },
    )
    await new Promise((resolve) => setTimeout(resolve, 25))
    loop.stop()
    expect(ticks).toBeGreaterThan(1)
  })
})
