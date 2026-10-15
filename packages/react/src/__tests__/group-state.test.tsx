import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const GREEN: Rgba = [0, 255, 0, 255]

type TestRoot = ReturnType<typeof createTestRoot>

function sample(testRoot: TestRoot, name: string, x: number, y: number): Rgba {
  const file = `${SHOTS_DIR}/group-state-${name}.png`
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

/** A 100x100 group with a 40x40 child in its corner. Hover (80, 80) is in the
 *  group but outside the child. */
function card(groupProp: Record<string, unknown>, childStyle: Record<string, unknown>) {
  return (
    <div style={{ width: 100, height: 100, backgroundColor: "#ffffff", ...groupProp }}>
      <div style={{ width: 40, height: 40, backgroundColor: "#ff0000", ...childStyle }} />
    </div>
  )
}

describeNative("groupHover", () => {
  it("restyles a descendant while the ancestor group is hovered", () => {
    withRoot(card({ group: true }, { groupHover: { backgroundColor: "#0000ff" } }), (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(sample(testRoot, "hover-off", 20, 20)).toEqual(RED)
      // Over the group, not over the child itself.
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "hover-on", 20, 20)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(sample(testRoot, "hover-off-again", 20, 20)).toEqual(RED)
    })
  })

  it("matches the named group, and ignores a group of another name", () => {
    withRoot(
      card(
        { group: "card" },
        {
          groupHover: { group: "card", backgroundColor: "#0000ff" },
        }
      ),
      (testRoot) => {
        testRoot.renderer.nativeSimulateMouseMove(80, 80)
        expect(sample(testRoot, "named-on", 20, 20)).toEqual(BLUE)
      }
    )
    withRoot(
      card({ group: "card" }, { groupHover: { group: "other", backgroundColor: "#0000ff" } }),
      (testRoot) => {
        testRoot.renderer.nativeSimulateMouseMove(80, 80)
        expect(sample(testRoot, "named-other", 20, 20)).toEqual(RED)
      }
    )
    // `true` and an unnamed groupHover share the default name.
    withRoot(card({ group: true }, { groupHover: { group: true, backgroundColor: "#0000ff" } }), (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "default-name", 20, 20)).toEqual(BLUE)
    })
  })

  it("follows the nearest ancestor with that name", () => {
    const tree = (
      <div style={{ width: 200, height: 100, display: "flex", backgroundColor: "#ffffff", group: "row" }}>
        <div style={{ width: 100, height: 100, group: "row" }}>
          <div style={{ width: 40, height: 40, backgroundColor: "#ff0000", groupHover: { group: "row", backgroundColor: "#0000ff" } }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      // Inside the outer group but outside the inner one: the nearest is the inner.
      testRoot.renderer.nativeSimulateMouseMove(150, 50)
      expect(sample(testRoot, "nearest-outer-only", 20, 20)).toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "nearest-inner", 20, 20)).toEqual(BLUE)
    })
  })

  it("takes two independent groups on different descendants", () => {
    const tree = (
      <div style={{ display: "flex" }}>
        <div style={{ width: 50, height: 50, group: "a", backgroundColor: "#ffffff" }}>
          <div style={{ width: 20, height: 20, backgroundColor: "#ff0000", groupHover: { group: "a", backgroundColor: "#0000ff" } }} />
        </div>
        <div style={{ width: 50, height: 50, group: "b", backgroundColor: "#ffffff" }}>
          <div style={{ width: 20, height: 20, backgroundColor: "#ff0000", groupHover: { group: "b", backgroundColor: "#00ff00" } }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(40, 40)
      expect(sample(testRoot, "pair-a", 10, 10)).toEqual(BLUE)
      expect(sample(testRoot, "pair-a-other", 60, 10)).toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(90, 40)
      expect(sample(testRoot, "pair-b", 60, 10)).toEqual(GREEN)
      expect(sample(testRoot, "pair-b-other", 10, 10)).toEqual(RED)
    })
  })

  it("also fires while the pointer is over the filled or positioned element itself", () => {
    // A fill or a position takes nothing from the hit test: the pointer is over
    // the child, which is hovered with every ancestor, the group included.
    const filled = (
      <div style={{ width: 100, height: 100, backgroundColor: "#ffffff", group: true }}>
        <div style={{ width: 40, height: 40, backgroundColor: "#ff0000", groupHover: { backgroundColor: "#0000ff" } }} />
      </div>
    )
    withRoot(filled, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "filled-child", 20, 20)).toEqual(BLUE)
    })
    const positioned = (
      <div style={{ width: 100, height: 100, backgroundColor: "#ffffff", group: true, position: "relative" }}>
        <div
          style={{ position: "absolute", left: 0, top: 0, width: 40, height: 40, backgroundColor: "#ff0000" }}
        />
        <div
          style={{
            position: "absolute",
            left: 50,
            top: 50,
            width: 40,
            height: 40,
            backgroundColor: "#ff0000",
            groupHover: { backgroundColor: "#0000ff" },
          }}
        />
      </div>
    )
    withRoot(positioned, (testRoot) => {
      // Over the other filled, positioned child, not the group's own box.
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "positioned-child", 60, 60)).toEqual(BLUE)
    })
  })

  it("applies between base and hover: the element's own hover beats groupHover", () => {
    const tree = card(
      { group: true },
      {
        groupHover: { backgroundColor: "#00ff00", opacity: 1 },
        hover: { backgroundColor: "#0000ff" },
      }
    )
    withRoot(tree, (testRoot) => {
      // Over the group only: the child is not hovered, so groupHover shows.
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "order-group-only", 20, 20)).toEqual(GREEN)
      // Over the child: both apply, hover wins.
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "order-both", 20, 20)).toEqual(BLUE)
    })
  })

  it("a groupHover's color reaches the descendants' currentColor", () => {
    const tree = (
      <div style={{ width: 100, height: 100, backgroundColor: "#ffffff", group: true }}>
        <div style={{ color: "#00ff00", groupHover: { color: "#0000ff" } }}>
          <div style={{ width: 40, height: 40, backgroundColor: "currentColor" }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(sample(testRoot, "group-color-off", 20, 20)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "group-color-on", 20, 20)).toEqual(BLUE)
    })
  })

  it("ignores a group above an inert subtree", () => {
    const tree = (
      <div style={{ width: 100, height: 100, backgroundColor: "#ffffff", group: true }}>
        <div inert style={{ width: 40, height: 40, backgroundColor: "#ff0000", groupHover: { backgroundColor: "#0000ff" } }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "inert", 20, 20)).toEqual(RED)
    })
  })

  it("resolves currentColor against the restyled element", () => {
    withRoot(
      card({ group: true }, { color: "#00ff00", groupHover: { color: "#0000ff", backgroundColor: "currentColor" } }),
      (testRoot) => {
        testRoot.renderer.nativeSimulateMouseMove(80, 80)
        expect(sample(testRoot, "current-color", 20, 20)).toEqual(BLUE)
      }
    )
  })
})

