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

  // CSS: the blur radius B is a gaussian with sigma = B / 2, so a straight edge
  // is 50% at the edge and falls to 1 - Phi(2) = 2.3% at B outside it.
  it("blurs a drop shadow over its radius, with a standard deviation of half of it", () => {
    const pixels = capture("blur", boxed("0 0 40px #000000"))
    expect(coverageAt(pixels, 0)).toBeGreaterThan(0.42)
    expect(coverageAt(pixels, 0)).toBeLessThan(0.58)
    // 1 - Phi(1) = 15.9%
    expect(coverageAt(pixels, 20)).toBeGreaterThan(0.1)
    expect(coverageAt(pixels, 20)).toBeLessThan(0.22)
    // 1 - Phi(2) = 2.3%
    expect(coverageAt(pixels, 40)).toBeLessThan(0.05)
    expect(coverageAt(pixels, 40)).toBeGreaterThan(0.005)
    expect(coverageAt(pixels, 50)).toBeLessThan(0.02)
  })

  it("moves the blurred edge by the spread", () => {
    const pixels = capture("blur-spread", boxed("0 0 40px 10px #000000"))
    expect(coverageAt(pixels, 10)).toBeGreaterThan(0.42)
    expect(coverageAt(pixels, 10)).toBeLessThan(0.58)
    expect(coverageAt(pixels, 50)).toBeLessThan(0.05)
  })

  it("blurs an inset shadow over its radius as well", () => {
    const pixels = capture("blur-inset", boxed("inset 0 0 40px #000000"))
    expect(coverageAt(pixels, -20)).toBeGreaterThan(0.1)
    expect(coverageAt(pixels, -20)).toBeLessThan(0.22)
    expect(coverageAt(pixels, -40)).toBeLessThan(0.05)
    expect(coverageAt(pixels, -40)).toBeGreaterThan(0.005)
  })

  // CSS Backgrounds 3 7.1.1: an outer shadow is clipped to the outside of the border box, so
  // a translucent fill shows the page and not the shadow under it.
  describe("under a translucent box", () => {
    function translucent(style: Record<string, unknown>) {
      return (
        <div style={{ padding: EDGE, backgroundColor: "#ffffff" }}>
          <div style={{ width: SIZE, height: SIZE, backgroundColor: "#ff000080", ...style }} />
        </div>
      )
    }
    const FILL = [255, 127, 127]
    const near = (actual: number[], expected: number[]) =>
      expect(actual.slice(0, 3).map((value, i) => Math.abs(value - expected[i]!))).toSatisfy(
        (diffs: number[]) => diffs.every((diff) => diff <= 2)
      )

    it("keeps the fill's colour where an offset shadow lies under the box", () => {
      const pixels = capture("translucent-offset", translucent({ boxShadow: "30px 30px 0 #000000" }))
      // Inside both the box and the shadow's rect.
      near(pixels.at(EDGE + SIZE - 10, EDGE + SIZE - 10), FILL)
      near(pixels.at(EDGE + SIZE / 2, EDGE + SIZE / 2), FILL)
      // The shadow still shows outside the box.
      expect(pixels.at(EDGE + SIZE + 10, EDGE + SIZE + 10)).toEqual([0, 0, 0, 255])
      expect(pixels.at(EDGE + SIZE + 10, EDGE + 40)).toEqual([0, 0, 0, 255])
    })

    it("keeps the fill's colour under a blurred shadow, and the shadow outside", () => {
      const pixels = capture("translucent-blur", translucent({ boxShadow: "0 0 30px 10px #000000" }))
      near(pixels.at(EDGE + 5, EDGE + SIZE / 2), FILL)
      near(pixels.at(EDGE + SIZE / 2, EDGE + 5), FILL)
      expect(coverageAt(pixels, 5)).toBeGreaterThan(0.5)
    })

    it("cuts out the box's rounded corners, not its bounding rectangle", () => {
      const rounded = translucent({ borderRadius: 50, boxShadow: "0 0 0 20px #000000" })
      const pixels = capture("translucent-rounded", rounded)
      // The corner of the bounding rectangle lies outside the rounded box.
      expect(pixels.at(EDGE + 3, EDGE + 3)).toEqual([0, 0, 0, 255])
      near(pixels.at(EDGE + SIZE / 2, EDGE + SIZE / 2), FILL)
      near(pixels.at(EDGE + 20, EDGE + SIZE / 2), FILL)
    })

    it("keeps an inset shadow inside the padding box, off a translucent border", () => {
      const pixels = capture(
        "translucent-border",
        translucent({
          backgroundColor: "#ffffff",
          border: "10px solid #0000ff80",
          boxShadow: "inset 0 0 0 5px #000000",
        })
      )
      // Blue at half opacity over the white fill; the shadow does not darken it.
      near(pixels.at(EDGE + 2, EDGE + SIZE / 2), [127, 127, 255])
      // The ring starts at the padding edge.
      expect(pixels.at(EDGE + 12, EDGE + SIZE / 2)).toEqual([0, 0, 0, 255])
      expect(pixels.at(EDGE + SIZE / 2, EDGE + SIZE / 2)).toEqual([255, 255, 255, 255])
    })
  })
})
