import path from "path"
import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FILL = "#ff0000"
const PAGE = "#ffffff"
const MARGIN = 40

function shoot(name: string, tree: ReactNode) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(tree)
    renderer.dispatchNativeEvents()
    const file = path.join(SHOTS_DIR, `border-radius-elliptical-${name}.png`)
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer)
  } finally {
    unmount()
  }
}

/** A white page with a red box at (MARGIN, MARGIN). */
function boxed(width: number, height: number, style: Record<string, unknown>) {
  return (
    <div style={{ width: width + 2 * MARGIN, height: height + 2 * MARGIN, padding: MARGIN, backgroundColor: PAGE }}>
      <div style={{ width, height, backgroundColor: FILL, ...style }} />
    </div>
  )
}

/** Whether (x, y), in the box's own coordinates, is painted red. */
function painted(pixels: Pixels, x: number, y: number) {
  const [r, g, b] = pixels.at(MARGIN + x, MARGIN + y)
  if (r > 200 && g < 60 && b < 60) return true
  if (r > 240 && g > 240 && b > 240) return false
  throw new Error(`(${x}, ${y}) is neither the fill nor the page: ${pixels.at(MARGIN + x, MARGIN + y)}`)
}

/** The 100x100 box with an `a / b` radius: TL 10x40, TR 40x10, BR 10x40, BL 40x10. */
const ASYMMETRIC = "10px 40px / 40px 10px"

/** Points inside and outside each of the four corner ellipses of `ASYMMETRIC`. */
const ASYMMETRIC_INSIDE = [
  [3, 30],
  [80, 5],
  [97, 70],
  [20, 95],
]
const ASYMMETRIC_OUTSIDE = [
  [0, 15],
  [97, 2],
  [99, 98],
  [2, 98],
]

