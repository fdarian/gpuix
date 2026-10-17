/// `outline` and `outlineStyle`, as Chrome paints them: the style is `none` unless something
/// gives one, the width `medium` and the colour `currentColor`.

import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Style = Record<string, unknown>
type Rgba = [number, number, number, number]

/** Renders `tree`, lets `interact` act on it, and returns a sampler for logical pixels. */
function paint(name: string, tree: ReactElement, interact?: (renderer: TestRoot["renderer"]) => void) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    interact?.(testRoot.renderer)
    const file = `${SHOTS_DIR}/outline-shorthand-${name}.png`
    testRoot.renderer.captureScreenshot(file)
    return readLogicalPixels(file, testRoot.renderer)
  } finally {
    testRoot.unmount()
  }
}

const isBlue = ([r, g, b]: Rgba) => b > 200 && r < 60 && g < 60
const isRed = ([r, g, b]: Rgba) => r > 200 && g < 60 && b < 60
const isGreen = ([r, g, b]: Rgba) => g > 200 && r < 60 && b < 60

/** A 40x40 box at (30, 30), so its outline has room all round. */
function boxed(style: Style) {
  return (
    <div style={{ padding: 30 }}>
      <div testId="box" tabIndex={0} style={{ width: 40, height: 40, ...style }} />
    </div>
  )
}

/** The pixels of the ring along the top of the box, between its corners. */
function topStrip(shot: ReturnType<typeof paint>, y: number, test: (pixel: Rgba) => boolean) {
  const painted: boolean[] = []
  for (let x = 36; x < 64; x++) painted.push(test(shot.at(x, y)))
  return painted
}

