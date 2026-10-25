// @ts-nocheck
/**
 * The scroll extents of a scroller are current in the frame of a tick that only repaints what moves in it.
 *
 * A CSS transition or a Motion tween of the translation of a child in a box moves what the box can scroll to. The tick is
 * patched (the child is built alone and painted again), and the box is not: gpui works its range out again for the
 * scrollers around a patched root, and gpuix publishes the `scrollWidth` of them. Every tick reads what a frame built whole
 * at the same instant gives, and the end of the run what Chrome gives (`fixtures/transform-geometry-oracle.json`, scenes
 * `e18` and `e19` of `extent`).
 *
 * A box that hides its overflow paints no scrollbar, so its ticks stay patched. One that scrolls may paint a thumb whose
 * length follows the range: its tick is drawn whole when the range changes.
 */

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const patching = process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined
const TIMEOUT = 180_000
const WARM = 3
const TICKS = 30
const STEP = 16

const ORACLE = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/transform-geometry-oracle.json"), "utf8")
)
/** Chrome's `scrollWidth` of the end of the run, by the overflow of the box. */
const CHROME_END = { hidden: ORACLE.extent.sizes.e18[6], auto: ORACLE.extent.sizes.e19[6] }

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

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

/** A box of the size of the oracle's, whose overflow is `overflow`, holding a child that is moved by `mover`. */
const box = (overflow: "hidden" | "auto", mover: React.ReactNode) => (
  <div testId="box" style={{ position: "relative", width: 120, height: 80, overflow, scrollbarWidth: "none" }}>
    {mover}
  </div>
)

const CHILD = { position: "absolute", left: 0, top: 0, width: 50, height: 30, backgroundColor: "#336699" } as const

const scenes = {
  "a CSS transition": (overflow, moving) =>
    box(
      overflow,
      <div testId="mover" style={{ ...CHILD, transition: "transform 1s linear", transform: moving ? "translateX(400px)" : "none" }} />
    ),
  "a Motion tween": (overflow, moving) =>
    box(
      overflow,
      <motion.div testId="mover" style={CHILD} initial={{ x: 0 }} animate={{ x: moving ? 400 : 0 }} transition={{ duration: 1, ease: "linear" }} />
    ),
}

function scrollSize(renderer) {
  const metrics = renderer.getScrollMetrics(renderer.findByTestId("box").id)
  expect(metrics, "the box has scroll metrics").not.toBeNull()
  return [metrics[2], metrics[3]]
}

async function settle(renderer, frames = 12) {
  for (let frame = 0; frame < frames; frame++) {
    renderer.flush()
    renderer.dispatchNativeEvents()
    await sleep(8)
  }
}

/** The scroll size a frame built whole at this instant gives; the patched state is put back by the ticks after it. */
function wholeSize(renderer) {
  renderer.setPatchEnabled(false)
  renderer.clockAdvance(0)
  renderer.drawFrame()
  const size = scrollSize(renderer)
  renderer.setPatchEnabled(true)
  return size
}

/** Ticks the transition, reading the scroll size against a frame built whole at the ticks `reads` says. */
async function run(name: string, overflow: "hidden" | "auto", reads: (tick: number) => boolean) {
  const root = createTestRoot({ width: 760, height: 620 })
  const renderer = root.renderer
  try {
    renderer.clockPause()
    root.render(<Page head={scenes[name](overflow, false)} />)
    await settle(renderer)
    renderer.setInlineDraws(false)
    renderer.setA11yActive(false)
    expect(scrollSize(renderer), "at rest").toEqual([120, 80])
    root.render(<Page head={scenes[name](overflow, true)} />)
    const tick = () => {
      renderer.clockAdvance(STEP)
      renderer.drawFrame()
    }
    for (let warm = 0; warm < WARM; warm++) tick()
    const before = renderer.getPatchCounters()
    const seen: number[] = []
    let after = before
    for (let index = 0; index < TICKS; index++) {
      tick()
      // The frame built whole for a read is not a tick.
      after = renderer.getPatchCounters()
      if (!reads(index)) continue
      const patched = scrollSize(renderer)
      expect(patched, `tick ${index}: the scroll size of the patched frame is the one of a frame built whole`).toEqual(wholeSize(renderer))
      seen.push(patched[0])
    }
    expect(seen.length, "the size was read").toBeGreaterThan(0)
    expect(seen[seen.length - 1], "the child moved out of the box").toBeGreaterThan(120)
    if (seen.length > 1) expect(new Set(seen).size, "the size follows the child").toBeGreaterThan(1)
    // The end of the run is a frame built whole, which Chrome's size is the end of.
    for (let index = 0; index < 80; index++) tick()
    await settle(renderer, 4)
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    expect(scrollSize(renderer)[0], "the end of the run is Chrome's").toBe(CHROME_END[overflow])
    return { patched: after.patchedFrames - before.patchedFrames, declines: after.declines, before: before.declines }
  } finally {
    root.root.unmount()
    renderer.dispose()
  }
}

describeNative("the scroll size of a box whose child a patched tick moves", { timeout: TIMEOUT }, () => {
  for (const name of Object.keys(scenes)) {
    for (const overflow of ["hidden", "auto"] as const) {
      // A read draws the tick after it whole, so the sizes of the patched frames are read every few ticks.
      it(`follows ${name} in a box with overflow ${overflow}`, async () => {
        const done = await run(name, overflow, (tick) => tick % 5 === 4)
        if (!patching || verifying || overflow === "auto") return
        expect(done.patched, "the ticks between the reads are patched").toBeGreaterThanOrEqual(TICKS / 2)
      })

      it(`patches the ticks of ${name} in a box with overflow ${overflow} that nothing reads, and follows the child at the end`, async () => {
        const done = await run(name, overflow, (tick) => tick === TICKS - 1)
        if (!patching || verifying) return
        if (overflow === "hidden") {
          expect(done.patched, "every tick is patched").toBeGreaterThanOrEqual(TICKS - 1)
          expect(done.declines).toEqual(done.before)
        } else {
          expect(JSON.parse(done.declines).ScrollExtent ?? 0, "a thumb that may change is drawn whole").toBeGreaterThan(0)
        }
      })
    }
  }
})
