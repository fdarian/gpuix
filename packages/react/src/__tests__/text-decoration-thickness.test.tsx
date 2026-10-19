/// `text-decoration-thickness`: Chrome's `auto` is a tenth of the font size of the
/// box that declares the line, and `from-font` is the font's own underline thickness
/// (a tenth of the font size when the font has none). Neither is rounded when it
/// resolves, a solid line floors it to a device pixel when it paints, and every line
/// (underline, overline, line-through) has the same thickness.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINES = ["underline", "overline", "line-through"] as const

let shots = 0

/** The thickness in device pixels of the red line `node` draws, and the scale of the
 *  display it was drawn on. */
function measure(node: React.ReactNode): { thickness: number; scale: number } {
  const testRoot = createTestRoot()
  testRoot.render(node)
  const file = `${SHOTS_DIR}/text-decoration-thickness-${shots++}.png`
  testRoot.renderer.captureScreenshot(file)
  const pixels = readPixels(file)
  const scale = pixels.width / testRoot.renderer.getWindowSize().width
  const rows: number[] = []
  for (let y = 0; y < pixels.height; y++) {
    let red = 0
    for (let x = 0; x < pixels.width; x++) {
      const [r, g, b] = pixels.at(x, y)
      if (r > 250 && g < 5 && b < 5) red++
    }
    if (red >= 8) rows.push(y)
  }
  expect(rows[rows.length - 1]! - rows[0]! + 1, "one line, no gap").toBe(rows.length)
  return { thickness: rows.length, scale }
}

const paragraph = (fontSize: number, style: object, children: React.ReactNode = "xxxx") => (
  <text
    style={
      {
        width: 300,
        height: Math.ceil(fontSize * 2),
        backgroundColor: "#ffffff",
        color: "#000000",
        fontSize,
        lineHeight: `${Math.ceil(fontSize * 1.5)}px`,
        textDecoration: "underline #ff0000",
        ...style,
      } as never
    }
  >
    {children}
  </text>
)

function paintedThickness(
  fontSize: number,
  style: object,
  line: (typeof LINES)[number] = "underline"
) {
  return measure(paragraph(fontSize, { textDecoration: `${line} #ff0000`, ...style }))
}

/** What Chrome paints for `auto` at 1x and 2x: `fontSize * scale / 10`, floored. */
const CHROME_AUTO: Record<number, Record<number, number>> = {
  1: { 16: 1, 32: 3, 40: 4 },
  2: { 16: 3, 32: 6, 40: 8 },
}

describeNative("textDecorationThickness: auto", () => {
  for (const fontSize of [16, 32, 40]) {
    it(`is a tenth of a ${fontSize}px font, floored to a device pixel`, () => {
      const { thickness, scale } = paintedThickness(fontSize, {})
      expect(thickness).toBe(Math.max(1, Math.floor((fontSize * scale) / 10)))
      expect(thickness).toBe(CHROME_AUTO[scale]?.[fontSize] ?? thickness)
    })
  }

  it("is what the keyword and the default say alike", () => {
    expect(paintedThickness(40, { textDecoration: "underline auto #ff0000" }).thickness).toBe(
      paintedThickness(40, {}).thickness
    )
    expect(paintedThickness(40, { textDecorationThickness: "auto" }).thickness).toBe(
      paintedThickness(40, {}).thickness
    )
  })

  for (const line of LINES) {
    it(`is the thickness of a ${line}`, () => {
      const { thickness, scale } = paintedThickness(40, {}, line)
      expect(thickness).toBe(Math.floor((40 * scale) / 10))
    })
  }

  it("follows the font size of the box that declares the line, not of the text", () => {
    const { thickness, scale } = measure(
      paragraph(16, { lineHeight: "60px" }, <span style={{ fontSize: 40 } as never}>xxxx</span>)
    )
    expect(thickness).toBe(Math.floor((16 * scale) / 10))
  })
})

describeNative("textDecorationThickness: from-font", () => {
  it("is the font's own underline thickness, the same for every line", () => {
    const fromFont = (fontSize: number, line: (typeof LINES)[number] = "underline") =>
      paintedThickness(fontSize, { textDecoration: `${line} from-font #ff0000` }, line)
    const { thickness, scale } = fromFont(100)
    expect(thickness).not.toBe(Math.floor((100 * scale) / 10))
    expect(thickness).toBeGreaterThan(0)
    for (const line of ["overline", "line-through"] as const) {
      expect(fromFont(100, line).thickness).toBe(thickness)
    }
  })

  it("grows with the font size, as the font's metric does", () => {
    const fromFont = (fontSize: number) =>
      paintedThickness(fontSize, { textDecoration: "underline from-font #ff0000" })
    const small = fromFont(100).thickness
    const large = fromFont(200).thickness
    expect(large).toBeGreaterThanOrEqual(2 * small)
    expect(large).toBeLessThanOrEqual(2 * small + 1)
  })
})

describeNative("textDecorationThickness: a negative length", () => {
  // Chrome keeps a negative length as a valid value and paints a line of one device pixel for it, as it does
  // for a length of zero (headless Chrome 1248: -2px, -0.1em and 0px all fill one row).
  for (const value of ["-2px", "-0.1em", "-50%", "calc(2px - 6px)", "0px"]) {
    it(`${value} paints one device pixel`, () => {
      expect(paintedThickness(40, { textDecorationThickness: value }).thickness).toBe(1)
      expect(paintedThickness(40, { textDecoration: `underline ${value} #ff0000` }).thickness).toBe(1)
    })
  }

  it("is not ignored for the thickness the shorthand set before it", () => {
    expect(paintedThickness(40, { textDecoration: "underline 6px #ff0000", textDecorationThickness: "-3px" }).thickness).toBe(1)
  })
})
