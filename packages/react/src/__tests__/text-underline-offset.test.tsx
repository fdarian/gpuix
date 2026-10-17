/// `text-underline-offset`: where the top edge of an underline sits below the
/// alphabetic baseline, as Chrome places it. `0` is the baseline itself, so every
/// expectation is a distance from the underline of the same text with `0px`.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const RED = [255, 0, 0] as const
const TEXT = {
  width: 200,
  height: 80,
  backgroundColor: "#ffffff",
  color: "#000000",
  fontSize: 16,
  lineHeight: "24px",
} as const

let shots = 0

/** The rows that have red pixels in them, which only the underline paints. */
function underlineRows(
  node: React.ReactNode,
  hover?: { x: number; y: number },
  keystrokes?: string
): number[] {
  const testRoot = createTestRoot()
  testRoot.render(node)
  if (hover) testRoot.renderer.nativeSimulateMouseMove(hover.x, hover.y)
  if (keystrokes) testRoot.renderer.simulateKeystrokes(keystrokes)
  const file = `${SHOTS_DIR}/text-underline-offset-${shots++}.png`
  testRoot.renderer.captureScreenshot(file)
  const pixels = readLogicalPixels(file, testRoot.renderer)
  const rows: number[] = []
  for (let y = 0; y < TEXT.height; y++) {
    let red = 0
    for (let x = 0; x < TEXT.width; x++) {
      const [r, g, b] = pixels.at(x, y)
      if (r > 200 && g < 60 && b < 60) red++
    }
    if (red >= 8) rows.push(y)
  }
  return rows
}

/** The row of a one pixel underline. */
function underlineY(node: React.ReactNode): number {
  const rows = underlineRows(node)
  expect(rows, "one underline row").toHaveLength(1)
  return rows[0]!
}

/** The top row of an underline thicker than a pixel: `auto` is a tenth of the font
 *  size, so text of 32px or more has a line of several. */
function underlineTop(node: React.ReactNode): number {
  const rows = underlineRows(node)
  expect(rows.length, "an underline").toBeGreaterThan(0)
  return rows[0]!
}

const UNDERLINE = { textDecoration: "underline #ff0000" } as const

/** `style` on the paragraph that declares the underline. */
const own = (style: object) => (
  <text style={{ ...TEXT, ...UNDERLINE, ...style } as never}>xxxx</text>
)

