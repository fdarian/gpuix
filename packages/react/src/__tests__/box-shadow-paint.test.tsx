import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function capture(name: string, tree: ReactElement) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    const file = `${SHOTS_DIR}/box-shadow-paint-${name}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    return readLogicalPixels(file, testRoot.renderer)
  } finally {
    testRoot.unmount()
  }
}

const EDGE = 60
const SIZE = 120

/** A white 120x120 box on a white page, its left edge at x=60 and its
 *  vertical middle at y=120, so a black shadow reads as grey. */
function boxed(boxShadow: string) {
  return (
    <div style={{ padding: EDGE, backgroundColor: "#ffffff" }}>
      <div style={{ width: SIZE, height: SIZE, backgroundColor: "#ffffff", boxShadow }} />
    </div>
  )
}

/** Shadow coverage of the black shadow at `distance` logical px outside the
 *  box's left edge (negative: inside it), sampled at a pixel centre. */
function coverageAt(pixels: ReturnType<typeof readLogicalPixels>, distance: number) {
  const x = distance >= 0 ? EDGE - distance - 1 : EDGE - distance
  return 1 - pixels.at(x, EDGE + SIZE / 2)[0] / 255
}

describeNative("boxShadow painting", () => {
  it("paints the first layer on top, as CSS does", () => {
    const pixels = capture("order", boxed("0 0 0 10px #ff0000, 0 0 0 10px #00ff00"))
    expect(pixels.at(EDGE - 5, 120)).toEqual([255, 0, 0, 255])
    const reversed = capture("order-reversed", boxed("0 0 0 10px #00ff00, 0 0 0 10px #ff0000"))
    expect(reversed.at(EDGE - 5, 120)).toEqual([0, 255, 0, 255])
  })

  it("keeps a layer's own blur and offset when reordering", () => {
    // The sharp green ring lies over the larger red one.
    const pixels = capture("order-sizes", boxed("0 0 0 10px #00ff00, 0 0 0 20px #ff0000"))
    expect(pixels.at(EDGE - 5, 120)).toEqual([0, 255, 0, 255])
    expect(pixels.at(EDGE - 15, 120)).toEqual([255, 0, 0, 255])
  })
})
