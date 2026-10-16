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
    const file = `${SHOTS_DIR}/border-style-paint-${name}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    return readLogicalPixels(file, testRoot.renderer)
  } finally {
    testRoot.unmount()
  }
}

const EDGE = 20
const SIZE = 100

/** A white 100x100 box on a white page, its top left corner at (20, 20). */
function boxed(style: Record<string, unknown>) {
  return (
    <div style={{ padding: EDGE, backgroundColor: "#ffffff" }}>
      <div style={{ width: SIZE, height: SIZE, boxSizing: "border-box", backgroundColor: "#ffffff", ...style }} />
    </div>
  )
}

const RED = [255, 0, 0, 255]
const WHITE = [255, 255, 255, 255]

function near(actual: number[], expected: number[], tolerance = 4) {
  expect(actual.slice(0, 3).map((value, i) => Math.abs(value - expected[i]!))).toSatisfy((diffs: number[]) =>
    diffs.every((diff) => diff <= tolerance)
  )
}

describeNative("border styles painting", () => {
  // 9px is 18 device px at 2x: lines of 6 with a gap of 6, and at 1x lines of 3 with a gap of 3.
  it("draws a double border as two lines with a gap between them", () => {
    const pixels = capture("double", boxed({ border: "9px double #ff0000" }))
    const x = EDGE + SIZE / 2
    expect(pixels.at(x, EDGE + 1)).toEqual(RED)
    expect(pixels.at(x, EDGE + 4)).toEqual(WHITE)
    expect(pixels.at(x, EDGE + 7)).toEqual(RED)
    expect(pixels.at(x, EDGE + 12)).toEqual(WHITE)
    // The left side is the same pattern.
    const y = EDGE + SIZE / 2
    expect(pixels.at(EDGE + 1, y)).toEqual(RED)
    expect(pixels.at(EDGE + 4, y)).toEqual(WHITE)
    expect(pixels.at(EDGE + 7, y)).toEqual(RED)
  })

  it("draws a dotted border as round dots one width across", () => {
    const pixels = capture("dotted", boxed({ border: "10px dotted #000000" }))
    // Dots are 10px across and 22.5px apart along the 100px top side, one of them at its middle.
    const middle = EDGE + SIZE / 2
    expect(pixels.at(middle, EDGE + 5)).toEqual([0, 0, 0, 255])
    // Between two dots there is the page.
    expect(pixels.at(middle - 11.25, EDGE + 5)).toEqual(WHITE)
    // A dot is round: the corner of its square is not painted.
    expect(pixels.at(middle - 6, EDGE + 1)).toEqual(WHITE)
    expect(pixels.at(middle, EDGE + 1)).toEqual([0, 0, 0, 255])
  })

  it("splits a groove into an inset outer half and an outset inner half", () => {
    const pixels = capture("groove", boxed({ border: "10px groove #808080" }))
    const x = EDGE + SIZE / 2
    // Top: the outer half is the darker one, the inner half the lighter.
    near(pixels.at(x, EDGE + 2), [44, 44, 44])
    near(pixels.at(x, EDGE + 7), [212, 212, 212])
    // Bottom: the other way round.
    near(pixels.at(x, EDGE + SIZE - 3), [212, 212, 212])
    near(pixels.at(x, EDGE + SIZE - 8), [44, 44, 44])
    // A ridge swaps the halves.
    const ridge = capture("ridge", boxed({ border: "10px ridge #808080" }))
    near(ridge.at(x, EDGE + 2), [212, 212, 212])
    near(ridge.at(x, EDGE + 7), [44, 44, 44])
  })

  it("shades the sides of an inset and an outset border", () => {
    const outset = capture("outset", boxed({ border: "6px outset #ff0000" }))
    const middle = EDGE + SIZE / 2
    expect(outset.at(middle, EDGE + 2)).toEqual(RED)
    expect(outset.at(EDGE + 2, middle)).toEqual(RED)
    near(outset.at(middle, EDGE + SIZE - 3), [171, 0, 0])
    near(outset.at(EDGE + SIZE - 3, middle), [171, 0, 0])

    const inset = capture("inset", boxed({ border: "6px inset #ff0000" }))
    near(inset.at(middle, EDGE + 2), [171, 0, 0])
    expect(inset.at(middle, EDGE + SIZE - 3)).toEqual(RED)
  })

  it("gives each side its own style", () => {
    const pixels = capture(
      "per-side",
      boxed({ border: "9px solid #ff0000", borderLeftStyle: "double", borderBottomStyle: "none" })
    )
    const middle = EDGE + SIZE / 2
    // Solid on the top and right.
    expect(pixels.at(middle, EDGE + 4)).toEqual(RED)
    expect(pixels.at(EDGE + SIZE - 5, middle)).toEqual(RED)
    // Double on the left, with its gap.
    expect(pixels.at(EDGE + 1, middle)).toEqual(RED)
    expect(pixels.at(EDGE + 4, middle)).toEqual(WHITE)
    expect(pixels.at(EDGE + 7, middle)).toEqual(RED)
    // None at the bottom: no border there, and no width taken.
    expect(pixels.at(middle, EDGE + SIZE - 3)).toEqual(WHITE)
  })
})
