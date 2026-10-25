// @ts-nocheck
/**
 * A tick of a CSS transition, a CSS animation or a Web Animations seek of `transform`, `translate`, `rotate` and `scale`
 * is patched: the element that runs is built alone and gpui paints its operations again (`getPatchCounters`).
 */

import React from "react"
import { describe, expect, it } from "vitest"
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const patching = process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined
const TIMEOUT = 180_000
const WARM = 12
const TICKS = 20
const BOX = { width: 100, height: 50 }

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

function bounds(renderer, testId: string): [number, number, number, number] {
  const box = renderer.getElementBounds(renderer.findByTestId(testId).id)
  expect(box, `${testId} has a box`).not.toBeNull()
  return [box.x, box.y, box.width, box.height]
}

/** The page: the head above 100 rows of 4 bordered cells, the shape of the pages the other patch counters run on. */
function Page({ head }: { head: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: 700, height: 560, overflow: "hidden" }}>
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

/** What `TICKS` ticks did to the counters, after the warm-up that hands the page over to ticks. */
async function ticksOf(head: React.ReactNode, drive?: (root, renderer, tick: number) => void) {
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
    const start = bounds(renderer, "head")
    for (let warm = 0; warm < WARM; warm++) tick(warm)
    const patches = renderer.getPatchCounters()
    const painted = renderer.getPaintedFrameCount()
    for (let index = 0; index < TICKS; index++) tick(WARM + index)
    const after = renderer.getPatchCounters()
    const end = bounds(renderer, "head")
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    return {
      patched: after.patchedFrames - patches.patchedFrames,
      full: renderer.getPaintedFrameCount() - painted,
      declines: gained(after.declines, patches.declines),
      unpatchable: gained(after.unpatchable, patches.unpatchable),
      moved: start.join() !== end.join(),
    }
  } finally {
    root.root.unmount()
    renderer.dispose()
  }
}

function expectPatched(done: Awaited<ReturnType<typeof ticksOf>>) {
  expect(done.moved, "the element moved").toBe(true)
  if (!patching || verifying) return
  expect(done.patched, JSON.stringify(done)).toBeGreaterThanOrEqual(TICKS - 1)
  expect(done.full).toBeLessThanOrEqual(1)
  expect(done.declines).toEqual({})
  expect(done.unpatchable).toEqual({})
}

defineKeyframes("tpatch-spin", { from: { rotate: "0deg" }, to: { rotate: "360deg" } })
defineKeyframes("tpatch-slide", { from: { translate: "0px 0px" }, to: { translate: "400px 20%" } })
defineKeyframes("tpatch-mismatch", {
  from: { transform: "rotate(0deg) translate(0px)" },
  "50%": { transform: "translate(200px) rotate(90deg)" },
  to: { transform: "none" },
})
defineKeyframes("tpatch-var", { from: { transform: "translateX(0px)" }, to: { transform: "translateX(calc(300% * var(--p)))" } })
defineKeyframes("tpatch-origin", {
  from: { transformOrigin: "0 0" },
  to: { transformOrigin: "100% 100%" },
})

const head = (style: object) => <div testId="head" style={{ ...BOX, backgroundColor: "#336699", ...style }} />

describeNative("a tick of a CSS animation or transition of the transform", { timeout: TIMEOUT }, () => {
  it("patches every tick of a transition of transform", async () => {
    const done = await ticksOf(head({ transition: "transform 2s linear" }), (root, _renderer, tick) => {
      if (tick === 0) {
        root.render(<Page head={head({ transition: "transform 2s linear", transform: "translateX(400px) rotate(90deg)" })} />)
      }
    })
    expectPatched(done)
  })

  it("patches every tick of a transition of a transform that reads a custom property of its own", async () => {
    const style = { transition: "transform 2s linear", transform: "translateX(calc(100% * var(--p)))" }
    const done = await ticksOf(head({ ...style, "--p": 0 }), (root, _renderer, tick) => {
      if (tick === 0) root.render(<Page head={head({ ...style, "--p": 4 })} />)
    })
    expectPatched(done)
  })

  it("patches every tick of a transition of a transform that reads a custom property of an ancestor", async () => {
    const style = { transition: "rotate 2s linear", rotate: "calc(var(--a) * 1)" }
    const above = (a: string) => (
      <div style={{ "--a": a }}>
        {head(style)}
      </div>
    )
    const done = await ticksOf(above("0deg"), (root, _renderer, tick) => {
      if (tick === 0) root.render(<Page head={above("200deg")} />)
    })
    expectPatched(done)
  })

  it("patches every tick of an animation whose keyframes read a custom property", async () => {
    expectPatched(await ticksOf(head({ "--p": 1, animation: "tpatch-var 1.6s linear infinite" })))
  })

  it("patches every tick of a transition of rotate, translate and scale together", async () => {
    const style = { transition: "rotate 2s linear, translate 2s linear, scale 2s linear" }
    const done = await ticksOf(head(style), (root, _renderer, tick) => {
      if (tick === 0) root.render(<Page head={head({ ...style, rotate: "120deg", translate: "300px 10px", scale: 1.5 })} />)
    })
    expectPatched(done)
  })

  it("patches every tick of an infinite animation of rotate", async () => {
    expectPatched(await ticksOf(head({ animation: "tpatch-spin 1.6s linear infinite" })))
  })

  it("patches every tick of an animation of lists that do not pair", async () => {
    expectPatched(await ticksOf(head({ animation: "tpatch-mismatch 1.6s linear infinite" })))
  })

  it("patches every tick of an animation of transform-origin around a transform", async () => {
    expectPatched(await ticksOf(head({ transform: "rotate(30deg)", animation: "tpatch-origin 1.6s linear infinite" })))
  })

  it("patches every tick of a translate that Web Animations paused, seeked and played", async () => {
    // A command is a write the window draws whole (the boxes may have changed as far as it knows); the ticks
    // after it, which the clock alone moves, are patched.
    const done = await ticksOf(head({ animation: "tpatch-slide 2s linear infinite" }), (_root, renderer, tick) => {
      const animations = renderer.getAnimations(renderer.findByTestId("head").id)
      if (tick === 0) renderer.pauseAnimation(animations[0].id)
      if (tick === 4) renderer.setAnimationCurrentTime(animations[0].id, 300)
      if (tick === 6) renderer.playAnimation(animations[0].id)
    })
    expectPatched(done)
  })
})
