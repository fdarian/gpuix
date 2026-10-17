/// `hasHover`, `hasActive`, `hasFocus` and `hasFocusVisible`: `:has(:hover)`,
/// `:has(:active)`, `:has(:focus)` and `:has(:focus-visible)`. An element matches while a
/// strict descendant does, so the element itself being hovered, pressed or focused is
/// not enough.

import fs from "fs"
import type { ReactElement } from "react"
import { useState } from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createPortal } from "../reconciler/portal.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const YELLOW: Rgba = [255, 255, 0, 255]

type TestRoot = ReturnType<typeof createTestRoot>

function sample(testRoot: TestRoot, name: string, x: number, y: number): Rgba {
  const file = `${SHOTS_DIR}/has-state-${name}.png`
  fs.rmSync(file, { force: true })
  testRoot.renderer.captureScreenshot(file)
  return readLogicalPixels(file, testRoot.renderer).at(x, y)
}

function withRoot(tree: ReactElement, run: (testRoot: TestRoot) => void) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    run(testRoot)
  } finally {
    testRoot.unmount()
  }
}

const bounds = (testRoot: TestRoot, testId: string) => {
  testRoot.renderer.dispatchNativeEvents()
  return testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId(testId).id)
}
const widthOf = (testRoot: TestRoot, testId: string) => bounds(testRoot, testId).width
const focus = (testRoot: TestRoot, testId: string) =>
  testRoot.renderer.focusElement(testRoot.renderer.findByTestId(testId).id)
const blur = (testRoot: TestRoot, testId: string) =>
  testRoot.renderer.blurElement(testRoot.renderer.findByTestId(testId).id)

/** A red 100x100 box with a transparent 40x40 child in its corner. (20, 20) is over the
 *  child, (80, 80) over the box alone, (500, 500) over neither. The box is sampled at
 *  (80, 80). */
function box(boxStyle: Record<string, unknown>, childStyle: Record<string, unknown> = {}, props: { inert?: boolean } = {}) {
  return (
    <div {...props} style={{ width: 100, height: 100, backgroundColor: "#ff0000", ...boxStyle }}>
      <div style={{ width: 40, height: 40, ...childStyle }} />
    </div>
  )
}

