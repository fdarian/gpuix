/// The tiles of a `backgroundImage` are cut to the part of the window that shows, and the tiles are in the box's own space:
/// a box wider than the window that a translation moves, or a scale or a rotation, shows the tiles the transform puts in the
/// window, not the ones layout would have put there.

import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]

const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const WHITE: Rgba = [255, 255, 255, 255]

/** A 50 x 50 image, red on its left half and blue on its right. */
const TILE = `url(data:image/svg+xml;base64,${Buffer.from(
  `<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50"><rect width="25" height="50" fill="#ff0000"/><rect x="25" width="25" height="50" fill="#0000ff"/></svg>`
).toString("base64")})`

let counter = 0

function paint(tree: ReactElement): (x: number, y: number) => Rgba {
  const testRoot = createTestRoot({ width: 320, height: 240 })
  try {
    testRoot.render(tree)
    testRoot.renderer.dispatchNativeEvents()
    const file = `${SHOTS_DIR}/background-image-in-transform-${counter++}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    const pixels = readLogicalPixels(file, testRoot.renderer)
    return (x, y) => pixels.at(x, y)
  } finally {
    testRoot.unmount()
  }
}

function near(actual: Rgba, expected: Rgba) {
  for (let i = 0; i < 4; i++) {
    expect(Math.abs(actual[i]! - expected[i]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(6)
  }
}

/** A 1000 x 100 box of the tile, laid out at `left`, `transform` on it, in a white window with a clipping box around it. */
function stage(left: number, transform: string, clipped: boolean) {
  const box = (
    <div
      style={{
        position: "absolute",
        left,
        top: 0,
        width: 1000,
        height: 100,
        backgroundImage: TILE,
        transformOrigin: "0 0",
        transform,
      }}
    />
  )
  return (
    <div style={{ width: 320, height: 240, backgroundColor: "#ffffff", position: "relative" }}>
      {clipped ? <div style={{ position: "relative", width: 320, height: 100, overflow: "hidden" }}>{box}</div> : box}
    </div>
  )
}

describeNative("backgroundImage in a transform", () => {
  for (const clipped of [false, true]) {
    const where = clipped ? "in a clipping box" : "unclipped"

    it(`shows the tiles a translation puts in the window, laid out in it (${where})`, () => {
      const at = paint(stage(0, "translateX(-300px)", clipped))
      // Layout x 300..350 is the tile at the window's left edge; the window ends at layout x 620.
      near(at(10, 10), RED)
      near(at(30, 10), BLUE)
      near(at(60, 10), RED)
      near(at(310, 10), RED)
      near(at(10, 110), WHITE)
    })

    it(`shows them when it was laid out outside the window and the translation brings it in (${where})`, () => {
      const at = paint(stage(400, "translateX(-400px)", clipped))
      near(at(10, 10), RED)
      near(at(30, 10), BLUE)
      near(at(310, 10), RED)
    })

    it(`shows them when it was laid out left of the window and the translation brings it in (${where})`, () => {
      const at = paint(stage(-500, "translateX(500px)", clipped))
      near(at(10, 10), RED)
      near(at(30, 10), BLUE)
      near(at(310, 10), RED)
    })

    it(`covers the window under a scale up and a scale down (${where})`, () => {
      const up = paint(stage(0, "scale(2)", clipped))
      // Tiles are 100 wide, red for the first 50.
      near(up(10, 10), RED)
      near(up(70, 10), BLUE)
      near(up(210, 10), RED)
      near(up(270, 10), BLUE)
      const down = paint(stage(0, "scale(0.5)", clipped))
      // Tiles are 25 wide, 1000 of layout is 500 of window.
      near(down(5, 10), RED)
      near(down(19, 10), BLUE)
      near(down(310, 10), RED)
      near(down(10, 60), WHITE)
    })

    it(`covers the window under a rotation (${where})`, () => {
      // A quarter turn about the top left: the box runs down the window's left edge, x from -100 to 0, so move it right.
      const at = paint(stage(0, "translateX(100px) rotate(90deg)", clipped))
      near(at(70, 10), RED)
      near(at(70, 30), BLUE)
      near(at(70, 60), RED)
      near(at(70, 90), BLUE)
      near(at(120, 10), WHITE)
    })
  }
})