describeNative("elliptical border radius, painted", () => {
  it("50% on a wide box is an ellipse, not a circle", () => {
    const pixels = shoot("percent", boxed(200, 100, { borderRadius: "50%" }))
    expect(painted(pixels, 100, 50)).toBe(true)
    expect(painted(pixels, 100, 1)).toBe(true)
    expect(painted(pixels, 1, 50)).toBe(true)
    expect(painted(pixels, 198, 50)).toBe(true)
    // Bounding-box corners.
    for (const [x, y] of [
      [1, 1],
      [198, 1],
      [1, 98],
      [198, 98],
    ]) {
      expect(painted(pixels, x!, y!), `(${x}, ${y})`).toBe(false)
    }
    // A stadium with 50px caps would paint these; the ellipse does not.
    expect(painted(pixels, 30, 5)).toBe(false)
    expect(painted(pixels, 170, 95)).toBe(false)
  })

  it("a percentage resolves against the width across and the height down", () => {
    const percent = shoot("per-axis-percent", boxed(200, 100, { borderRadius: "30%" }))
    const lengths = shoot("per-axis-lengths", boxed(200, 100, { borderRadius: "60px / 30px" }))
    const circular = shoot("per-axis-circular", boxed(200, 100, { borderRadius: 30 }))
    let compared = 0
    for (let y = 0; y < 100; y += 3) {
      for (let x = 0; x < 200; x += 3) {
        expect(percent.at(MARGIN + x, MARGIN + y), `(${x}, ${y})`).toEqual(lengths.at(MARGIN + x, MARGIN + y))
        compared++
      }
    }
    expect(compared).toBeGreaterThan(2000)
    // The corner is cut deeper than a 30px circle cuts it.
    expect(painted(circular, 10, 10)).toBe(true)
    expect(painted(percent, 10, 10)).toBe(false)
  })

  it("em and a calc() of a length and a percentage resolve against the element, a percentage per axis", () => {
    // 1em is 20px on the box, and the box is 200 by 100: 2em + 10% is 60px across and 1em + 20% is 40px down.
    const units = shoot(
      "units",
      boxed(200, 100, { fontSize: 20, borderRadius: "calc(2em + 10%) / calc(1em + 20%)" }),
    )
    const lengths = shoot("units-lengths", boxed(200, 100, { borderRadius: "60px / 40px" }))
    let compared = 0
    for (let y = 0; y < 100; y += 3) {
      for (let x = 0; x < 200; x += 3) {
        expect(units.at(MARGIN + x, MARGIN + y), `(${x}, ${y})`).toEqual(lengths.at(MARGIN + x, MARGIN + y))
        compared++
      }
    }
    expect(compared).toBeGreaterThan(2000)
    expect(painted(units, 5, 5)).toBe(false)
    expect(painted(units, 100, 50)).toBe(true)
  })

  it("`a / b` gives each corner its own horizontal and vertical radius", () => {
    const pixels = shoot("asymmetric", boxed(100, 100, { borderRadius: ASYMMETRIC }))
    for (const [x, y] of ASYMMETRIC_INSIDE) expect(painted(pixels, x!, y!), `(${x}, ${y}) inside`).toBe(true)
    for (const [x, y] of ASYMMETRIC_OUTSIDE) expect(painted(pixels, x!, y!), `(${x}, ${y}) outside`).toBe(false)
  })

  it("the corner longhands take a horizontal and a vertical radius", () => {
    const pixels = shoot(
      "longhands",
      boxed(100, 100, {
        borderTopLeftRadius: "10px 40px",
        borderTopRightRadius: "40px 10px",
        borderBottomRightRadius: "10px 40px",
        borderBottomLeftRadius: "40px 10px",
      })
    )
    for (const [x, y] of ASYMMETRIC_INSIDE) expect(painted(pixels, x!, y!), `(${x}, ${y}) inside`).toBe(true)
    for (const [x, y] of ASYMMETRIC_OUTSIDE) expect(painted(pixels, x!, y!), `(${x}, ${y}) outside`).toBe(false)
  })

  it("a longhand wins over the shorthand and a zero vertical radius squares the corner", () => {
    const pixels = shoot(
      "square",
      boxed(100, 100, { borderRadius: "40px", borderTopLeftRadius: "20px 0", borderBottomRightRadius: 0 })
    )
    expect(painted(pixels, 1, 1)).toBe(true)
    expect(painted(pixels, 98, 98)).toBe(true)
    expect(painted(pixels, 98, 1)).toBe(false)
    expect(painted(pixels, 1, 98)).toBe(false)
  })

  it("overflow clips a child to the elliptical corner", () => {
    const pixels = shoot(
      "overflow",
      <div style={{ width: 200 + 2 * MARGIN, height: 100 + 2 * MARGIN, padding: MARGIN, backgroundColor: PAGE }}>
        <div style={{ width: 200, height: 100, borderRadius: "50%", overflow: "hidden" }}>
          <div style={{ width: 200, height: 100, backgroundColor: FILL }} />
        </div>
      </div>
    )
    expect(painted(pixels, 100, 50)).toBe(true)
    expect(painted(pixels, 100, 1)).toBe(true)
    expect(painted(pixels, 1, 1)).toBe(false)
    expect(painted(pixels, 30, 5)).toBe(false)
  })

  it("clipPath inset() round takes the vertical radii after the slash", () => {
    const pixels = shoot("clip-path", boxed(100, 100, { clipPath: `inset(0 round ${ASYMMETRIC})` }))
    for (const [x, y] of ASYMMETRIC_INSIDE) expect(painted(pixels, x!, y!), `(${x}, ${y}) inside`).toBe(true)
    for (const [x, y] of ASYMMETRIC_OUTSIDE) expect(painted(pixels, x!, y!), `(${x}, ${y}) outside`).toBe(false)
  })

  it("clipPath inset() round with one percentage rounds both axes by it", () => {
    const pixels = shoot("clip-path-percent", boxed(200, 100, { clipPath: "inset(0 round 50%)" }))
    expect(painted(pixels, 100, 50)).toBe(true)
    expect(painted(pixels, 1, 1)).toBe(false)
    expect(painted(pixels, 30, 5)).toBe(false)
    expect(painted(pixels, 100, 1)).toBe(true)
  })
})

