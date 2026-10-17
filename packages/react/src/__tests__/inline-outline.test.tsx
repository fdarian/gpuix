/// The `outline` of an inline box. A span has no element to draw one with, so its
/// paragraph rings each line fragment of the span's text, over the glyphs.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const
const PAGE = { width: 300, height: 100, padding: 20, backgroundColor: "#ffffff" } as const
const RING = { outlineWidth: 4, outlineStyle: "solid", outlineColor: "#ff0000" } as const
const RED: [number, number, number, number] = [255, 0, 0, 255]
const BLUE: [number, number, number, number] = [0, 0, 255, 255]
const WHITE: [number, number, number, number] = [255, 255, 255, 255]

let measuredColumn: number | undefined

/** Menlo has one advance for every character, so a column is a fixed width. Measured
 *  once, in a window of its own: a second test renderer stops the first. */
function columnWidth() {
  measuredColumn ??= measureColumn()
  return measuredColumn
}

function measureColumn() {
  const testRoot = createTestRoot()
  testRoot.render(
    <div highlight={{ query: "aaaaaaaaaa" }}>
      <text style={TEXT}>{"aaaaaaaaaa"}</text>
    </div>
  )
  const width = testRoot.renderer.getPaintedHighlights()[0]!.rects[0]!.width / 10
  testRoot.unmount()
  return width
}

