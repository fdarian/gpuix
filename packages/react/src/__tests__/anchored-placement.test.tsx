import { describe, expect, it } from "vitest"
import type { AnchoredPlacementEvent } from "@ghia/native/host"
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

  describe("re-places on every layout", () => {
    it("flips when the trigger scrolls toward the window edge, and reports it", () => {
      const events: AnchoredPlacementEvent[] = []
      const { render, renderer, unmount } = createTestRoot(WINDOW)
      try {
        render(
          <div testId="scroller" style={{ width: 800, height: 600, overflow: "scroll" }}>
            <div style={{ height: 450 }} />
            <div testId="trigger" style={{ position: "relative", width: 100, height: 30 }}>
              <anchored gap={4} snapMargin={8} onPlacement={(event) => events.push(event)}>
                <div testId="popup" style={{ width: 80, height: 200 }} />
              </anchored>
            </div>
            <div style={{ height: 1000 }} />
          </div>
        )
        renderer.flush()
        renderer.dispatchNativeEvents()
        const scroller = renderer.findByTestId("scroller")!.id
        // 450 + 30 + 4 + 200 does not fit above 592: opens upward.
        expect(events.at(-1)).toMatchObject({ side: "top" })
        expect(box(renderer, "popup").y).toBe(450 - 4 - 200)

        renderer.scrollTo(scroller, 0, 300)
        renderer.flush()
        renderer.dispatchNativeEvents()
        expect(events.at(-1)).toMatchObject({ side: "bottom" })
        expect(box(renderer, "popup").y).toBe(150 + 30 + 4)

        renderer.scrollTo(scroller, 0, 0)
        renderer.flush()
        renderer.dispatchNativeEvents()
        expect(events.at(-1)).toMatchObject({ side: "top" })
        expect(events.map((event) => event.side)).toEqual(["top", "bottom", "top"])
      } finally {
        unmount()
      }
    })

    it("flips when the popup grows or shrinks", () => {
      const events: AnchoredPlacementEvent[] = []
      const { render, renderer, unmount } = createTestRoot(WINDOW)
      try {
        const ui = (height: number) =>
          scene({ x: 100, y: 400 }, { width: 80, height }, {}, (event) => events.push(event))
        render(ui(100))
        renderer.dispatchNativeEvents()
        expect(events.at(-1)).toMatchObject({ side: "bottom" })

        render(ui(300))
        renderer.dispatchNativeEvents()
        expect(events.at(-1)).toMatchObject({ side: "top" })
        expect(box(renderer, "popup").y).toBe(400 - 4 - 300)

        render(ui(100))
        renderer.dispatchNativeEvents()
        expect(events.at(-1)).toMatchObject({ side: "bottom" })
        expect(events.map((event) => event.side)).toEqual(["bottom", "top", "bottom"])
      } finally {
        unmount()
      }
    })
  })

  describe("anchor size", () => {
    it("reports the trigger's border box in onPlacement", () => {
      const events: AnchoredPlacementEvent[] = []
      withScene(
        scene({ x: 100, y: 100 }, { width: 80, height: 50 }, {}, (event) => events.push(event)),
        () => {
          expect(events.at(-1)).toMatchObject({ anchorWidth: 100, anchorHeight: 30 })
        }
      )
    })

    it("resolves anchor-size() on a direct child against the trigger", () => {
      const ui = (triggerWidth: number) => (
        <div>
          <div
            testId="trigger"
            style={{ position: "absolute", left: 100, top: 100, width: triggerWidth, height: 30 }}
          >
            <anchored gap={4}>
              <div
                testId="match"
                style={{ width: "anchor-size(width)", minHeight: "anchor-size(height)", height: 10 }}
              />
              <div testId="floor" style={{ minWidth: "anchor-size(width)", maxWidth: 400 }}>
                <div style={{ width: 20, height: 10 }} />
              </div>
            </anchored>
          </div>
        </div>
      )
      const { render, renderer, unmount } = createTestRoot(WINDOW)
      try {
        render(ui(140))
        renderer.flush()
        renderer.flush()
        expect(box(renderer, "match")).toMatchObject({ width: 140, height: 30 })
        expect(box(renderer, "floor").width).toBe(140)

        // The trigger changes size: the child follows without any JS measuring.
        render(ui(220))
        renderer.flush()
        renderer.flush()
        expect(box(renderer, "match").width).toBe(220)
        expect(box(renderer, "floor").width).toBe(220)
      } finally {
        unmount()
      }
    })

    it("resolves anchor-size() on margins and insets", () => {
      const { render, renderer, unmount } = createTestRoot(WINDOW)
      try {
        render(
          <div>
            <div
              testId="trigger"
              style={{ position: "absolute", left: 100, top: 100, width: 140, height: 30 }}
            >
              <anchored gap={4}>
                <div
                  testId="margin"
                  style={{ marginLeft: "anchor-size(width)", marginTop: "anchor-size(height)", width: 20, height: 10 }}
                />
                <div
                  testId="inset"
                  style={{ position: "relative", left: "anchor-size(width)", top: "anchor-size(height)", width: 20, height: 10 }}
                />
              </anchored>
            </div>
          </div>
        )
        renderer.flush()
        renderer.flush()
        const margin = box(renderer, "margin")
        const inset = box(renderer, "inset")
        // The layer opens at (100, 134); the child sits 140 right of it and 30 below it.
        expect(margin).toMatchObject({ x: 100 + 140, y: 134 + 30, width: 20, height: 10 })
        // The inset shifts the box from where flow put it, under the margin child.
        expect(inset).toMatchObject({ x: 100 + 140, y: 134 + 30 + 10 + 30, width: 20, height: 10 })
      } finally {
        unmount()
      }
    })

    it("rejects an unknown anchor-size axis", () => {
      const errors: Error[] = []
      const { render, unmount } = createTestRoot({ ...WINDOW, onUncaughtError: (error) => errors.push(error) })
      try {
        render(<div style={{ width: "anchor-size(depth)" }} />)
        expect(errors.map((error) => error.message).join()).toMatch(/anchor-size/)
      } finally {
        unmount()
      }
    })
  })
})
