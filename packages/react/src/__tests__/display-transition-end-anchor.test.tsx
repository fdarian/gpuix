// @ts-nocheck
// An element that is built as absent (its display transition just ended) does not take the anchored layer's scope
// with it: the scope is for the layer's direct child only, and the next element built must not resolve against it.
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("an anchored layer whose last child ends a display transition", () => {
  it("does not hand its anchor to the element built after it", () => {
    const page = (display?: "none") => (
      <div testId="trigger" style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", width: 140, height: 30 }}>
        <anchored gap={4}>
          <div
            testId="leaving"
            style={{ display, width: 20, height: 10, transition: "display 200ms linear allow-discrete" }}
          />
        </anchored>
        <div testId="after" style={{ width: "anchor-size(width)", height: 10 }} />
      </div>
    )
    const { render, renderer, unmount } = createTestRoot({ width: 400, height: 300 })
    try {
      renderer.clockPause()
      render(page())
      for (let i = 0; i < 4; i++) renderer.flush()
      renderer.setInlineDraws(false)
      const widths: number[] = []
      render(page("none"))
      for (let i = 0; i < 20; i++) {
        renderer.clockFastForward(16)
        renderer.drawFrame()
        widths.push(renderer.getElementBounds(renderer.findByTestId("after").id).width)
      }
      expect(new Set(widths).size).toBe(1)
    } finally {
      unmount()
    }
  })
})