describeNative("outline on inline boxes", () => {
  function setup(node: React.ReactNode) {
    columnWidth()
    const testRoot = createTestRoot({ width: 300, height: 100 })
    testRoot.render(node)
    const origin = () => testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("p")!.id)!
    const shoot = (name: string) => {
      const file = `${SHOTS_DIR}/inline-outline-${name}.png`
      testRoot.renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, testRoot.renderer)
      const box = origin()
      return {
        /** The pixel at `column` columns and `line` lines (fractions allowed) from
         *  the paragraph's top left corner. */
        at: (column: number, line: number) =>
          pixels.at(Math.floor(box.x + column * columnWidth()), Math.floor(box.y + line * LINE)),
        /** The pixel `dx` and `dy` logical pixels from that point. */
        near: (column: number, line: number, dx: number, dy: number) =>
          pixels.at(
            Math.floor(box.x + column * columnWidth() + dx),
            Math.floor(box.y + line * LINE + dy)
          ),
        redPixels: () => {
          let count = 0
          for (let y = 0; y < pixels.height; y++)
            for (let x = 0; x < pixels.width; x++) {
              const [r, g, b] = pixels.at(x, y)
              if (r > 200 && g < 60 && b < 60) count++
            }
          return count
        },
      }
    }
    return { testRoot, shoot, tab: () => testRoot.renderer.simulateKeystrokes("Tab") }
  }

  /** At 10.5 columns "aaaa bbbb cccc" wraps after "bbbb ": the span `bbbb cccc` is
   *  columns 5-9 of line 0, the hanging space included, and columns 0-3 of line 1.
   *  A second stop follows, to move focus to. */
  const wrapped = (span: object) => (
    <div style={PAGE}>
      <text testId="p" style={{ ...TEXT, width: Math.ceil(10.5 * columnWidth()) }}>
        aaaa{" "}
        <span tabIndex={0} style={span}>
          bbbb cccc
        </span>
      </text>
      <div tabIndex={0} style={{ width: 10, height: 10 }} />
    </div>
  )

  it("rings each line fragment of a focused span and nothing before it has focus", () => {
    const { testRoot, shoot, tab } = setup(wrapped({ focusVisible: RING }))
    expect(shoot("fragments-before").redPixels()).toBe(0)
    tab()
    const ring = shoot("fragments")

    expect(ring.near(5, 0, -2, 10), "left of the first fragment").toEqual(RED)
    expect(ring.near(10, 0, 2, 10), "right of the first fragment").toEqual(RED)
    expect(ring.near(7, 0, 0, -2), "above the first fragment").toEqual(RED)
    expect(ring.near(7, 1, 0, 2), "below the first fragment").toEqual(RED)
    expect(ring.near(0, 1, -2, 10), "left of the second fragment").toEqual(RED)
    expect(ring.near(4, 1, 2, 10), "right of the second fragment").toEqual(RED)
    expect(ring.near(2, 0, 0, 18), "above the second fragment").toEqual(RED)
    expect(ring.near(2, 1, 0, 22), "below the second fragment").toEqual(RED)

    // Not one box round both lines: the corners that box would have are bare.
    expect(ring.near(0, 0, -2, 10), "left of line 0, where the span has no text").toEqual(WHITE)
    expect(ring.near(10, 1, 2, 10), "right of line 1, where the span has no text").toEqual(WHITE)
    expect(ring.near(7, 0, 0, 18), "inside the first fragment").toEqual(WHITE)
    testRoot.unmount()
  })

  it("takes the ring off when focus moves on", () => {
    const { testRoot, shoot, tab } = setup(wrapped({ focusVisible: RING }))
    tab()
    expect(shoot("away-on").redPixels()).toBeGreaterThan(0)
    tab()
    expect(shoot("away-off").redPixels()).toBe(0)
    testRoot.unmount()
  })

  it("keeps the layout still while the ring shows", () => {
    const { testRoot, tab } = setup(wrapped({ focusVisible: { ...RING, outlineOffset: 3 } }))
    const paragraph = () => testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("p")!.id)
    const before = paragraph()
    tab()
    expect(paragraph()).toEqual(before)
    testRoot.unmount()
  })

  it("puts the ring outside the fragment by its outlineOffset", () => {
    const { testRoot, shoot, tab } = setup(wrapped({ focusVisible: { ...RING, outlineOffset: 6 } }))
    tab()
    const ring = shoot("offset")
    // The ring covers 6 to 10 pixels left of the second fragment.
    expect(ring.near(0, 1, -3, 10), "within the offset").toEqual(WHITE)
    expect(ring.near(0, 1, -8, 10), "on the ring").toEqual(RED)
    expect(ring.near(0, 1, -12, 10), "beyond it").toEqual(WHITE)
    testRoot.unmount()
  })

  it("draws a negative offset over the glyphs", () => {
    const { testRoot, shoot, tab } = setup(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          <span tabIndex={0} style={{ focusVisible: { outlineWidth: 6, outlineStyle: "solid", outlineColor: "#ff0000", outlineOffset: -6 } }}>
            mmmm
          </span>
        </text>
      </div>
    )
    tab()
    const ring = shoot("inside")
    // The ring fills the first six pixels of the cell where an `m` stands, and
    // nothing of the glyph shows through it.
    for (const dy of [3, 6, 10, 14, 17]) {
      for (const dx of [1, 3, 5]) expect(ring.near(0, 0, dx, dy)).toEqual(RED)
    }
    testRoot.unmount()
  })

  /** `a [b [c] d] e`: the outer span is columns 2-6, the inner one column 4. */
  const nested = (outer: object, inner: object) => (
    <div style={PAGE}>
      <text testId="p" style={TEXT}>
        a{" "}
        <span tabIndex={0} style={outer}>
          b{" "}
          <span tabIndex={0} style={inner}>
            c
          </span>{" "}
          d
        </span>{" "}
        e
      </text>
    </div>
  )
  const BLUE_RING = { outlineWidth: 4, outlineStyle: "solid", outlineColor: "#0000ff" } as const

  it("rings the focused span and not the one around it", () => {
    const { testRoot, shoot, tab } = setup(
      nested({ focusVisible: BLUE_RING }, { focusVisible: RING })
    )
    tab()
    const outer = shoot("outer")
    expect(outer.near(2, 0, -2, 10), "left of the outer span").toEqual(BLUE)
    expect(outer.near(4, 0, -2, 10), "the nested span is not focus-visible").toEqual(WHITE)
    tab()
    const inner = shoot("inner")
    expect(inner.near(4, 0, -2, 10), "left of the nested span").toEqual(RED)
    expect(inner.near(2, 0, -2, 10), "the outer span is not focus-visible").toEqual(WHITE)
    expect(inner.near(7, 0, 2, 10), "right of the outer span").toEqual(WHITE)
    testRoot.unmount()
  })

  it("rings a span for focusWithin while a span in it has focus", () => {
    const { testRoot, shoot, tab } = setup(nested({ focusWithin: BLUE_RING }, {}))
    expect(shoot("within-before").near(2, 0, -2, 10)).toEqual(WHITE)
    tab()
    tab()
    const within = shoot("within")
    expect(within.near(2, 0, -2, 10)).toEqual(BLUE)
    expect(within.near(7, 0, 2, 10)).toEqual(BLUE)
    testRoot.unmount()
  })

  it("rings a hovered span and takes the ring off when the pointer leaves", () => {
    const { testRoot, shoot } = setup(nested({ hover: RING }, {}))
    const box = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("p")!.id)!
    const column = columnWidth()
    testRoot.renderer.nativeSimulateMouseMove(box.x + 2.5 * column, box.y + LINE / 2)
    expect(shoot("hover").near(2, 0, -2, 10)).toEqual(RED)
    testRoot.renderer.nativeSimulateMouseMove(box.x + 20 * column, box.y + LINE / 2)
    expect(shoot("hover-off").redPixels()).toBe(0)
    testRoot.unmount()
  })

  it("clips the ring to the box around the paragraph like any painted thing", () => {
    const { testRoot, shoot, tab } = setup(
      <div style={PAGE}>
        <div style={{ overflow: "hidden", width: 200 }}>
          <text testId="p" style={TEXT}>
            <span tabIndex={0} style={{ focusVisible: RING }}>
              abcd
            </span>
          </text>
        </div>
      </div>
    )
    tab()
    const ring = shoot("clipped")
    expect(ring.near(0, 0, -2, 10), "outside the clip").toEqual(WHITE)
    expect(ring.near(4, 0, 2, 10), "inside it").toEqual(RED)
    testRoot.unmount()
  })
})
