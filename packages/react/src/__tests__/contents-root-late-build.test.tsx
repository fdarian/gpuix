// @ts-nocheck
// A card spliced out of display:contents wrappers under a display:contents root is built late by gpui
// (GpuixView::build_subtree); it must still get the wrappers' inherited style.
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function page(tick: number) {
  return (
    <div style={{ display: "contents" }}>
      <div style={{ display: "contents", color: "#ff0000", fontSize: 30 }}>
        <div testId="card" style={{ width: 200 }}>
          <text testId="label">hi there</text>
        </div>
      </div>
      <div testId="tick" style={{ width: 10 + tick, height: 10 }} />
    </div>
  )
}

function run(planning: boolean, prod: boolean) {
  const kit = createTestRoot({ width: 400, height: 300 })
  const { renderer, render } = kit
  const log: string[] = []
  try {
    renderer.setRetainPlanning(planning)
    if (prod) renderer.setInlineDraws(false)
    const frame = () => (prod ? renderer.drawFrame() : renderer.flush())
    render(page(0))
    for (let i = 0; i < 4; i++) frame()
    const card = () => renderer.getElementBounds(renderer.findByTestId("card").id)
    log.push(`start card ${JSON.stringify(card())}`)
    for (let i = 1; i <= 4; i++) {
      render(page(i))
      frame()
      log.push(`tick ${i} card ${JSON.stringify(card())}`)
    }
    return log
  } finally {
    kit.unmount()
  }
}

describeNative("a card under a display:contents root and wrapper", () => {
  for (const prod of [false, true]) {
    it(`keeps the wrapper's font size across frames that replay it (production frames: ${prod})`, () => {
      const on = run(true, prod)
      const off = run(false, prod)
      expect(on).toEqual(off)
    })
  }
})