describeNative("elliptical border radius, hit testing", () => {
  /** Clicks (x, y) and returns the names of the elements that got the click, target first. */
  function clickAt(tree: (record: (name: string) => () => void) => ReactNode, x: number, y: number) {
    const { render, renderer, unmount } = createTestRoot()
    const log: string[] = []
    try {
      render(tree((name) => () => log.push(name)))
      renderer.dispatchNativeEvents()
      renderer.nativeSimulateClick(x, y)
      return log
    } finally {
      unmount()
    }
  }

  /** A box at (50, 50) above a sibling that fills the stage. */
  const scene =
    (width: number, height: number, style: Record<string, unknown>) => (record: (name: string) => () => void) => (
      <div style={{ position: "relative", width: 300, height: 250 }}>
        <div style={{ position: "absolute", width: 300, height: 250 }} onClick={record("under")} />
        <div style={{ position: "absolute", left: 50, top: 50, width, height, ...style }} onClick={record("rounded")} />
      </div>
    )

  it("a point inside the bounding box but outside the ellipse reaches what is underneath", () => {
    const ellipse = scene(200, 100, { borderRadius: "50%" })
    expect(clickAt(ellipse, 51, 51)).toEqual(["under"])
    expect(clickAt(ellipse, 80, 55)).toEqual(["under"])
    expect(clickAt(ellipse, 220, 145)).toEqual(["under"])
    expect(clickAt(ellipse, 150, 100)).toEqual(["rounded"])
    expect(clickAt(ellipse, 150, 52)).toEqual(["rounded"])
    expect(clickAt(ellipse, 52, 100)).toEqual(["rounded"])
  })

  it("`a / b` cuts each corner by its own ellipse", () => {
    const corners = scene(100, 100, { borderRadius: ASYMMETRIC })
    for (const [x, y] of ASYMMETRIC_INSIDE) {
      expect(clickAt(corners, 50 + x!, 50 + y!), `(${x}, ${y}) inside`).toEqual(["rounded"])
    }
    for (const [x, y] of ASYMMETRIC_OUTSIDE) {
      expect(clickAt(corners, 50 + x!, 50 + y!), `(${x}, ${y}) outside`).toEqual(["under"])
    }
  })

  it("a percentage cuts by the width across and the height down", () => {
    const percent = scene(200, 100, { borderRadius: "30%" })
    // Inside a 30px circle, outside the 60x30 ellipse.
    expect(clickAt(percent, 60, 60)).toEqual(["under"])
    expect(clickAt(percent, 80, 80)).toEqual(["rounded"])
  })

  it("a corner longhand with two values cuts its own corner only", () => {
    const longhand = scene(100, 100, { borderTopLeftRadius: "10px 40px" })
    expect(clickAt(longhand, 50, 65)).toEqual(["under"])
    expect(clickAt(longhand, 53, 80)).toEqual(["rounded"])
    expect(clickAt(longhand, 149, 51)).toEqual(["rounded"])
  })
})

describeNative("an invalid border radius is an uncaught error", () => {
  /** The messages of the errors rendering `style` reports. */
  function errorsOf(style: Record<string, unknown>) {
    const errors: Error[] = []
    const { render, unmount } = createTestRoot({ onUncaughtError: (error) => errors.push(error) })
    try {
      render(<div style={style} />)
      return errors.map((error) => error.message).join("\n")
    } finally {
      unmount()
    }
  }

  it("names the property and the value", () => {
    expect(errorsOf({ borderRadius: "1ch" })).toMatch(/border-radius "1ch": "1ch" is not a non-negative length/)
    expect(errorsOf({ borderRadius: "-4px" })).toMatch(/"-4px" is negative/)
    expect(errorsOf({ borderRadius: "10px /" })).toMatch(/border-radius "10px \/"/)
    expect(errorsOf({ borderTopLeftRadius: "1px 2px 3px" })).toMatch(/border-radius "1px 2px 3px"/)
    expect(errorsOf({ borderTopLeftRadius: -1 })).toMatch(/non-negative number/)
  })

  it("is an error inside a state style too", () => {
    expect(errorsOf({ hover: { borderRadius: "bogus" } })).toMatch(/border-radius "bogus"/)
  })

  it("keeps the previous valid style when a re-render's radius is rejected", () => {
    const errors: Error[] = []
    const { render, renderer, unmount } = createTestRoot({ onUncaughtError: (error) => errors.push(error) })
    try {
      render(<div testId="box" style={{ width: 50, height: 20, borderRadius: "50%" }} />)
      render(<div testId="box" style={{ width: 80, height: 20, borderRadius: "5ch" }} />)
      expect(errors.map((error) => error.message).join()).toMatch(/border-radius "5ch"/)
      expect(renderer.getElementBounds(renderer.findByTestId("box")!.id)?.width).toBe(50)
    } finally {
      unmount()
    }
  })
})
