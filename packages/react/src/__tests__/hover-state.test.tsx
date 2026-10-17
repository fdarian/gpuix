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
  const file = `${SHOTS_DIR}/hover-state-${name}.png`
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

const TWO_TONE =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><rect width="20" height="20" fill="currentColor"/></svg>'

describeNative("hover color and currentColor", () => {
  it("reaches a descendant's background, border and svg", () => {
    const tree = (
      <div style={{ width: 300, height: 100, display: "flex", color: "#00ff00", hover: { color: "#0000ff" } }}>
        <div style={{ width: 50, height: 50, backgroundColor: "currentColor" }} />
        <div style={{ width: 50, height: 50, borderWidth: 10, borderStyle: "solid", borderColor: "currentColor", boxSizing: "border-box" }} />
        <svg source={TWO_TONE} style={{ width: 50, height: 50 }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(sample(testRoot, "off", 25, 25)).toEqual(GREEN)
      expect(sample(testRoot, "off", 55, 25)).toEqual(GREEN)
      // Over the parent's own box, off every child.
      testRoot.renderer.nativeSimulateMouseMove(250, 80)
      expect(sample(testRoot, "on", 25, 25)).toEqual(BLUE)
      expect(sample(testRoot, "on", 55, 25)).toEqual(BLUE)
      expect(sample(testRoot, "on", 125, 25)).toEqual(BLUE)
    })
  })

  it("reaches a grandchild through an element that sets no color", () => {
    const tree = (
      <div style={{ width: 100, height: 100, color: "#00ff00", hover: { color: "#0000ff" } }}>
        <div>
          <div style={{ width: 40, height: 40, backgroundColor: "currentColor" }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "grandchild", 20, 20)).toEqual(BLUE)
    })
  })

  it("a descendant's own color is not overridden by the hovered parent's", () => {
    const tree = (
      <div style={{ width: 100, height: 100, color: "#00ff00", hover: { color: "#0000ff" } }}>
        <div style={{ width: 40, height: 40, color: "#ff0000", backgroundColor: "currentColor" }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(80, 80)
      expect(sample(testRoot, "own-color", 20, 20)).toEqual(RED)
    })
  })
})

describeNative("a filled or positioned child does not block its parent's hover", () => {
  it("hover", () => {
    const tree = (
      <div
        style={{
          position: "relative",
          width: 100,
          height: 100,
          backgroundColor: "#ff0000",
          hover: { backgroundColor: "#0000ff" },
        }}
      >
        <div style={{ position: "absolute", left: 20, top: 20, width: 40, height: 40, backgroundColor: "#00ff00" }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      // Over the filled, absolutely positioned child.
      testRoot.renderer.nativeSimulateMouseMove(40, 40)
      expect(sample(testRoot, "filled-parent", 90, 90)).toEqual(BLUE)
      expect(sample(testRoot, "filled-parent-child", 40, 40)).toEqual(GREEN)
    })
  })
})

describeNative("a state style that moves the element out from under the pointer", () => {
  const left = (testRoot: TestRoot, testId: string) =>
    testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId(testId).id).x

  // A window resyncs hover once per frame, so the element flips once per frame; the test renderer
  // has no display link, so a flush is the frame. Resyncing inside the flush that drew would never
  // return.
  it("hover flips once per flush", () => {
    const tree = <div testId="box" style={{ width: 100, height: 100, hover: { marginLeft: 200 } }} />
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const moved = left(testRoot, "box")
      expect(moved).toBe(200)
      for (let flush = 0; flush < 4; flush++) {
        testRoot.renderer.flush()
        expect(left(testRoot, "box")).toBe(flush % 2 === 0 ? 0 : 200)
      }
    })
  })

  it("hasHover on a parent flips once per flush", () => {
    const tree = (
      <div testId="box" style={{ width: 100, height: 100, hasHover: { marginLeft: 200 } }}>
        <div style={{ width: 40, height: 40 }} />
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(20, 20)
      expect(left(testRoot, "box")).toBe(200)
      for (let flush = 0; flush < 4; flush++) {
        testRoot.renderer.flush()
        expect(left(testRoot, "box")).toBe(flush % 2 === 0 ? 0 : 200)
      }
    })
  })

  it("a flush that changes nothing under the pointer leaves hover where it is", () => {
    const tree = <div testId="box" style={{ width: 100, height: 100, hover: { marginLeft: 20 } }} />
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      for (let flush = 0; flush < 3; flush++) {
        testRoot.renderer.flush()
        expect(left(testRoot, "box")).toBe(20)
      }
    })
  })
})
