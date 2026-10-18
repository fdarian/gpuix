// @ts-nocheck
// A pass that only lays out (a bounds read on a window with a change to show) builds the elements it visits. One
// under `display: none` at that moment is not visited, so the change it was told of (the pointer here) has to reach
// it in the frame that does, or that frame replays what was recorded before the change. The other sources of change
// (active, focus, measurement, links, selection) are covered by the unit tests of retain.rs.
// The read-outs avoid captureScreenshot, which draws and would hide the stale frame.
import React from "react"
import { expect, it } from "vitest"
import { Card, describeNative, Page, setup } from "./retained-harness.js"

function scenario(planning: boolean, hideAround: boolean) {
  const render = (hidden: boolean) => (
    <Page
      card={(index) =>
        index === 3 ? (
          <div key={index} testId="wrap" style={{ display: hidden ? "none" : "block" }}>
            <Card index={index} extra={<div testId="probe" style={{ width: 20, height: 6, hover: { width: 60 } }} />} />
          </div>
        ) : undefined
      }
    />
  )
  const kit = setup(render(false))
  const { renderer, render: rerender, bounds, unmount } = kit
  try {
    renderer.setRetainPlanning(planning)
    kit.settle()
    kit.settle()
    renderer.setInlineDraws(false)
    const probeBox = bounds("probe")
    renderer.dispatchMouseMove(probeBox.x + 5, probeBox.y + 3)
    if (hideAround) {
      rerender(render(true))
      renderer.getElementBounds(renderer.findByTestId("card-4").id)
    }
    rerender(render(false))
    renderer.drawFrame()
    renderer.drawFrame()
    renderer.drawFrame()
    return bounds("probe").width
  } finally {
    unmount()
  }
}

describeNative("a layout-only pass while the hovered element is under display:none", () => {
  for (const hideAround of [false, true]) {
    it(`applies the hover style after it shows again (hidden and read in between: ${hideAround})`, () => {
      const built = scenario(false, hideAround)
      expect(built).toBe(60)
      expect(scenario(true, hideAround)).toBe(built)
    })
  }
})
