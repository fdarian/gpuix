/// A deferred layer under a CSS transform. A popup of `<anchored>` is a portal into body: it is not turned or scaled
/// by the ancestors of its trigger, and floating-ui puts it by `getBoundingClientRect` of the trigger, the box that
/// holds the four corners of the transformed border box. With `deferred={false}` the popup is an absolute box in the
/// tree, drawn inside the transform and placed and flipped in its space. A box the layout glide lifts above its
/// siblings while it is dragged stays the element in place, inside the transform of its parents.

import { describe, expect, it } from "vitest"
import { Reorder } from "../index.js"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Renderer = TestRoot["renderer"]

const WINDOW = { width: 800, height: 600 }

function box(renderer: Renderer, testId: string) {
  const bounds = renderer.getElementBounds(renderer.findByTestId(testId)!.id)
  expect(bounds, `no painted bounds for ${testId}`).toBeTruthy()
  return bounds!
}

/** A 100x30 trigger at (100, `y`) in a box at the window's origin with `transform`, and a 80x50 popup under it. */
function scene(transform: string, y: number, popup: { height?: number; deferred?: boolean } = {}) {
  return (
    <div style={{ position: "absolute", left: 0, top: 0, width: 400, height: 300, transform, transformOrigin: "0 0" }}>
      <div testId="trigger" style={{ position: "absolute", left: 100, top: y, width: 100, height: 30 }}>
        <anchored gap={4} snapMargin={0} deferred={popup.deferred}>
          <div testId="popup" style={{ width: 80, height: popup.height ?? 50 }} />
        </anchored>
      </div>
    </div>
  )
}

function withScene(ui: React.ReactElement, check: (renderer: Renderer) => void) {
  const { render, renderer, unmount } = createTestRoot(WINDOW)
  try {
    render(ui)
    renderer.flush()
    renderer.dispatchNativeEvents()
    check(renderer)
  } finally {
    unmount()
  }
}

describeNative("<anchored> under a transform", () => {
  it("opens at the bounding box of a scaled trigger, and is not scaled itself", () => {
    withScene(scene("scale(2)", 100), (renderer) => {
      const trigger = box(renderer, "trigger")
      expect(trigger).toMatchObject({ x: 200, y: 200, width: 200, height: 60 })
      expect(box(renderer, "popup")).toMatchObject({ x: 200, y: trigger.y + trigger.height + 4, width: 80, height: 50 })
    })
  })

  it("opens at a translated trigger", () => {
    withScene(scene("translate(50px, 20px)", 100), (renderer) => {
      const trigger = box(renderer, "trigger")
      expect(trigger).toMatchObject({ x: 150, y: 120 })
      expect(box(renderer, "popup")).toMatchObject({ x: 150, y: 154, width: 80, height: 50 })
    })
  })

  it("opens at the bounding box of a rotated trigger", () => {
    withScene(scene("rotate(90deg)", 100), (renderer) => {
      // Turned about the corner of its parent, (100..200, 100..130) is (-130..-100, 100..200).
      const trigger = box(renderer, "trigger")
      expect(trigger.x).toBeCloseTo(-130, 1)
      expect(trigger.width).toBeCloseTo(30, 1)
      const popup = box(renderer, "popup")
      expect(popup.y).toBeCloseTo(trigger.y + trigger.height + 4, 1)
      expect(popup).toMatchObject({ width: 80, height: 50 })
    })
  })

  it("flips on the side of the window the scaled trigger is nearest to", () => {
    // The trigger is at 500..560 on screen; 120 under 564 does not fit in 600, and it fits over 496.
    withScene(scene("scale(2)", 250, { height: 120 }), (renderer) => {
      expect(box(renderer, "popup")).toMatchObject({ x: 200, y: 500 - 4 - 120, height: 120 })
    })
  })

  it("draws in the tree, scaled with the trigger, with deferred={false}", () => {
    withScene(scene("scale(2)", 100, { deferred: false }), (renderer) => {
      const trigger = box(renderer, "trigger")
      expect(box(renderer, "popup")).toMatchObject({
        x: 200,
        y: trigger.y + trigger.height + 8,
        width: 160,
        height: 100,
      })
    })
  })

  it("flips in the tree against the window in the space of the scale", () => {
    // 500..560 on screen. 120 local under the trigger is 240 on screen: 568 + 240 > 600. The window is 300 high in the
    // space of the box, where 250 + 30 + 4 + 120 fits.
    withScene(scene("scale(2)", 250, { height: 120, deferred: false }), (renderer) => {
      expect(box(renderer, "popup")).toMatchObject({ x: 200, y: 500 - 8 - 240, width: 160, height: 240 })
    })
  })
})

describeNative("a dragged layout box under a transform", () => {
  it("is drawn scaled with its parent while it is dragged", () => {
    const { render, renderer, unmount } = createTestRoot(WINDOW)
    try {
      renderer.clockPause()
      render(
        <div style={{ transform: "scale(2)", transformOrigin: "0 0", width: 200, height: 200 }}>
          <Reorder.Group axis="y" values={["a", "b"]} onReorder={() => {}} style={{ display: "flex", flexDirection: "column" }}>
            {["a", "b"].map((name) => (
              <Reorder.Item key={name} value={name} testId={name} style={{ height: 20, width: 50 }} />
            ))}
          </Reorder.Group>
        </div>
      )
      expect(box(renderer, "a")).toMatchObject({ y: 0, width: 100, height: 40 })
      renderer.nativeSimulateMouseDown(20, 20)
      renderer.nativeSimulateMouseMove(20, 30)
      renderer.flush()
      const dragged = box(renderer, "a")
      expect(dragged).toMatchObject({ width: 100, height: 40 })
      expect(dragged.y).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })
})
