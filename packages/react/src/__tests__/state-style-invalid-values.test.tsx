import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]

type TestRoot = ReturnType<typeof createTestRoot>

function sample(testRoot: TestRoot, name: string, x: number, y: number): Rgba {
  const file = `${SHOTS_DIR}/state-invalid-${name}.png`
  fs.rmSync(file, { force: true })
  testRoot.renderer.captureScreenshot(file)
  return readLogicalPixels(file, testRoot.renderer).at(x, y)
}

function withHovered(tree: ReactElement, run: (testRoot: TestRoot) => void) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    // Over the box at (30, 30)..(130, 130).
    testRoot.renderer.nativeSimulateMouseMove(80, 80)
    run(testRoot)
  } finally {
    testRoot.unmount()
  }
}

// An invalid declaration is dropped when CSS parses it, so a state style that
// gets a value wrong leaves the base value in place instead of replacing it.
describeNative("an invalid state style value", () => {
  it("keeps the base background", () => {
    const tree = (
      <div style={{ marginLeft: 30, marginTop: 30, width: 100, height: 100, backgroundColor: "#ff0000", hover: { background: "linear-gradient(blue" } }} />
    )
    withHovered(tree, (testRoot) => {
      expect(sample(testRoot, "background", 80, 80)).toEqual(RED)
    })
  })

  it("keeps the base box shadow", () => {
    const tree = (
      <div
        style={{
          marginLeft: 30,
          marginTop: 30,
          width: 100,
          height: 100,
          boxShadow: "0 0 0 10px #0000ff",
          hover: { boxShadow: "4 4 red" },
        }}
      />
    )
    withHovered(tree, (testRoot) => {
      // The ring is outside the box, 5px from its left edge.
      expect(sample(testRoot, "shadow", 25, 80)).toEqual(BLUE)
    })
  })

  it("still lets a valid state value replace the base", () => {
    const tree = (
      <div style={{ marginLeft: 30, marginTop: 30, width: 100, height: 100, backgroundColor: "#ff0000", hover: { background: "#0000ff" } }} />
    )
    withHovered(tree, (testRoot) => {
      expect(sample(testRoot, "valid", 80, 80)).toEqual(BLUE)
    })
  })
})
