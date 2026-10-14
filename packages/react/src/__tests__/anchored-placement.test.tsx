import { describe, expect, it } from "vitest"
import type { AnchoredPlacementEvent } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Renderer = TestRoot["renderer"]

function box(renderer: Renderer, testId: string) {
  const bounds = renderer.getElementBounds(renderer.findByTestId(testId)!.id)
  expect(bounds, `no painted bounds for ${testId}`).toBeTruthy()
  return bounds!
}

const WINDOW = { width: 800, height: 600 }

/** A 100x30 trigger at (x, y) with a popup of the given size under `anchored`. */
function scene(
  trigger: { x: number; y: number },
  popup: { width: number; height: number },
  props: Record<string, unknown> = {},
  onPlacement?: (event: AnchoredPlacementEvent) => void
) {
  return (
    <div>
      <div
        testId="trigger"
        style={{ position: "absolute", left: trigger.x, top: trigger.y, width: 100, height: 30 }}
      >
        <anchored gap={4} snapMargin={8} onPlacement={onPlacement} {...props}>
          <div testId="popup" style={{ width: popup.width, height: popup.height }} />
        </anchored>
      </div>
    </div>
  )
}

function withScene(ui: React.ReactElement, check: (renderer: Renderer) => void) {
  const { render, renderer, unmount } = createTestRoot(WINDOW)
  try {
    render(ui)
    // A capped layer settles on the frame after its first placement.
    renderer.flush()
    renderer.dispatchNativeEvents()
    check(renderer)
  } finally {
    unmount()
  }
}

describeNative("<anchored> placement", () => {
  it("opens below the trigger, aligned to its start, with the gap", () => {
    withScene(scene({ x: 100, y: 100 }, { width: 80, height: 50 }), (renderer) => {
      expect(box(renderer, "popup")).toMatchObject({ x: 100, y: 134 })
    })
  })

  it("align=center centers on the trigger exactly once", () => {
    withScene(
      scene({ x: 100, y: 100 }, { width: 40, height: 50 }, { align: "center" }),
      (renderer) => {
        expect(box(renderer, "popup").x).toBe(130)
      }
    )
    withScene(
      scene({ x: 300, y: 100 }, { width: 40, height: 50 }, { side: "right", align: "center" }),
      (renderer) => {
        const popup = box(renderer, "popup")
        expect(popup.x).toBe(404)
        expect(popup.y).toBe(115 - 25)
      }
    )
  })

  it("align=end lines the trailing edges up", () => {
    withScene(
      scene({ x: 200, y: 100 }, { width: 40, height: 50 }, { align: "end" }),
      (renderer) => {
        expect(box(renderer, "popup").x).toBe(300 - 40)
      }
    )
  })

  it("flips to the other side of the trigger with the same gap when it does not fit", () => {
    withScene(scene({ x: 100, y: 500 }, { width: 80, height: 120 }), (renderer) => {
      expect(box(renderer, "popup")).toMatchObject({ y: 500 - 4 - 120 })
    })
  })

  it("takes the side with more room when neither side fits", () => {
    withScene(scene({ x: 100, y: 400 }, { width: 80, height: 500 }), (renderer) => {
      // 388 above, 168 below: above is larger, and the popup shifts inside the window.
      const popup = box(renderer, "popup")
      expect(popup.y).toBe(8)
    })
  })

  it('fit="snap" slides instead of flipping', () => {
    withScene(
      scene({ x: 100, y: 500 }, { width: 80, height: 120 }, { fit: "snap" }),
      (renderer) => {
        expect(box(renderer, "popup").y).toBe(600 - 8 - 120)
      }
    )
  })

  it("shifts along the cross axis to stay inside the window", () => {
    withScene(scene({ x: 700, y: 100 }, { width: 200, height: 50 }), (renderer) => {
      expect(box(renderer, "popup").x).toBe(800 - 8 - 200)
    })
  })

  it("reports placement and available room, and only when they change", () => {
    const events: AnchoredPlacementEvent[] = []
    const { render, renderer, unmount } = createTestRoot(WINDOW)
    try {
      const ui = (y: number) =>
        scene({ x: 100, y }, { width: 80, height: 120 }, {}, (event) => events.push(event))
      render(ui(100))
      renderer.dispatchNativeEvents()
      expect(events).toHaveLength(1)
      expect(events[0]).toMatchObject({
        side: "bottom",
        align: "start",
        availableWidth: 784,
        availableHeight: 600 - 8 - 130 - 4,
      })
      render(ui(100))
      renderer.dispatchNativeEvents()
      expect(events, "an identical frame reports nothing").toHaveLength(1)

      render(ui(500))
      renderer.dispatchNativeEvents()
      expect(events.at(-1)).toMatchObject({ side: "top", availableHeight: 500 - 8 - 4 })
    } finally {
      unmount()
    }
  })

  it("constrainToAvailable caps the layer to the room that is left", () => {
    withScene(
      scene(
        { x: 100, y: 400 },
        { width: 80, height: 2000 },
        { constrainToAvailable: true, testId: "layer" }
      ),
      (renderer) => {
        // 388px above the trigger: the layer fills it instead of running off the window.
        expect(box(renderer, "layer")).toMatchObject({ y: 8, height: 388 })
      }
    )
  })
})
