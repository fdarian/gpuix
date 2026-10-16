import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const WHITE: Rgba = [255, 255, 255, 255]

let counter = 0

/** A 100x100 box with `style`, painted; returns a pixel sampler. */
function paint(style: Record<string, unknown>, wrap?: (box: ReactElement) => ReactElement) {
  const testRoot = createTestRoot()
  try {
    const box = <div style={{ width: 100, height: 100, ...style }} />
    testRoot.render(wrap ? wrap(box) : box)
    const file = `${SHOTS_DIR}/background-layers-${counter++}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    const pixels = readLogicalPixels(file, testRoot.renderer)
    return (x: number, y: number) => pixels.at(x, y)
  } finally {
    testRoot.unmount()
  }
}

function near(actual: Rgba, expected: Rgba, tolerance = 6) {
  for (let i = 0; i < 4; i++) {
    expect(Math.abs(actual[i]! - expected[i]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(tolerance)
  }
}

describeNative("gradients in backgroundImage", () => {
  it("paints a linear gradient left to right", () => {
    const at = paint({ backgroundImage: "linear-gradient(to right, red, blue)" })
    near(at(2, 50), [250, 0, 5, 255], 8)
    near(at(97, 50), [5, 0, 250, 255], 8)
    near(at(50, 50), [128, 0, 128, 255], 8)
  })

  it("takes more than two stops and positions", () => {
    const at = paint({ backgroundImage: "linear-gradient(to right, red 0%, #00ff00 50%, blue 100%)" })
    near(at(50, 50), GREEN, 8)
    const hard = paint({ backgroundImage: "linear-gradient(to right, red 0 50%, blue 50% 100%)" })
    near(hard(25, 50), RED)
    near(hard(75, 50), BLUE)
  })

  it("honours angles and corner keywords", () => {
    const up = paint({ backgroundImage: "linear-gradient(0deg, red 0 50%, blue 50%)" })
    near(up(50, 25), BLUE)
    near(up(50, 75), RED)
    const corner = paint({ backgroundImage: "linear-gradient(to bottom right, red 0 50%, blue 50%)" })
    near(corner(10, 10), RED)
    near(corner(90, 90), BLUE)
  })

  it("moves the midpoint with a colour hint", () => {
    const plain = paint({ backgroundImage: "linear-gradient(to right, red, blue)" })(50, 50)
    const hinted = paint({ backgroundImage: "linear-gradient(to right, red, 80%, blue)" })(50, 50)
    // The hint pushes the 50% point towards red.
    expect(hinted[0]).toBeGreaterThan(plain[0] + 20)
  })

  it("interpolates in oklab when asked", () => {
    const srgb = paint({ backgroundImage: "linear-gradient(to right, red, blue)" })(50, 50)
    const oklab = paint({ backgroundImage: "linear-gradient(to right in oklab, red, blue)" })(50, 50)
    expect(Math.abs(srgb[0] - oklab[0]) + Math.abs(srgb[2] - oklab[2])).toBeGreaterThan(20)
  })

  it("paints radial gradients from their centre", () => {
    const at = paint({ backgroundImage: "radial-gradient(circle closest-side at center, red, blue)" })
    near(at(50, 50), RED, 12)
    near(at(2, 2), BLUE, 8)
    const off = paint({ backgroundImage: "radial-gradient(circle closest-side at 20px 20px, red, blue)" })
    expect(off(20, 20)[0]).toBeGreaterThan(off(80, 80)[0])
  })

  it("paints conic gradients clockwise from the top", () => {
    const at = paint({
      backgroundImage: "conic-gradient(red 0 25%, blue 25% 50%, #00ff00 50% 75%, white 75%)",
    })
    near(at(75, 25), RED)
    near(at(75, 75), BLUE)
    near(at(25, 75), GREEN)
    near(at(25, 25), WHITE)
  })

  it("repeats a repeating gradient", () => {
    const at = paint({ backgroundImage: "repeating-linear-gradient(to right, red 0 10px, blue 10px 20px)" })
    near(at(5, 50), RED)
    near(at(15, 50), BLUE)
    near(at(25, 50), RED)
    near(at(35, 50), BLUE)
  })

  // CSS: a repeating cycle of no length is the average colour of its stops. Here
  // 10px is 5% of the 200px line, so only the wider box has a cycle.
  it("paints the average colour when a repeating cycle in mixed units has no length", () => {
    const image = "repeating-linear-gradient(to right, red 10px, blue 5%)"
    const empty = paint({ width: 200, backgroundImage: image })
    near(empty(5, 50), [128, 0, 128, 255])
    near(empty(150, 50), [128, 0, 128, 255])
    const cycling = paint({ width: 400, backgroundImage: image })
    // Red to blue over 10px, from x=10: x=2 is 20% of the way, x=7 is 70%.
    near(cycling(2, 50), [204, 0, 51, 255], 14)
    near(cycling(7, 50), [77, 0, 178, 255], 14)
    near(cycling(12, 50), cycling(2, 50), 14)
  })

  it("resolves colour functions and currentColor in stops", () => {
    const at = paint(
      {
        color: "#00ff00",
        backgroundImage: "linear-gradient(currentColor, light-dark(#00ff00, red))",
      }
    )
    near(at(50, 50), GREEN)
  })
})

describeNative("layers: size, position, repeat, origin, clip", () => {
  const solid = (color: string) => `linear-gradient(${color}, ${color})`

  it("puts the first layer on top and lets the colour show through", () => {
    near(paint({ backgroundImage: `${solid("red")}, ${solid("blue")}` })(50, 50), RED)
    near(paint({ backgroundColor: "#00ff00", backgroundImage: "linear-gradient(transparent, transparent)" })(50, 50), GREEN)
    const over = paint({
      backgroundColor: "#0000ff",
      backgroundImage: "linear-gradient(to right, red 0 50%, transparent 50%)",
    })
    near(over(25, 50), RED)
    near(over(75, 50), BLUE)
  })

  it("tiles by default and stops with no-repeat", () => {
    const tiled = paint({ backgroundImage: `${solid("red")}`, backgroundSize: "20px 20px", backgroundColor: "#ffffff" })
    near(tiled(10, 10), RED)
    near(tiled(70, 70), RED)
    const once = paint({
      backgroundImage: solid("red"),
      backgroundSize: "20px 20px",
      backgroundRepeat: "no-repeat",
      backgroundColor: "#ffffff",
    })
    near(once(10, 10), RED)
    near(once(70, 70), WHITE)
  })

  it("positions a tile, including from the far edge", () => {
    const base = {
      backgroundImage: solid("red"),
      backgroundSize: "20px 20px",
      backgroundRepeat: "no-repeat",
      backgroundColor: "#ffffff",
    }
    near(paint({ ...base, backgroundPosition: "right bottom" })(90, 90), RED)
    near(paint({ ...base, backgroundPosition: "right bottom" })(10, 10), WHITE)
    near(paint({ ...base, backgroundPosition: "right 10px bottom 10px" })(80, 80), RED)
    near(paint({ ...base, backgroundPosition: "right 10px bottom 10px" })(95, 95), WHITE)
    near(paint({ ...base, backgroundPosition: "center" })(50, 50), RED)
    near(paint({ ...base, backgroundPosition: "50% 0" })(50, 5), RED)
  })

  it("spaces and rounds tiles", () => {
    const space = paint({
      backgroundImage: solid("red"),
      backgroundSize: "30px 100%",
      backgroundRepeat: "space no-repeat",
      backgroundColor: "#ffffff",
    })
    // Three whole 30px tiles with 5px gaps: 0-30, 35-65, 70-100.
    near(space(15, 50), RED)
    near(space(32, 50), WHITE)
    near(space(50, 50), RED)
    near(space(85, 50), RED)
  })

  it("keeps the layer lists aligned and cycles short ones", () => {
    const at = paint({
      backgroundImage: `${solid("red")}, ${solid("blue")}`,
      backgroundSize: "50px 100px",
      backgroundPosition: "left, right",
      backgroundRepeat: "no-repeat",
      backgroundColor: "#ffffff",
    })
    near(at(25, 50), RED)
    near(at(75, 50), BLUE)
  })

  it("clips to the padding or content box and positions in the origin box", () => {
    const framed = {
      padding: 10,
      borderWidth: 10,
      borderStyle: "solid",
      borderColor: "#00ff00",
      backgroundImage: solid("red"),
    }
    // Border box by default paints under the border too (the border is opaque here).
    const content = paint({ ...framed, backgroundClip: "content-box", backgroundColor: "#ffffff" })
    near(content(25, 25), RED)
    near(content(15, 15), WHITE)
    const padding = paint({ ...framed, backgroundClip: "padding-box", backgroundColor: "#ffffff" })
    near(padding(15, 15), RED)
    const origin = paint({
      ...framed,
      backgroundOrigin: "content-box",
      backgroundSize: "20px 20px",
      backgroundRepeat: "no-repeat",
      backgroundClip: "padding-box",
      backgroundColor: "#ffffff",
    })
    near(origin(25, 25), RED)
    near(origin(15, 15), WHITE)
  })

  it("backgroundClip text paints nothing where there are no glyphs", () => {
    near(paint({ backgroundImage: solid("red"), backgroundClip: "text" })(50, 50), [0, 0, 0, 255])
  })
})

describeNative("the background shorthand", () => {
  it("takes layers, position / size, repeat and a final colour", () => {
    const at = paint({ background: "linear-gradient(red, red) no-repeat 0 0 / 50% 100%, blue" })
    near(at(25, 50), RED)
    near(at(75, 50), BLUE)
  })

  it("keeps a plain colour and the object gradient working", () => {
    near(paint({ background: "#ff0000" })(50, 50), RED)
    near(paint({ background: "oklch(62.8% 0.258 29.2)" })(50, 50), RED, 4)
    const object = paint({
      background: {
        type: "linear-gradient",
        angle: 90,
        stops: [
          { color: "#ff0000", position: 0 },
          { color: "#0000ff", position: 1 },
        ],
      },
    })
    near(object(2, 50), [250, 0, 5, 255], 8)
    near(object(97, 50), [5, 0, 250, 255], 8)
  })

  it("lets backgroundColor win over the shorthand colour, and longhands win over layers", () => {
    near(paint({ background: "linear-gradient(red, red), blue", backgroundColor: "#00ff00", backgroundSize: "20px 20px", backgroundRepeat: "no-repeat" })(80, 80), GREEN)
    near(paint({ background: "linear-gradient(red, red)", backgroundImage: "linear-gradient(blue, blue)" })(50, 50), BLUE)
  })

  it("resolves currentColor in the final colour", () => {
    near(paint({ color: "#00ff00", background: "linear-gradient(transparent, transparent) currentColor" })(50, 50), GREEN)
  })
})

describeNative("ignored backgrounds", () => {
  const unset = () => paint({})(50, 50)

  it("ignores url(), none and invalid gradients", () => {
    for (const image of ["url(x.png)", "none", "linear-gradient(red)", "linear-gradient(to nowhere, red, blue)", "nonsense"]) {
      expect(paint({ backgroundImage: image })(50, 50)).toEqual(unset())
    }
  })

  it("skips an unsupported layer and keeps the rest", () => {
    near(paint({ backgroundImage: "url(x.png), linear-gradient(red, red)" })(50, 50), RED)
    near(paint({ backgroundImage: "linear-gradient(in oklch, blue, blue), linear-gradient(red, red)" })(50, 50), RED)
  })

  it("ignores an invalid longhand on its own", () => {
    near(paint({ backgroundImage: "linear-gradient(red, red)", backgroundSize: "nonsense" })(50, 50), RED)
    near(paint({ backgroundImage: "linear-gradient(red, red)", backgroundClip: "nonsense" })(50, 50), RED)
  })
})

describeNative("background layers in states", () => {
  it("applies backgroundImage in a hover state", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div style={{ width: 100, height: 100, hover: { backgroundImage: "linear-gradient(red, red)" } }} />
      )
      const before = `${SHOTS_DIR}/background-layers-hover-before.png`
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      testRoot.renderer.captureScreenshot(before)
      expect(readLogicalPixels(before, testRoot.renderer).at(50, 50)).not.toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const after = `${SHOTS_DIR}/background-layers-hover-after.png`
      testRoot.renderer.captureScreenshot(after)
      near(readLogicalPixels(after, testRoot.renderer).at(50, 50), RED)
    } finally {
      testRoot.unmount()
    }
  })
  it("backgroundImage: none in a hover state clears the base gradient", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div
          style={{
            width: 100,
            height: 100,
            backgroundImage: "linear-gradient(red, red)",
            hover: { backgroundImage: "none" },
          }}
        />
      )
      const before = `${SHOTS_DIR}/background-layers-hover-none-before.png`
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      testRoot.renderer.captureScreenshot(before)
      near(readLogicalPixels(before, testRoot.renderer).at(50, 50), RED)
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const after = `${SHOTS_DIR}/background-layers-hover-none-after.png`
      testRoot.renderer.captureScreenshot(after)
      expect(readLogicalPixels(after, testRoot.renderer).at(50, 50)).not.toEqual(RED)
    } finally {
      testRoot.unmount()
    }
  })

  it("a background shorthand in a hover state resets the base layers", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div
          style={{
            width: 100,
            height: 100,
            backgroundImage: "linear-gradient(red, red)",
            hover: { background: "blue" },
          }}
        />
      )
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const after = `${SHOTS_DIR}/background-layers-hover-shorthand-after.png`
      testRoot.renderer.captureScreenshot(after)
      near(readLogicalPixels(after, testRoot.renderer).at(50, 50), BLUE)
    } finally {
      testRoot.unmount()
    }
  })

  it("a hover backgroundColor leaves the base gradient layer on top", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div
          style={{
            width: 100,
            height: 100,
            backgroundImage: "linear-gradient(red, red)",
            hover: { backgroundColor: "blue" },
          }}
        />
      )
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const after = `${SHOTS_DIR}/background-layers-hover-color-after.png`
      testRoot.renderer.captureScreenshot(after)
      near(readLogicalPixels(after, testRoot.renderer).at(50, 50), RED)
    } finally {
      testRoot.unmount()
    }
  })
})
