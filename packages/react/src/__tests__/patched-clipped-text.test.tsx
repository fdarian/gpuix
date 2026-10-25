// @ts-nocheck
/**
 * A patched animation keeps an operation for every glyph of the text it paints, so that its operations do not
 * depend on the clip, but rasterizes only the glyphs that show: a line of 300 different glyphs slid through a
 * clip 120 wide costs the glyphs that came into view, not the line (`getFrameWorkCounters().rasterizedGlyphs`).
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
const TICKS = 30
const GLYPHS = 300
const LINE = Array.from({ length: GLYPHS }, (_, index) => String.fromCodePoint(0x4e00 + index)).join("")

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

async function settle(renderer, frames = 12) {
  for (let frame = 0; frame < frames; frame++) {
    renderer.flush()
    renderer.dispatchNativeEvents()
    await sleep(8)
  }
}

defineKeyframes("cpatch-slide", { from: { translate: "0px 0px" }, to: { translate: "-1200px 0px" } })

const head = () => (
  <div style={{ width: 120, height: 24, overflow: "hidden" }}>
    <div
      testId="head"
      style={{ width: 6000, whiteSpace: "nowrap", fontSize: 16, color: "#222222", animation: "cpatch-slide 2s linear infinite" }}
    >
      {LINE}
    </div>
  </div>
)

describeNative("a patched animation of text a clip hides", { timeout: TIMEOUT }, () => {
  it("rasterizes the glyphs that come into the clip and not the ones that stay out", async () => {
    const root = createTestRoot({ width: 760, height: 620 })
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<Page head={head()} />)
      await settle(renderer)
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      const tick = () => {
        renderer.clockAdvance(16)
        renderer.drawFrame()
      }
      for (let warm = 0; warm < WARM; warm++) tick()
      const rasterizedAtStart = renderer.getFrameWorkCounters().rasterizedGlyphs
      const patches = renderer.getPatchCounters()
      for (let index = 0; index < TICKS; index++) tick()
      const after = renderer.getPatchCounters()
      const rasterized = renderer.getFrameWorkCounters().rasterizedGlyphs
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)

      // The line has 300 glyphs of its own and the clip shows about 8 at a time. The page paints some 30 glyphs
      // of its own, and the 30 ticks slide the line by a few dozen px: a handful of glyphs come into the clip.
      expect(rasterized, "the glyphs that came into the clip were rasterized").toBeGreaterThan(rasterizedAtStart)
      expect(rasterized - rasterizedAtStart, "and only those").toBeLessThan(40)
      expect(rasterized, `rasterized ${rasterized} of ${GLYPHS} glyphs and the page's`).toBeLessThan(GLYPHS / 2)
      if (!patching || verifying) return
      expect(after.patchedFrames - patches.patchedFrames).toBeGreaterThanOrEqual(TICKS - 1)
      expect(JSON.parse(after.declines)).toEqual(JSON.parse(patches.declines))
    } finally {
      root.root.unmount()
      renderer.dispose()
    }
  })
})