describeNative("textUnderlineOffset", () => {
  const baseline = () => underlineY(own({ textUnderlineOffset: "0px" }))

  it("puts the underline a pixel below the baseline by default, and as auto", () => {
    const y0 = baseline()
    expect(underlineY(own({}))).toBe(y0 + 1)
    expect(underlineY(own({ textUnderlineOffset: "auto" }))).toBe(y0 + 1)
  })

  it("measures a length from the baseline, with no gap", () => {
    const y0 = baseline()
    expect(underlineY(own({ textUnderlineOffset: "4px" }))).toBe(y0 + 4)
    expect(underlineY(own({ textUnderlineOffset: 4 }))).toBe(y0 + 4)
    expect(underlineY(own({ textUnderlineOffset: "10px" }))).toBe(y0 + 10)
    expect(underlineY(own({ textUnderlineOffset: "calc(1px + 0.1875em)" }))).toBe(y0 + 4)
  })

  it("takes em of the element's own font size", () => {
    const y0 = baseline()
    expect(underlineY(own({ textUnderlineOffset: "0.25em" }))).toBe(y0 + 4)
    expect(underlineTop(own({ textUnderlineOffset: "0.25em", fontSize: 32 }))).toBe(
      underlineTop(own({ textUnderlineOffset: "0px", fontSize: 32 })) + 8
    )
  })

  it("takes a percentage of 1em", () => {
    const y0 = baseline()
    expect(underlineY(own({ textUnderlineOffset: "25%" }))).toBe(y0 + 4)
    expect(underlineY(own({ textUnderlineOffset: "50%" }))).toBe(y0 + 8)
    expect(underlineTop(own({ textUnderlineOffset: "25%", fontSize: 32 }))).toBe(
      underlineTop(own({ textUnderlineOffset: "0px", fontSize: 32 })) + 8
    )
  })

  it("draws a negative offset above the baseline", () => {
    const y0 = baseline()
    expect(underlineY(own({ textUnderlineOffset: "-3px" }))).toBe(y0 - 3)
    expect(underlineY(own({ textUnderlineOffset: "-25%" }))).toBe(y0 - 4)
  })

  it("rounds a fractional offset to a pixel", () => {
    const y0 = baseline()
    expect(underlineY(own({ textUnderlineOffset: "4.4px" }))).toBe(y0 + 4)
    expect(underlineY(own({ textUnderlineOffset: "4.6px" }))).toBe(y0 + 5)
  })

  it("ignores the thickness once the offset is a length, and grows the auto gap with it", () => {
    const y0 = baseline()
    const rows = underlineRows(own({ textUnderlineOffset: "2px", textDecorationThickness: "6px" }))
    expect(rows[0]).toBe(y0 + 2)
    expect(rows).toHaveLength(6)
    // Half the thickness, rounded up.
    expect(underlineRows(own({ textDecorationThickness: "6px" }))[0]).toBe(y0 + 3)
    expect(underlineRows(own({ textDecorationThickness: "3px" }))[0]).toBe(y0 + 2)
  })

  describe("inherited", () => {
    it("is inherited from a parent by the element that declares the underline", () => {
      const y0 = baseline()
      expect(
        underlineY(
          <div style={{ textUnderlineOffset: "4px" } as never}>
            <text style={{ ...TEXT, ...UNDERLINE } as never}>xxxx</text>
          </div>
        )
      ).toBe(y0 + 4)
      expect(
        underlineY(
          <div style={{ textUnderlineOffset: "4px" } as never}>
            <div>
              <text style={{ ...TEXT, ...UNDERLINE } as never}>xxxx</text>
            </div>
          </div>
        )
      ).toBe(y0 + 4)
    })

    it("is replaced by the offset the element sets itself, and auto resets it", () => {
      const y0 = baseline()
      const under = (parent: object, own: object) => (
        <div style={parent as never}>
          <text style={{ ...TEXT, ...UNDERLINE, ...own } as never}>xxxx</text>
        </div>
      )
      expect(underlineY(under({ textUnderlineOffset: "4px" }, { textUnderlineOffset: "7px" }))).toBe(y0 + 7)
      expect(underlineY(under({ textUnderlineOffset: "4px" }, { textUnderlineOffset: "auto" }))).toBe(y0 + 1)
    })

    it("passes an em on as the px it was where it was set", () => {
      const y0 = baseline()
      // 0.25em of the parent's 16px is 4px, whatever the text's own size.
      const big = underlineTop(
        <div style={{ fontSize: 16, textUnderlineOffset: "0.25em" } as never}>
          <text style={{ ...TEXT, ...UNDERLINE, fontSize: 32 } as never}>xxxx</text>
        </div>
      )
      expect(big).toBe(underlineTop(own({ textUnderlineOffset: "0px", fontSize: 32 })) + 4)
      expect(y0).toBeGreaterThan(0)
    })

    it("passes a percentage on as a percentage, of the box that draws the line", () => {
      const big = underlineTop(
        <div style={{ fontSize: 16, textUnderlineOffset: "25%" } as never}>
          <text style={{ ...TEXT, ...UNDERLINE, fontSize: 32 } as never}>xxxx</text>
        </div>
      )
      expect(big).toBe(underlineTop(own({ textUnderlineOffset: "0px", fontSize: 32 })) + 8)
    })
  })

  describe("on a span", () => {
    const inline = (paragraph: object, span: object) => (
      <text style={{ ...TEXT, ...paragraph } as never}>
        <span style={span as never}>xxxx</span>
      </text>
    )

    it("is the decorating box's, inherited from the paragraph", () => {
      const y0 = baseline()
      expect(underlineY(inline({ textUnderlineOffset: "4px" }, UNDERLINE))).toBe(y0 + 4)
    })

    it("is the offset the span sets, over the paragraph's", () => {
      const y0 = baseline()
      expect(underlineY(inline({ textUnderlineOffset: "4px" }, { ...UNDERLINE, textUnderlineOffset: "6px" }))).toBe(y0 + 6)
      expect(underlineY(inline({}, { ...UNDERLINE, textUnderlineOffset: "6px" }))).toBe(y0 + 6)
    })

    it("is not moved by an offset below the box that declared the line", () => {
      const y0 = baseline()
      const below = underlineY(
        <text style={{ ...TEXT, ...UNDERLINE, textUnderlineOffset: "4px" } as never}>
          <span style={{ textUnderlineOffset: "10px" } as never}>xxxx</span>
        </text>
      )
      expect(below).toBe(y0 + 4)
    })

    it("takes the em it inherited as px and a percentage of the span's own size", () => {
      const reference = underlineTop(inline({}, { ...UNDERLINE, fontSize: 32, textUnderlineOffset: "0px" }))
      expect(
        underlineTop(inline({ textUnderlineOffset: "0.25em" }, { ...UNDERLINE, fontSize: 32 }))
      ).toBe(reference + 4)
      expect(
        underlineTop(inline({ textUnderlineOffset: "25%" }, { ...UNDERLINE, fontSize: 32 }))
      ).toBe(reference + 8)
    })

    it("sets each line of nested underlines by the box that declared it", () => {
      const y0 = baseline()
      const rows = underlineRows(
        <text style={{ ...TEXT, ...UNDERLINE, textUnderlineOffset: "2px" } as never}>
          <span style={{ ...UNDERLINE, textUnderlineOffset: "8px" } as never}>xxxx</span>
        </text>
      )
      expect(rows).toEqual([y0 + 2, y0 + 8])
    })
  })

  describe("on state styles", () => {
    it("takes a hover style's offset", () => {
      const y0 = baseline()
      const hovered = underlineRows(
        <text style={{ ...TEXT, ...UNDERLINE, hover: { textUnderlineOffset: "6px" } } as never}>xxxx</text>,
        { x: 4, y: 4 }
      )
      expect(hovered).toEqual([y0 + 6])
    })
  })

  describe("on focus styles", () => {
    /** A focusable box whose `state` style declares the underline. */
    const focusable = (state: "focusVisible" | "focusWithin", declared: object, own: object = {}) => (
      <div
        tabIndex={0}
        style={{ ...TEXT, ...own, [state]: declared } as never}
      >
        xxxx
      </div>
    )

    for (const state of ["focusVisible", "focusWithin"] as const) {
      it(`gives the underline a ${state} style declares the offset it inherits`, () => {
        const y0 = baseline()
        const inherited = (child: React.ReactElement) => (
          <div style={{ textUnderlineOffset: "4px" } as never}>{child}</div>
        )
        expect(underlineRows(focusable(state, UNDERLINE), undefined, "Tab")).toEqual([y0 + 1])
        expect(
          underlineRows(inherited(focusable(state, UNDERLINE)), undefined, "Tab")
        ).toEqual([y0 + 4])
        expect(underlineRows(inherited(focusable(state, UNDERLINE)))).toEqual([])
      })

      it(`gives the underline a ${state} style the offset of the element itself over the inherited one`, () => {
        const y0 = baseline()
        expect(
          underlineRows(
            <div style={{ textUnderlineOffset: "4px" } as never}>
              {focusable(state, UNDERLINE, { textUnderlineOffset: "7px" })}
            </div>,
            undefined,
            "Tab"
          )
        ).toEqual([y0 + 7])
      })

      it(`takes the offset a ${state} style declares itself`, () => {
        const y0 = baseline()
        expect(
          underlineRows(
            <div style={{ textUnderlineOffset: "4px" } as never}>
              {focusable(state, { ...UNDERLINE, textUnderlineOffset: "9px" })}
            </div>,
            undefined,
            "Tab"
          )
        ).toEqual([y0 + 9])
      })

      it(`moves the underline of the element when a ${state} style sets only the offset`, () => {
        const y0 = baseline()
        const box = focusable(state, { textUnderlineOffset: "6px" }, UNDERLINE)
        expect(underlineRows(box)).toEqual([y0 + 1])
        expect(underlineRows(box, undefined, "Tab")).toEqual([y0 + 6])
      })
    }

    it("lets focusVisible take the offset focusWithin sets under it", () => {
      const y0 = baseline()
      expect(
        underlineRows(
          <div
            tabIndex={0}
            style={
              {
                ...TEXT,
                focusWithin: { textUnderlineOffset: "5px" },
                focusVisible: UNDERLINE,
              } as never
            }
          >
            xxxx
          </div>,
          undefined,
          "Tab"
        )
      ).toEqual([y0 + 5])
    })
  })
})
