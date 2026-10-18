// @ts-nocheck
// The frame in which a `display ... allow-discrete` transition ends builds its element with no box, as every frame
// after it does. Built as an empty block it is a flex item, and the gap around it shows for one frame.
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("a display transition that ends in a flex parent with a gap", () => {
  for (const planning of [true, false]) {
    it(`takes the gap with the box in one frame (retention planning: ${planning})`, () => {
      const page = (display?: "none") => (
        <div style={{ display: "flex", flexDirection: "column", gap: 10, width: 100 }}>
          <div testId="card" style={{ display, width: 100, height: 20, transition: "display 200ms linear allow-discrete" }} />
          <div testId="below" style={{ width: 100, height: 20 }} />
        </div>
      )
      const { render, renderer, unmount } = createTestRoot()
      try {
        renderer.setRetainPlanning(planning)
        renderer.clockPause()
        render(page())
        for (let i = 0; i < 4; i++) renderer.flush()
        // Frames as a window presents them, not the inline draws of the test renderer.
        renderer.setInlineDraws(false)
        const ys: number[] = []
        render(page("none"))
        for (let i = 0; i < 20; i++) {
          renderer.clockFastForward(16)
          renderer.drawFrame()
          ys.push(renderer.getElementBounds(renderer.findByTestId("below").id).y)
        }
        expect(ys[0]).toBe(30)
        expect(ys.at(-1)).toBe(0)
        expect(new Set(ys)).toEqual(new Set([30, 0]))
        expect(ys.indexOf(0)).toBeGreaterThan(5)
      } finally {
        unmount()
      }
    })
  }
})
