/**
 * A tick of a CSS transition, a CSS animation or a `motion` animation of `filter` is patched: the element that runs is
 * built alone and gpui paints its operations again (`getPatchCounters`), on a page of 400 other cells. `filter` is in
 * `AnimProp::paints_only`, so the animation is a unit; the filter stays an effect group from the first tick to the last
 * when both ends are lists, which is what a patch needs (an element that starts or stops being a group is lifted or not
 * by the frame that does it).
 */

import React from "react"
import { describe, expect, it } from "vitest"
import { defineKeyframes, motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const patching = process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined
const TIMEOUT = 180_000
const WARM = 12
const TICKS = 20
const BOX = { width: 100, height: 50 }

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

/** The page: the head above 100 rows of 4 bordered cells, the shape of the pages the other patch counters run on. */
function Page({ head }: { head: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: 700, height: 560, overflow: "hidden", backgroundColor: "#ffffff" }}>
      <div style={{ padding: 100 }}>{head}</div>
      {Array.from({ length: 100 }, (_, row) => (
        <div key={row} style={{ display: "flex", flexDirection: "row", gap: 4, padding: 2 }}>
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

async function settle(renderer, frames = 12) {
  for (let frame = 0; frame < frames; frame++) {
    renderer.flush()
    renderer.dispatchNativeEvents()
    await sleep(8)
  }
}

/** The red channel at `probe`: just outside the left edge of the head, which a blur darkens, or on the right of it, where a shadow falls. */
function edge(renderer, probe: [number, number]): number {
  const file = `${SHOTS_DIR}/filter-patch-${process.pid}.png`
  renderer.captureScreenshot(file)
  return readLogicalPixels(file, renderer).at(probe[0], probe[1])[0]!
}

/** What `TICKS` ticks did to the counters, after the warm-up that hands the page over to ticks. */
async function ticksOf(head: React.ReactNode, drive?: (root, renderer, tick: number) => void, probe: [number, number] = [97, 125]) {
  const root = createTestRoot({ width: 760, height: 620 })
  const renderer = root.renderer
  try {
    renderer.clockPause()
    root.render(<Page head={head} />)
    await settle(renderer)
    renderer.setInlineDraws(false)
    renderer.setA11yActive(false)
    const tick = (index: number) => {
      drive?.(root, renderer, index)
      renderer.clockAdvance(16)
      renderer.drawFrame()
    }
    const start = edge(renderer, probe)
    for (let warm = 0; warm < WARM; warm++) tick(warm)
    const patches = renderer.getPatchCounters()
    const painted = renderer.getPaintedFrameCount()
    const built = renderer.getBuildCounters()
    for (let index = 0; index < TICKS; index++) tick(WARM + index)
    const after = renderer.getPatchCounters()
    const end = edge(renderer, probe)
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    return {
      patched: after.patchedFrames - patches.patchedFrames,
      full: renderer.getPaintedFrameCount() - painted,
      declines: gained(after.declines, patches.declines),
      unpatchable: gained(after.unpatchable, patches.unpatchable),
      built: renderer.getBuildCounters().elementsBuilt - built.elementsBuilt,
      changed: start !== end,
    }
  } finally {
    root.root.unmount()
    renderer.dispose()
  }
}

function expectPatched(done: Awaited<ReturnType<typeof ticksOf>>) {
  expect(done.changed, "the filter changed what is painted").toBe(true)
  if (!patching || verifying) return
  expect(done.patched, JSON.stringify(done)).toBeGreaterThanOrEqual(TICKS - 1)
  expect(done.full).toBeLessThanOrEqual(1)
  expect(done.declines).toEqual({})
  expect(done.unpatchable).toEqual({})
  // Not the page: a few elements a tick, not the 400 cells.
  expect(done.built, JSON.stringify(done)).toBeLessThan(TICKS * 8)
}

defineKeyframes("fpatch-blur", { from: { filter: "blur(0px)" }, to: { filter: "blur(16px) brightness(1.5)" } })
defineKeyframes("fpatch-shadow", {
  from: { filter: "drop-shadow(0px 0px 0px rgba(0, 0, 0, 0))" },
  to: { filter: "drop-shadow(20px 20px 10px rgba(0, 0, 0, 1))" },
})

const SHADOW_PROBE: [number, number] = [208, 135]

const head = (style: object) => <div testId="head" style={{ ...BOX, backgroundColor: "#000000", ...style }} />

describeNative("a tick of an animation of filter", { timeout: TIMEOUT }, () => {
  it("patches every tick of a transition of blur", async () => {
    const style = { filter: "blur(0px)", transition: "filter 2s linear" }
    const done = await ticksOf(head(style), (root, _renderer, tick) => {
      if (tick === 0) root.render(<Page head={head({ ...style, filter: "blur(16px)" })} />)
    })
    expectPatched(done)
  })

  it("patches every tick of a transition from none, which makes the element an effect group at its first tick", async () => {
    const style = { transition: "filter 2s linear" }
    const done = await ticksOf(head(style), (root, _renderer, tick) => {
      if (tick === 0) root.render(<Page head={head({ ...style, filter: "blur(16px)" })} />)
    })
    expectPatched(done)
  })

  it("patches every tick of a transition of a list against a longer one", async () => {
    const style = { filter: "blur(0px)", transition: "filter 2s linear" }
    const done = await ticksOf(head(style), (root, _renderer, tick) => {
      if (tick === 0) root.render(<Page head={head({ ...style, filter: "blur(16px) brightness(1.5)" })} />)
    })
    expectPatched(done)
  })

  it("patches every tick of a transition of a drop-shadow", async () => {
    const style = { filter: "drop-shadow(0px 0px 0px rgba(0, 0, 0, 0))", transition: "filter 2s linear" }
    const done = await ticksOf(
      head(style),
      (root, _renderer, tick) => {
        if (tick === 0) root.render(<Page head={head({ ...style, filter: "drop-shadow(20px 20px 10px rgba(0, 0, 0, 1))" })} />)
      },
      SHADOW_PROBE
    )
    expectPatched(done)
  })

  it("patches every tick of an animation of blur between two lists", async () => {
    expectPatched(await ticksOf(head({ filter: "blur(0px)", animation: "fpatch-blur 1.6s linear infinite" })))
  })

  it("patches every tick of an animation of a drop-shadow", async () => {
    expectPatched(
      await ticksOf(
        head({ filter: "drop-shadow(0px 0px 0px rgba(0, 0, 0, 0))", animation: "fpatch-shadow 1.6s linear infinite" }),
        undefined,
        SHADOW_PROBE
      )
    )
  })

  it("patches every tick of a motion tween of blur", async () => {
    expectPatched(
      await ticksOf(
        <motion.div
          testId="head"
          style={{ ...BOX, backgroundColor: "#000000" }}
          initial={{ filter: "blur(0px)" }}
          animate={{ filter: "blur(16px)" }}
          transition={{ duration: 2, ease: "linear" }}
        />
      )
    )
  })

  it("patches every tick of a motion spring of blur with a scale", async () => {
    expectPatched(
      await ticksOf(
        <motion.div
          testId="head"
          style={{ ...BOX, backgroundColor: "#000000" }}
          initial={{ scale: 0.85, filter: "blur(16px)" }}
          animate={{ scale: 1, filter: "blur(0px)" }}
          transition={{ type: "spring", stiffness: 20, damping: 4 }}
        />
      )
    )
  })
})