describeNative("outline shorthand", () => {
  it("paints a ring just outside the border box: 6px solid blue is 24 to 30 on the left", () => {
    const shot = paint("solid", boxed({ outline: "6px solid #0000ff" }))
    expect(isBlue(shot.at(27, 50))).toBe(true)
    expect(isBlue(shot.at(24, 50))).toBe(true)
    expect(isBlue(shot.at(23, 50))).toBe(false)
    expect(isBlue(shot.at(33, 50)), "inside the box").toBe(false)
  })

  it("takes the width, style and colour in any order", () => {
    for (const [index, outline] of ["6px solid #0000ff", "#0000ff solid 6px", "solid 6px #0000ff", "#0000ff 6px solid"].entries()) {
      const shot = paint(`order-${index}`, boxed({ outline }))
      expect(isBlue(shot.at(27, 50)), outline).toBe(true)
    }
  })

  it("is medium wide when it names no width: 3px", () => {
    const shot = paint("medium", boxed({ outline: "solid #0000ff" }))
    expect(isBlue(shot.at(28, 50))).toBe(true)
    expect(isBlue(shot.at(26, 50))).toBe(false)
  })

  it("is currentColor when it names no colour", () => {
    const shot = paint("current", boxed({ color: "#ff0000", outline: "6px solid" }))
    expect(isRed(shot.at(27, 50))).toBe(true)
  })

  it("paints nothing when it names no style, as the style is none", () => {
    for (const [index, style] of [
      { outline: "6px #0000ff" },
      { outline: "6px none #0000ff" },
      { outlineWidth: 6, outlineColor: "#0000ff" },
      { outline: "6px solid #0000ff", outlineStyle: "none" },
    ].entries()) {
      const shot = paint(`none-${index}`, boxed(style))
      expect(isBlue(shot.at(27, 50)), JSON.stringify(style)).toBe(false)
    }
  })

  it("takes the longhands, which win over it", () => {
    const longhands = paint("longhands", boxed({ outlineWidth: 6, outlineStyle: "solid", outlineColor: "#0000ff" }))
    expect(isBlue(longhands.at(27, 50))).toBe(true)
    const winning = paint("winning", boxed({ outline: "2px dashed #ff0000", outlineWidth: 6, outlineStyle: "solid", outlineColor: "#0000ff" }))
    expect(isBlue(winning.at(27, 50))).toBe(true)
    expect(isRed(winning.at(27, 50))).toBe(false)
  })

  it("moves out by outlineOffset, which the shorthand leaves alone", () => {
    const shot = paint("offset", boxed({ outline: "6px solid #0000ff", outlineOffset: 4 }))
    expect(isBlue(shot.at(23, 50))).toBe(true)
    expect(isBlue(shot.at(28, 50)), "the gap").toBe(false)
  })

  it("draws dashes and dots for dashed and dotted, and a continuous line for solid", () => {
    const y = 27
    const solid = topStrip(paint("strip-solid", boxed({ outline: "6px solid #0000ff" })), y, isBlue)
    expect(solid.every(Boolean)).toBe(true)
    for (const style of ["dashed", "dotted"]) {
      const strip = topStrip(paint(`strip-${style}`, boxed({ outline: `6px ${style} #0000ff` })), y, isBlue)
      expect(strip.some(Boolean), `${style} paints`).toBe(true)
      expect(strip.some((painted) => !painted), `${style} has gaps`).toBe(true)
    }
  })

  it("draws auto as a solid line", () => {
    const strip = topStrip(paint("strip-auto", boxed({ outline: "6px auto #0000ff" })), 27, isBlue)
    expect(strip.every(Boolean)).toBe(true)
  })

  it("follows the rounded corners of the box", () => {
    const shot = paint("rounded", boxed({ borderRadius: 20, outline: "6px solid #0000ff" }))
    expect(isBlue(shot.at(27, 50))).toBe(true)
    // The corner of the ring is cut away where the box is round.
    expect(isBlue(shot.at(25, 25))).toBe(false)
  })

  it("is an error for a value CSS would drop", () => {
    const errorsOf = (style: Style) => {
      const errors: Error[] = []
      const { render, unmount } = createTestRoot({ onUncaughtError: (error) => errors.push(error) })
      try {
        render(<div style={style} />)
        return errors.map((error) => error.message).join("\n")
      } finally {
        unmount()
      }
    }
    expect(errorsOf({ outline: "2px 3px" })).toMatch(/invalid outline "2px 3px"/)
    expect(errorsOf({ outline: "solid dashed" })).toMatch(/invalid outline/)
    expect(errorsOf({ outline: "hidden" })).toMatch(/invalid outline "hidden"/)
    expect(errorsOf({ outline: "2px solid notacolour" })).toMatch(/not a colour/)
    expect(errorsOf({ outline: "" })).toMatch(/invalid outline/)
    expect(errorsOf({ hover: { outline: "50%" } })).toMatch(/invalid outline "50%"/)
  })

  describe("in a focus style", () => {
    const tab = (renderer: TestRoot["renderer"]) => renderer.simulateKeystrokes("Tab")

    it("takes the style and colour of the base when the state gives only the width", () => {
      const style = { outline: "2px solid #ff0000", focusVisible: { outlineWidth: 6 } }
      const at = paint("focus-width-rest", boxed(style))
      expect(isRed(at.at(29, 50))).toBe(true)
      expect(isRed(at.at(26, 50))).toBe(false)
      const on = paint("focus-width", boxed(style), tab)
      expect(isRed(on.at(26, 50))).toBe(true)
    })

    it("follows a colour the state changes when the base outline is currentColor", () => {
      const style = { outline: "6px solid", color: "#ff0000", focusVisible: { color: "#00ff00" } }
      expect(isRed(paint("focus-color-rest", boxed(style)).at(27, 50))).toBe(true)
      expect(isGreen(paint("focus-color", boxed(style), tab).at(27, 50))).toBe(true)
    })

    it("goes away with the state's outlineStyle none", () => {
      const style = { outline: "6px solid #0000ff", focusVisible: { outlineStyle: "none" } }
      expect(isBlue(paint("focus-off-rest", boxed(style)).at(27, 50))).toBe(true)
      expect(isBlue(paint("focus-off", boxed(style), tab).at(27, 50))).toBe(false)
    })
  })

  describe("in a state style", () => {
    const hoverBox = (renderer: TestRoot["renderer"]) => {
      const bounds = renderer.getElementBounds(renderer.findByTestId("box")!.id)!
      renderer.nativeSimulateMouseMove(bounds.x + 5, bounds.y + 5)
      renderer.clockFastForward(16)
    }

    it("shows on hover and replaces the base outline's longhands", () => {
      const style = { outlineWidth: 6, outlineStyle: "solid", outlineColor: "#ff0000" }
      const at = paint("state-rest", boxed({ ...style, hover: { outline: "6px solid #00ff00" } }))
      expect(isRed(at.at(27, 50))).toBe(true)
      const on = paint("state-hover", boxed({ ...style, hover: { outline: "6px solid #00ff00" } }), hoverBox)
      expect(isGreen(on.at(27, 50))).toBe(true)
    })

    it("appears from nothing", () => {
      const at = paint("state-none-rest", boxed({ hover: { outline: "6px solid #0000ff" } }))
      expect(isBlue(at.at(27, 50))).toBe(false)
      const on = paint("state-none-hover", boxed({ hover: { outline: "6px solid #0000ff" } }), hoverBox)
      expect(isBlue(on.at(27, 50))).toBe(true)
    })

    it("goes away with a state's outlineStyle none", () => {
      const style = { outline: "6px solid #0000ff", hover: { outlineStyle: "none" } }
      expect(isBlue(paint("state-off-rest", boxed(style)).at(27, 50))).toBe(true)
      expect(isBlue(paint("state-off-hover", boxed(style), hoverBox).at(27, 50))).toBe(false)
    })
  })
})
