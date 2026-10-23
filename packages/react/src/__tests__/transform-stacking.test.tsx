/// A transform makes an element a stacking context, painted in the layer of `z-index: 0`, and the
/// containing block of its absolute and fixed descendants, as CSS has it.

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

function paint(name: string, tree: ReactElement, run: (at: (x: number, y: number) => Rgba) => void) {
  const testRoot = createTestRoot({ width: 400, height: 400 })
  try {
    testRoot.render(<div style={{ width: 400, height: 400, backgroundColor: "#ffffff" }}>{tree}</div>)
    const file = `${SHOTS_DIR}/transform-stacking-${process.pid}-${name}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.flush()
    testRoot.renderer.captureScreenshot(file)
    run(readLogicalPixels(file, testRoot.renderer).at)
  } finally {
    testRoot.unmount()
  }
}

describeNative("a transform is a stacking context", () => {
  /** A red box and a later blue one over its lower half. */
  const overlap = (red: Record<string, unknown>) => (
    <div>
      <div style={{ width: 100, height: 100, backgroundColor: "#ff0000", ...red }} />
      <div style={{ width: 100, height: 100, marginTop: -50, backgroundColor: "#0000ff" }} />
    </div>
  )

  it("paints in the layer of positioned boxes, over a later box in the flow", () => {
    paint("order-plain", overlap({}), (at) => {
      expect(at(50, 75)).toEqual(BLUE)
    })
    paint("order-transformed", overlap({ transform: "translateX(0px)" }), (at) => {
      expect(at(50, 75)).toEqual(RED)
      expect(at(50, 125)).toEqual(BLUE)
    })
  })

  it("does the same for each of the individual properties, an identity value included", () => {
    for (const style of [{ translate: "0px" }, { rotate: "0deg" }, { scale: 1 }, { transform: "translateZ(0)" }]) {
      paint(`order-${Object.keys(style)[0]}`, overlap(style), (at) => {
        expect(at(50, 75), JSON.stringify(style)).toEqual(RED)
      })
    }
  })

  it("does nothing of the kind for none, or for a value that is not valid", () => {
    for (const style of [
      { transform: "none" },
      { transform: "bogus(1)" },
      { translate: "none", rotate: "none", scale: "none" },
    ]) {
      paint(`order-no-${Object.keys(style)[0]}`, overlap(style), (at) => {
        expect(at(50, 75), JSON.stringify(style)).toEqual(BLUE)
      })
    }
  })

  it("holds a child with a negative z-index above its own background", () => {
    const parent = (left: number, style: Record<string, unknown>) => (
      <div style={{ position: "absolute", left, top: 0, width: 100, height: 100, backgroundColor: "#ff0000", ...style }}>
        <div style={{ position: "absolute", left: 20, top: 20, width: 40, height: 40, zIndex: -1, backgroundColor: "#0000ff" }} />
      </div>
    )
    paint(
      "negative",
      <>
        {parent(0, {})}
        {parent(200, { transform: "translateX(0px)" })}
      </>,
      (at) => {
        // The first box is positioned, but no context: its child goes behind the background of the box.
        expect(at(40, 40)).toEqual(RED)
        // The second one is, so the child stays inside it, over the background.
        expect(at(240, 40)).toEqual(BLUE)
      }
    )
  })
})

describeNative("a transform is a containing block", () => {
  /** A static box at (100, 100), in a flex container so that its margin stays its own. */
  const parent = (style: Record<string, unknown>, child: Record<string, unknown>) => (
    <div style={{ display: "flex" }}>
      <div style={{ marginLeft: 100, marginTop: 100, width: 100, height: 100, backgroundColor: "#ff0000", ...style }}>
        <div style={{ left: 5, top: 5, width: 20, height: 20, backgroundColor: "#0000ff", ...child }} />
      </div>
    </div>
  )

  it("places a fixed child against the box, and moves it with the box", () => {
    const fixed = { position: "fixed" }
    paint("fixed-plain", parent({}, fixed), (at) => {
      expect(at(10, 10)).toEqual(BLUE)
      expect(at(115, 115)).toEqual(RED)
    })
    paint("fixed-transformed", parent({ transform: "translate(10px, 10px)" }, fixed), (at) => {
      expect(at(10, 10)).toEqual(WHITE)
      // The box is at (100, 100) and drawn 10px further; the child is 5px inside it and goes along.
      expect(at(110, 110)).toEqual(RED)
      expect(at(120, 120)).toEqual(BLUE)
      expect(at(134, 134)).toEqual(BLUE)
      expect(at(136, 136)).toEqual(RED)
    })
    for (const style of [{ translate: "10px 10px" }, { rotate: "0deg" }, { scale: 1 }]) {
      paint(`fixed-${Object.keys(style)[0]}`, parent(style, fixed), (at) => {
        expect(at(10, 10), JSON.stringify(style)).toEqual(WHITE)
      })
    }
  })

  it("places an absolute child against a box that has no position", () => {
    const absolute = { position: "absolute" }
    paint("absolute-plain", parent({}, absolute), (at) => {
      expect(at(10, 10)).toEqual(BLUE)
    })
    paint("absolute-transformed", parent({ transform: "translate(10px, 10px)" }, absolute), (at) => {
      expect(at(10, 10)).toEqual(WHITE)
      expect(at(120, 120)).toEqual(BLUE)
    })
  })

  it("keeps a box with `transform: none` out of it", () => {
    paint("fixed-none", parent({ transform: "none" }, { position: "fixed" }), (at) => {
      expect(at(10, 10)).toEqual(BLUE)
    })
  })
})