describeNative("hasHover", () => {
  it("restyles an ancestor while a descendant is under the pointer, and reverts when it leaves", () => {
    withRoot(box({ hasHover: { backgroundColor: "#0000ff" } }), (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(sample(testRoot, "hover-off", 80, 80)).toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "hover-on", 80, 80)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(sample(testRoot, "hover-off-again", 80, 80)).toEqual(RED)
    })
  })

  it("does not match while only the element itself is under the pointer", () => {
    // The same position does hover the element, so its `hover` applies and shows the
    // element is hovered while hasHover is not.
    const tree = box({ hasHover: { backgroundColor: "#0000ff", width: 120 }, hover: { backgroundColor: "#00ff00" } })
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "self-only", 80, 80)).toEqual(GREEN)
      // hasHover's width would widen the box to x=120.
      expect(sample(testRoot, "self-only-wide", 110, 80)).not.toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "self-then-child", 110, 80)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "child-then-self", 110, 80)).not.toEqual(GREEN)
    })
  })

  it("matches for a deeper descendant too, and not for a sibling", () => {
    const tree = (
      <div style={{ width: 200, height: 100, display: "flex" }}>
        <div style={{ width: 100, height: 100, backgroundColor: "#ff0000", hasHover: { backgroundColor: "#0000ff" } }}>
          <div style={{ width: 80, height: 80 }}>
            <div style={{ width: 20, height: 20 }} />
          </div>
        </div>
        <div style={{ width: 100, height: 100 }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(10, 10)
      expect(sample(testRoot, "deep", 90, 90)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(150, 50)
      expect(sample(testRoot, "sibling", 90, 90)).toEqual(RED)
    })
  })

  it("applies above groupHover and below the element's own hover", () => {
    // Over the child the box is hovered too, so its own hover beats hasHover and groupHover.
    const tree = (
      <div style={{ group: true, width: 100, height: 100 }}>
        <div
          style={{
            width: 100,
            height: 100,
            backgroundColor: "#ff0000",
            groupHover: { backgroundColor: "#00ff00" },
            hasHover: { backgroundColor: "#0000ff" },
            hover: { backgroundColor: "#ffff00" },
          }}
        >
          <div style={{ width: 40, height: 40 }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "order-all", 80, 80)).toEqual(YELLOW)
    })
    const withoutHover = (
      <div style={{ group: true, width: 100, height: 100 }}>
        <div
          style={{
            width: 100,
            height: 100,
            backgroundColor: "#ff0000",
            groupHover: { backgroundColor: "#00ff00" },
            hasHover: { backgroundColor: "#0000ff" },
          }}
        >
          <div style={{ width: 40, height: 40 }} />
        </div>
      </div>
    )
    withRoot(withoutHover, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "order-group", 80, 80)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "order-has", 80, 80)).toEqual(BLUE)
    })
  })

  it("restyles layout in the same frame", () => {
    const tree = (
      <div style={{ width: 200, height: 100 }}>
        <div testId="box" style={{ width: 100, height: 100, hasHover: { width: 150 } }}>
          <div style={{ width: 40, height: 40 }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      expect(widthOf(testRoot, "box")).toBe(100)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(widthOf(testRoot, "box")).toBe(150)
      testRoot.renderer.nativeSimulateMouseMove(120, 20)
      // Still over the box, but no longer over its child.
      expect(widthOf(testRoot, "box")).toBe(100)
    })
  })

  it("counts a portal child as a descendant of its target, wherever it is painted", () => {
    function Screen() {
      const [layer, setLayer] = useState<PublicInstance | null>(null)
      return (
        <div style={{ width: 400, height: 200 }}>
          <div
            testId="layer"
            ref={setLayer}
            style={{ width: 100, height: 100, backgroundColor: "#ff0000", hasHover: { backgroundColor: "#0000ff" } }}
          />
          {layer &&
            createPortal(
              <div
                testId="portaled"
                style={{ position: "absolute", left: 200, top: 0, width: 40, height: 40, backgroundColor: "#00ff00" }}
              />,
              layer
            )}
        </div>
      )
    }
    withRoot(<Screen />, (testRoot) => {
      expect(testRoot.renderer.findByTestId("portaled").parentId).toBe(testRoot.renderer.findByTestId("layer").id)
      testRoot.renderer.nativeSimulateMouseMove(500, 150)
      expect(sample(testRoot, "portal-off", 80, 80)).toEqual(RED)
      // Over the portaled child, which is outside the layer's own box.
      testRoot.renderer.nativeSimulateMouseMove(220, 20)
      expect(sample(testRoot, "portal-on", 80, 80)).toEqual(BLUE)
    })
  })

  it("does not apply inside an inert subtree", () => {
    withRoot(box({ hasHover: { backgroundColor: "#0000ff" } }, {}, { inert: true }), (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "inert-self", 80, 80)).toEqual(RED)
    })
    const inertChild = (
      <div style={{ width: 100, height: 100, backgroundColor: "#ff0000", hasHover: { backgroundColor: "#0000ff" } }}>
        <div inert style={{ width: 40, height: 40 }} />
      </div>
    )
    withRoot(inertChild, (testRoot) => {
      // Nothing inside an inert subtree can be hovered, so there is no descendant to match.
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "inert-child", 80, 80)).toEqual(RED)
    })
  })

  it("runs a transition declared in it", () => {
    const tree = (
      <div style={{ width: 300, height: 100 }}>
        <div testId="box" style={{ width: 100, height: 100, hasHover: { width: 200, transition: "width 1s linear" } }}>
          <div style={{ width: 40, height: 40 }} />
        </div>
      </div>
    )
    const testRoot = createTestRoot()
    try {
      testRoot.renderer.clockPause()
      testRoot.render(tree)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      testRoot.renderer.clockFastForward(500)
      expect(widthOf(testRoot, "box")).toBeCloseTo(150, 0)
      testRoot.renderer.clockFastForward(1000)
      expect(widthOf(testRoot, "box")).toBe(200)
    } finally {
      testRoot.unmount()
    }
  })

  it("reaches the descendants' currentColor", () => {
    const tree = (
      <div style={{ width: 100, height: 100, color: "#00ff00", hasHover: { color: "#0000ff" } }}>
        <div style={{ width: 40, height: 40, backgroundColor: "currentColor" }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(sample(testRoot, "color-off", 20, 20)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "color-on", 20, 20)).toEqual(BLUE)
    })
  })
})