describeNative("groupActive", () => {
  it("restyles a descendant while the mouse is down inside the group", () => {
    withRoot(card({ group: true }, { groupActive: { backgroundColor: "#0000ff" } }), (testRoot) => {
      expect(sample(testRoot, "active-off", 20, 20)).toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      testRoot.renderer.nativeSimulateMouseDown(80, 80)
      expect(sample(testRoot, "active-on", 20, 20)).toEqual(BLUE)
    })
  })

  it("combines with groupHover on one element", () => {
    withRoot(
      card(
        { group: true },
        { groupHover: { backgroundColor: "#00ff00" }, groupActive: { backgroundColor: "#0000ff" } }
      ),
      (testRoot) => {
        testRoot.renderer.nativeSimulateMouseMove(80, 80)
        expect(sample(testRoot, "both-hover", 20, 20)).toEqual(GREEN)
        testRoot.renderer.nativeSimulateMouseDown(80, 80)
        expect(sample(testRoot, "both-active", 20, 20)).toEqual(BLUE)
      }
    )
  })

  it("applies after hover, and before the element's own active", () => {
    const tree = card(
      { group: true },
      {
        hover: { backgroundColor: "#00ff00" },
        groupActive: { backgroundColor: "#0000ff" },
        active: { backgroundColor: "#ffff00" },
      }
    )
    withRoot(tree, (testRoot) => {
      // Pressed on the group, off the child: groupActive only.
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(sample(testRoot, "active-order-hover", 20, 20)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      testRoot.renderer.nativeSimulateMouseDown(80, 80)
      expect(sample(testRoot, "active-order-group", 20, 20)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseUp(80, 80)
      // Pressed on the child: its own active wins over groupActive.
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      testRoot.renderer.nativeSimulateMouseDown(20, 20)
      expect(sample(testRoot, "active-order-own", 20, 20)).toEqual([255, 255, 0, 255])
    })
  })

  it("ends with the press", () => {
    withRoot(card({ group: true }, { groupActive: { backgroundColor: "#0000ff" } }), (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      testRoot.renderer.nativeSimulateMouseDown(80, 80)
      expect(sample(testRoot, "release-down", 20, 20)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseUp(80, 80)
      expect(sample(testRoot, "release-up", 20, 20)).toEqual(RED)
    })
  })

  it("a groupActive's color reaches the descendants' currentColor", () => {
    const tree = (
      <div style={{ width: 100, height: 100, backgroundColor: "#ffffff", group: true }}>
        <div style={{ color: "#00ff00", groupActive: { color: "#0000ff" } }}>
          <div style={{ width: 40, height: 40, backgroundColor: "currentColor" }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "active-color-off", 20, 20)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseDown(80, 80)
      expect(sample(testRoot, "active-color-on", 20, 20)).toEqual(BLUE)
    })
  })
})

describeNative("groupFocusWithin", () => {
  const widthOf = (testRoot: TestRoot, testId: string): number => {
    testRoot.renderer.dispatchNativeEvents()
    return testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId(testId).id).width
  }
  const focus = (testRoot: TestRoot, testId: string) =>
    testRoot.renderer.focusElement(testRoot.renderer.findByTestId(testId).id)
  const blur = (testRoot: TestRoot, testId: string) =>
    testRoot.renderer.blurElement(testRoot.renderer.findByTestId(testId).id)

  it("applies to descendants while an input inside the group is focused, and clears on blur", () => {
    const tree = (
      <div style={{ group: true, width: 200, height: 100 }}>
        <input testId="field" style={{ width: 100, height: 20 }} />
        <div testId="follower" style={{ width: 40, height: 20, groupFocusWithin: { width: 80 } }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      expect(widthOf(testRoot, "follower")).toBe(40)
      focus(testRoot, "field")
      expect(widthOf(testRoot, "follower")).toBe(80)
      blur(testRoot, "field")
      expect(widthOf(testRoot, "follower")).toBe(40)
      // A click moves focus the same way a script does.
      testRoot.renderer.nativeSimulateMouseDown(10, 10)
      testRoot.renderer.nativeSimulateMouseUp(10, 10)
      expect(widthOf(testRoot, "follower")).toBe(80)
      testRoot.renderer.nativeSimulateMouseDown(150, 80)
      testRoot.renderer.nativeSimulateMouseUp(150, 80)
      expect(widthOf(testRoot, "follower")).toBe(40)
    })
  })

  it("does not apply while focus is outside the group", () => {
    const tree = (
      <div>
        <div style={{ group: true, width: 200, height: 50 }}>
          <div testId="follower" style={{ width: 40, height: 20, groupFocusWithin: { width: 80 } }} />
        </div>
        <input testId="outside" style={{ width: 100, height: 20 }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      focus(testRoot, "outside")
      expect(widthOf(testRoot, "follower")).toBe(40)
    })
  })

  it("follows the nearest group by name", () => {
    const tree = (
      <div style={{ group: "outer", width: 300, height: 200 }}>
        <input testId="outer-field" style={{ width: 100, height: 20 }} />
        <div style={{ group: "inner", width: 200, height: 100 }}>
          <input testId="inner-field" style={{ width: 100, height: 20 }} />
          <div testId="inner-follower" style={{ width: 10, height: 20, groupFocusWithin: { group: "inner", width: 20 } }} />
          <div testId="outer-follower" style={{ width: 10, height: 20, groupFocusWithin: { group: "outer", width: 30 } }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      focus(testRoot, "outer-field")
      // The outer input is outside "inner", but inside "outer".
      expect([widthOf(testRoot, "inner-follower"), widthOf(testRoot, "outer-follower")]).toEqual([10, 30])
      focus(testRoot, "inner-field")
      expect([widthOf(testRoot, "inner-follower"), widthOf(testRoot, "outer-follower")]).toEqual([20, 30])
    })
  })

  it("applies after the element's own hover and before its focusWithin", () => {
    const tree = (
      <div style={{ group: true, width: 200, height: 100 }}>
        <input testId="field" style={{ width: 100, height: 20 }} />
        <div
          testId="follower"
          style={{
            width: 10,
            height: 20,
            hover: { width: 20, height: 40 },
            groupFocusWithin: { width: 30, height: 50, opacity: 0.5 },
            focusWithin: { height: 60 },
          }}
        />
      </div>
    )
    withRoot(tree, (testRoot) => {
      const size = () => {
        testRoot.renderer.dispatchNativeEvents()
        const bounds = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("follower").id)
        return [bounds.width, bounds.height]
      }
      expect(size()).toEqual([10, 20])
      focus(testRoot, "field")
      // The follower is not focused itself, so its own `focusWithin` does not apply.
      expect(size()).toEqual([30, 50])
    })
  })

  it("restyles a descendant's color through currentColor", () => {
    const tree = (
      <div style={{ group: true, width: 100, height: 100, backgroundColor: "#ffffff" }}>
        <input testId="field" style={{ width: 100, height: 20, opacity: 0 }} />
        <div style={{ color: "#00ff00", groupFocusWithin: { color: "#0000ff" } }}>
          <div style={{ width: 40, height: 40, backgroundColor: "currentColor" }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      expect(sample(testRoot, "focus-color-off", 20, 40)).toEqual(GREEN)
      focus(testRoot, "field")
      expect(sample(testRoot, "focus-color-on", 20, 40)).toEqual(BLUE)
    })
  })

  it("runs a transition declared in it", () => {
    const tree = (
      <div style={{ group: true, width: 200, height: 100 }}>
        <input testId="field" style={{ width: 100, height: 20 }} />
        <div
          testId="follower"
          style={{
            width: 40,
            height: 20,
            groupFocusWithin: { width: 80, transition: "width 1s linear" },
          }}
        />
      </div>
    )
    const testRoot = createTestRoot()
    try {
      testRoot.renderer.clockPause()
      testRoot.render(tree)
      focus(testRoot, "field")
      testRoot.renderer.clockFastForward(500)
      expect(widthOf(testRoot, "follower")).toBeCloseTo(60, 0)
      testRoot.renderer.clockFastForward(1000)
      expect(widthOf(testRoot, "follower")).toBe(80)
    } finally {
      testRoot.unmount()
    }
  })
})
