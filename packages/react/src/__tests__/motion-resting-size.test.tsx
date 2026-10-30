// @ts-nocheck
/**
 * A motion whose layout keys (`width`, `height`) rest while others glide: the glide is patched.
 *
 * - A `motion.div` that glides by `x` and `y` and also names `width` and `height` as motion keys that never change
 *   is the element of a highlight that follows the pointer. Its resting size tracks move nothing, so every tick of
 *   a glide but the first is patched, as without the keys (`getPatchCounters`).
 * - A size that changes during the glide moves a box again: those ticks are built whole and counted as
 *   `LayoutAnimation`, and the box ends at the size asked for.
 */

import React from "react"
import { expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot } from "../testing"
import { describeNative, NATIVE_TEST_TIMEOUT, verifying } from "./retained-harness.js"

const patching = process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined
const TICKS = 24
const HOPS = 4
const SIZE = { width: 216, height: 36 }
const SPRING = { type: "spring", stiffness: 300, damping: 30 } as const

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

function Page({ target, sized }: { target: Record<string, number>; sized: boolean }) {
  return (
    <div style={{ position: "relative", width: 700, height: 560, overflow: "hidden" }}>
      <motion.div
        testId="head"
        style={{ position: "absolute", top: 0, left: 0, backgroundColor: "#336699", ...(sized ? {} : SIZE) }}
        initial={sized ? { x: 0, y: 0, ...SIZE } : { x: 0, y: 0 }}
        animate={sized ? { ...SIZE, ...target } : target}
        transition={SPRING}
      />
      {Array.from({ length: 100 }, (_, row) => (
        <div key={row} style={{ display: "flex", flexDirection: "row", gap: 4, padding: 2, marginTop: row === 0 ? 40 : 0 }}>
          {Array.from({ length: 4 }, (_, cell) => (
            <div key={cell} style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
              <text style={{ fontSize: 11, color: "#333333" }}>{`r${row} c${cell}`}</text>
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

function gained(after: string, before: string): Record<string, number> {
  const was = JSON.parse(before) as Record<string, number>
  const out: Record<string, number> = {}
  for (const [reason, count] of Object.entries(JSON.parse(after) as Record<string, number>)) {
    if (count > (was[reason] ?? 0)) out[reason] = count - (was[reason] ?? 0)
  }
  return out
}

type Hop = { x: number; y: number; width?: number; height?: number }

/**
 * Glides through `hops` and counts what each tick drew. `read` is a layout read between the change and the first
 * tick of a hop, the `getBoundingClientRect` of a highlight.
 */
async function glide(options: { sized: boolean; hops: Hop[]; read?: boolean }) {
  const root = createTestRoot({ width: 760, height: 620 })
  const renderer = root.renderer
  const hopsDone: { patched: number; full: number; first: "patched" | "full"; unpatchable: Record<string, number> }[] = []
  try {
    renderer.clockPause()
    root.render(<Page target={{ x: 0, y: 0 }} sized={options.sized} />)
    for (let frame = 0; frame < 12; frame++) {
      renderer.flush()
      renderer.dispatchNativeEvents()
      await sleep(8)
    }
    renderer.setInlineDraws(false)
    renderer.setA11yActive(false)
    for (let warm = 0; warm < 3; warm++) {
      renderer.clockAdvance(16)
      renderer.drawFrame()
    }
    for (const hop of options.hops) {
      root.render(<Page target={hop} sized={options.sized} />)
      if (options.read) renderer.getElementBounds(renderer.findByTestId("head").id)
      const before = renderer.getPatchCounters()
      const painted = renderer.getPaintedFrameCount()
      let first: "patched" | "full" = "full"
      for (let tick = 0; tick < TICKS; tick++) {
        const patchedBefore = renderer.getPatchCounters().patchedFrames
        renderer.clockAdvance(16)
        renderer.drawFrame()
        if (tick === 0 && renderer.getPatchCounters().patchedFrames > patchedBefore) first = "patched"
      }
      const after = renderer.getPatchCounters()
      hopsDone.push({
        patched: after.patchedFrames - before.patchedFrames,
        full: renderer.getPaintedFrameCount() - painted,
        first,
        unpatchable: gained(after.unpatchable, before.unpatchable),
      })
      // The window sits idle between hops, as a pointer that stops does.
      for (let rest = 0; rest < 6; rest++) {
        renderer.clockAdvance(500)
        renderer.drawFrame()
      }
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    }
    const box = renderer.getElementBounds(renderer.findByTestId("head").id)
    return { hops: hopsDone, box }
  } finally {
    root.unmount()
  }
}

const HOPS_OF_X: Hop[] = Array.from({ length: HOPS }, (_, hop) => ({ x: 120 * (hop + 1), y: 40 * (hop % 2 === 0 ? 1 : 3) }))

describeNative("a glide of a motion with resting width and height keys", { timeout: NATIVE_TEST_TIMEOUT * 3 }, () => {
  it("patches every tick after the first, as without the keys", async () => {
    const without = await glide({ sized: false, hops: HOPS_OF_X })
    const sized = await glide({ sized: true, hops: HOPS_OF_X })
    for (const [name, done] of [["without keys", without], ["resting keys", sized]] as const) {
      expect(done.box.width, name).toBe(SIZE.width)
      expect(done.box.height, name).toBe(SIZE.height)
      if (!patching || verifying) continue
      const what = `${name}: ${JSON.stringify(done.hops)}`
      for (const [index, hop] of done.hops.entries()) {
        // The first tick of the first hop starts the element, which nothing has built as a unit yet.
        expect(hop.unpatchable, what).toEqual(index === 0 ? hop.unpatchable : {})
        expect(hop.patched, what).toBeGreaterThanOrEqual(TICKS - 2)
        expect(hop.full, what).toBeLessThanOrEqual(2)
      }
    }
  })

  it("glides the same with a layout read between the change and the first frame", async () => {
    for (const read of [false, true]) {
      const done = await glide({ sized: true, hops: HOPS_OF_X, read })
      if (!patching || verifying) continue
      const what = `read ${read}: ${JSON.stringify(done.hops)}`
      for (const hop of done.hops.slice(1)) {
        expect(hop.unpatchable, what).toEqual({})
        expect(hop.patched, what).toBeGreaterThanOrEqual(TICKS - 2)
        expect(hop.full, what).toBeLessThanOrEqual(1)
      }
    }
  })

  it("starts at the frame that shows it when a layout read came first, after the window sat idle", async () => {
    const root = createTestRoot({ width: 760, height: 620 })
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<Page target={{ x: 0, y: 0 }} sized />)
      for (let frame = 0; frame < 12; frame++) {
        renderer.flush()
        renderer.dispatchNativeEvents()
        await sleep(8)
      }
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      renderer.clockAdvance(16)
      renderer.drawFrame()
      // The last draw is seconds old when the change comes.
      renderer.clockAdvance(5000)
      root.render(<Page target={{ x: 300, y: 0 }} sized />)
      const x = () => renderer.getElementBounds(renderer.findByTestId("head").id).x
      expect(x()).toBe(0)
      renderer.clockAdvance(16)
      renderer.drawFrame()
      const first = x()
      expect(first, "one frame into the glide").toBeGreaterThan(0)
      expect(first, "one frame into the glide").toBeLessThan(100)
      renderer.clockAdvance(16)
      renderer.drawFrame()
      expect(x()).toBeGreaterThan(first)
    } finally {
      root.unmount()
    }
  })

  it("is built whole while a size changes during the glide, and ends at the size", async () => {
    const hops: Hop[] = [
      { x: 200, y: 40 },
      { x: 400, y: 80, width: 300, height: 60 },
    ]
    const done = await glide({ sized: true, hops: hops.map((hop) => ({ ...hop })) })
    expect(done.box.width).toBe(300)
    expect(done.box.height).toBe(60)
    if (!patching || verifying) return
    expect(done.hops[0].unpatchable.LayoutAnimation, JSON.stringify(done.hops)).toBeUndefined()
    expect(done.hops[1].unpatchable.LayoutAnimation ?? 0).toBeGreaterThan(0)
  })
})