describeNative("hasActive", () => {
  it("restyles an ancestor while the mouse is down on a descendant, and reverts on release", () => {
    withRoot(box({ hasActive: { backgroundColor: "#0000ff" } }), (testRoot) => {
      expect(sample(testRoot, "active-off", 80, 80)).toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      expect(sample(testRoot, "active-on", 80, 80)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseUp(20, 20)
      expect(sample(testRoot, "active-released", 80, 80)).toEqual(RED)
    })
  })

  it("does not match while the element itself is pressed", () => {
    withRoot(box({ hasActive: { backgroundColor: "#0000ff" }, active: { backgroundColor: "#00ff00" } }), (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      testRoot.renderer.nativeSimulateMouseDown(80, 80)
      expect(sample(testRoot, "active-self", 80, 80)).toEqual(GREEN)
    })
  })

  it("applies above groupActive and below the element's own active", () => {
    const tree = (
      <div style={{ group: true, width: 100, height: 100 }}>
        <div
          style={{
            width: 100,
            height: 100,
            backgroundColor: "#ff0000",
            groupActive: { backgroundColor: "#00ff00" },
            hasActive: { backgroundColor: "#0000ff" },
            active: { backgroundColor: "#ffff00" },
          }}
        >
          <div style={{ width: 40, height: 40 }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      // Pressing the box itself: its own active, and the group's.
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      testRoot.renderer.nativeSimulateMouseDown(80, 80)
      expect(sample(testRoot, "active-order-self", 80, 80)).toEqual(YELLOW)
      testRoot.renderer.nativeSimulateMouseUp(80, 80)
      // Pressing the child: the box is pressed too (an ancestor of the target is :active),
      // so its own active wins again.
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      expect(sample(testRoot, "active-order-child", 80, 80)).toEqual(YELLOW)
    })
    const withoutOwn = (
      <div style={{ group: true, width: 100, height: 100 }}>
        <div
          style={{
            width: 100,
            height: 100,
            backgroundColor: "#ff0000",
            groupActive: { backgroundColor: "#00ff00" },
            hasActive: { backgroundColor: "#0000ff" },
          }}
        >
          <div style={{ width: 40, height: 40 }} />
        </div>
      </div>
    )
    withRoot(withoutOwn, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      testRoot.renderer.nativeSimulateMouseDown(80, 80)
      expect(sample(testRoot, "active-order-group", 80, 80)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseUp(80, 80)
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      expect(sample(testRoot, "active-order-has", 80, 80)).toEqual(BLUE)
    })
  })

  it("does not apply inside an inert subtree", () => {
    withRoot(box({ hasActive: { backgroundColor: "#0000ff" } }, {}, { inert: true }), (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      expect(sample(testRoot, "active-inert", 80, 80)).toEqual(RED)
    })
  })
})

describeNative("hasFocus", () => {
  const field = <input testId="field" style={{ width: 100, height: 20 }} />

  it("applies while a descendant is focused, and clears on blur", () => {
    const tree = (
      <div testId="box" style={{ width: 200, height: 100, hasFocus: { width: 300 } }}>
        {field}
      </div>
    )
    withRoot(tree, (testRoot) => {
      expect(widthOf(testRoot, "box")).toBe(200)
      focus(testRoot, "field")
      expect(widthOf(testRoot, "box")).toBe(300)
      blur(testRoot, "field")
      expect(widthOf(testRoot, "box")).toBe(200)
      // A click moves focus the same way a script does.
      testRoot.renderer.nativeSimulateMouseDown(10, 10)
      testRoot.renderer.nativeSimulateMouseUp(10, 10)
      expect(widthOf(testRoot, "box")).toBe(300)
      testRoot.renderer.nativeSimulateMouseDown(150, 80)
      testRoot.renderer.nativeSimulateMouseUp(150, 80)
      expect(widthOf(testRoot, "box")).toBe(200)
    })
  })

  it("does not apply when the element itself is the focused one, unlike focusWithin", () => {
    const tree = (
      <div>
        <div
          testId="self"
          tabIndex={0}
          style={{ width: 50, height: 20, hasFocus: { width: 100 }, focusWithin: { height: 40 } }}
        />
        <div testId="outer" style={{ width: 50, height: 20, hasFocus: { width: 100 }, focusWithin: { height: 40 } }}>
          {field}
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      focus(testRoot, "self")
      expect([widthOf(testRoot, "self"), bounds(testRoot, "self").height]).toEqual([50, 40])
      focus(testRoot, "field")
      expect([widthOf(testRoot, "outer"), bounds(testRoot, "outer").height]).toEqual([100, 40])
      expect([widthOf(testRoot, "self"), bounds(testRoot, "self").height]).toEqual([50, 20])
    })
  })

  it("does not apply while focus is on a sibling", () => {
    const tree = (
      <div>
        <div testId="box" style={{ width: 50, height: 20, hasFocus: { width: 100 } }} />
        <input testId="outside" style={{ width: 100, height: 20 }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      focus(testRoot, "outside")
      expect(widthOf(testRoot, "box")).toBe(50)
    })
  })

  it("applies above groupFocusWithin and below focusWithin", () => {
    const tree = (
      <div style={{ group: true, width: 300, height: 200 }}>
        <input testId="field" style={{ width: 100, height: 20 }} />
        <div
          testId="follower"
          style={{
            width: 10,
            height: 20,
            groupFocusWithin: { width: 30, height: 50, opacity: 0.5 },
            hasFocus: { width: 40, height: 60 },
          }}
        />
        <div testId="holder" style={{ width: 10, height: 20, hasFocus: { width: 40, height: 60 }, focusWithin: { height: 70 } }}>
          <input testId="inner" style={{ width: 10, height: 10 }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      // The follower is only in the group: groupFocusWithin matches, hasFocus does not.
      focus(testRoot, "field")
      expect([widthOf(testRoot, "follower"), bounds(testRoot, "follower").height]).toEqual([30, 50])
      focus(testRoot, "inner")
      expect([widthOf(testRoot, "holder"), bounds(testRoot, "holder").height]).toEqual([40, 70])
    })
  })

  it("runs a transition declared in it", () => {
    const tree = (
      <div testId="box" style={{ width: 200, height: 100, hasFocus: { width: 300, transition: "width 1s linear" } }}>
        {field}
      </div>
    )
    const testRoot = createTestRoot()
    try {
      testRoot.renderer.clockPause()
      testRoot.render(tree)
      focus(testRoot, "field")
      testRoot.renderer.clockFastForward(500)
      expect(widthOf(testRoot, "box")).toBeCloseTo(250, 0)
      testRoot.renderer.clockFastForward(1000)
      expect(widthOf(testRoot, "box")).toBe(300)
    } finally {
      testRoot.unmount()
    }
  })
})

describeNative("hasFocusVisible", () => {
  const scene = (boxStyle: Record<string, unknown>) => (
    <div>
      <div testId="box" style={{ width: 50, height: 20, ...boxStyle }}>
        <div testId="button" tabIndex={0} style={{ width: 20, height: 10 }} />
      </div>
      <input testId="field" style={{ width: 100, height: 20 }} />
    </div>
  )

  it("applies while a descendant matches :focus-visible, and not for a mouse focus on a button", () => {
    withRoot(scene({ hasFocusVisible: { width: 120 } }), (testRoot) => {
      expect(widthOf(testRoot, "box")).toBe(50)
      testRoot.renderer.simulateKeystrokes("Tab")
      expect(widthOf(testRoot, "box")).toBe(120)
      testRoot.renderer.simulateKeystrokes("Tab")
      expect(widthOf(testRoot, "box")).toBe(50)
    })
    withRoot(scene({ hasFocusVisible: { width: 120 }, hasFocus: { height: 40 } }), (testRoot) => {
      const center = bounds(testRoot, "button")
      testRoot.renderer.nativeSimulateClick(center.x + 5, center.y + 5)
      // The button is focused by the mouse: focused, but not focus-visible.
      expect(testRoot.renderer.matchesFocus(testRoot.renderer.findByTestId("button").id, ":focus")).toBe(true)
      expect([widthOf(testRoot, "box"), bounds(testRoot, "box").height]).toEqual([50, 40])
    })
  })

  it("does not apply when the element itself is the focus-visible one", () => {
    const tree = (
      <div testId="box" tabIndex={0} style={{ width: 50, height: 20, hasFocusVisible: { width: 120 }, focusVisible: { height: 40 } }} />
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.simulateKeystrokes("Tab")
      expect([widthOf(testRoot, "box"), bounds(testRoot, "box").height]).toEqual([50, 40])
    })
  })

  it("applies above focusWithin and below focusVisible", () => {
    const tree = (
      <div
        testId="box"
        tabIndex={0}
        style={{
          width: 50,
          height: 20,
          focusWithin: { width: 60, height: 30, opacity: 0.5 },
          hasFocusVisible: { width: 70, height: 40 },
          focusVisible: { height: 50 },
        }}
      >
        <div testId="button" tabIndex={0} style={{ width: 20, height: 10 }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      // Tab goes to the box first (it is the nearer stop), then into the button.
      testRoot.renderer.simulateKeystrokes("Tab")
      expect([widthOf(testRoot, "box"), bounds(testRoot, "box").height]).toEqual([60, 50])
      testRoot.renderer.simulateKeystrokes("Tab")
      expect([widthOf(testRoot, "box"), bounds(testRoot, "box").height]).toEqual([70, 40])
    })
  })

  it("runs a transition declared in it", () => {
    const tree = scene({ hasFocusVisible: { width: 150, transition: "width 1s linear" } })
    const testRoot = createTestRoot()
    try {
      testRoot.renderer.clockPause()
      testRoot.render(tree)
      testRoot.renderer.simulateKeystrokes("Tab")
      testRoot.renderer.clockFastForward(500)
      expect(widthOf(testRoot, "box")).toBeCloseTo(100, 0)
      testRoot.renderer.clockFastForward(1000)
      expect(widthOf(testRoot, "box")).toBe(150)
    } finally {
      testRoot.unmount()
    }
  })
})

let measuredColumn: number | undefined

/** Menlo has one advance for every character, so a column is a fixed width. Measured
 *  once, in a window of its own: a second test renderer stops the first. */
function columnWidth() {
  measuredColumn ??= measureColumn()
  return measuredColumn
}

function measureColumn() {
  const testRoot = createTestRoot()
  testRoot.render(
    <div highlight={{ query: "aaaaaaaaaa" }}>
      <text style={TEXT}>{"aaaaaaaaaa"}</text>
    </div>
  )
  const width = testRoot.renderer.getPaintedHighlights()[0]!.rects[0]!.width / 10
  testRoot.unmount()
  return width
}

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const

describeNative("relational state on inline boxes", () => {
  /** `aa [bb [cc]] dd`: the outer span is columns 3-7, the inner one 6-7. */
  const sentence = (outer: object) => (
    <div style={{ width: 400 }}>
      <text testId="p" style={TEXT}>
        aa{" "}
        <span style={outer}>
          bb <span>cc</span>
        </span>
        <span ref={tail}>dd</span>
      </text>
    </div>
  )
  let tailInstance: PublicInstance | null = null
  const tail = (instance: PublicInstance | null) => {
    if (instance) tailInstance = instance
  }

  it("restyles a span while a span inside it is hovered, and not while only the span is", () => {
    const column = columnWidth()
    const testRoot = createTestRoot({ width: 300, height: 100 })
    try {
      // Padding after the inner span, so the span under the pointer does not move away.
      testRoot.render(sentence({ hasHover: { paddingRight: 20 } }))
      const point = (columns: number) => {
        const paragraph = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("p")!.id)!
        return { x: paragraph.x + columns * column, y: paragraph.y + LINE / 2 }
      }
      const rest = tailInstance!.getBoundingClientRect().x
      // The outer span's own text.
      const own = point(3.5)
      testRoot.renderer.nativeSimulateMouseMove(own.x, own.y)
      expect(tailInstance!.getBoundingClientRect().x).toBeCloseTo(rest, 1)
      // The inner span.
      const inner = point(6.5)
      testRoot.renderer.nativeSimulateMouseMove(inner.x, inner.y)
      expect(tailInstance!.getBoundingClientRect().x - rest).toBeCloseTo(20, 1)
      const away = point(0.5)
      testRoot.renderer.nativeSimulateMouseMove(away.x, away.y)
      expect(tailInstance!.getBoundingClientRect().x).toBeCloseTo(rest, 1)
    } finally {
      testRoot.unmount()
    }
  })
})
