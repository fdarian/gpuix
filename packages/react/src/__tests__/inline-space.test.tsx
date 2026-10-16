/// The space of an inline box along the line: its margin, border and padding push
/// the text around it, widen the line and wrap with it. The box that holds them is
/// `inline-border-box.test.tsx`; painting the border and background comes later.

import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const

/** Margin 10 + border 2 + padding 4 before the text, margin 6 + border 1 + padding 3
 *  after it. */
const SPACE = {
  marginLeft: 10,
  borderLeftWidth: 2,
  borderLeftStyle: "solid",
  paddingLeft: 4,
  marginRight: 6,
  borderRightWidth: 1,
  borderRightStyle: "solid",
  paddingRight: 3,
} as const
const LEADING = 16
const TRAILING = 10

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

describeNative("space of an inline box", () => {
  function setup() {
    columnWidth()
    const refs = new Map<string, PublicInstance>()
    const ref = (name: string) => (instance: PublicInstance | null) => {
      if (instance) refs.set(name, instance)
    }
    const root = createTestRoot({ width: 300, height: 100 })
    const element = (name: string) => refs.get(name)!
    const rect = (name: string) => element(name).getBoundingClientRect()
    /** The point `dx` pixels from the left of the line `line` of the paragraph `p`. */
    const at = (dx: number, line = 0) => {
      const box = root.renderer.getElementBounds(root.renderer.findByTestId("p")!.id)!
      return { x: box.x + dx, y: box.y + (line + 0.5) * LINE }
    }
    return { ...root, ref, rect, at }
  }

  /** A span with no run style of its own, and one with, as the two paint differently. */
  const variants = { "a span that only has space": {}, "a span with a colour": { color: "#ff0000" } }

  for (const [name, extra] of Object.entries(variants)) {
    it(`pushes the text after ${name} right by its margin, border and padding`, () => {
      const afterX = (space: object) => {
        const t = setup()
        try {
          t.render(
            <div style={{ width: 400 }}>
              <text style={TEXT}>
                aa <span style={{ ...extra, ...space }}>bb</span>
                <span ref={t.ref("after")}>cc</span>
              </text>
            </div>
          )
          return t.rect("after").x
        } finally {
          t.unmount()
        }
      }
      const plain = afterX({})
      expect(afterX(SPACE) - plain).toBeCloseTo(LEADING + TRAILING, 1)
      expect(afterX({ paddingLeft: 7 }) - plain).toBeCloseTo(7, 1)
      expect(afterX({ marginRight: 7 }) - plain).toBeCloseTo(7, 1)
      expect(afterX({ borderLeftWidth: 7, borderLeftStyle: "solid" }) - plain).toBeCloseTo(7, 1)
      expect(afterX({ borderLeftWidth: 7, borderLeftColor: "#ff0000" }) - plain, "no line style, no border").toBeCloseTo(0, 1)
      expect(afterX({ borderLeftStyle: "solid" }) - plain, "medium without a width").toBeCloseTo(3, 1)
    })
  }

  it("makes the line as much wider, and so the container that wraps around it", () => {
    const widthOf = (space: object) => {
      const t = setup()
      try {
        t.render(
          <div style={{ display: "flex" }}>
            <text ref={t.ref("text")} style={{ ...TEXT, whiteSpace: "nowrap" }}>
              aa <span style={space}>bb</span> cc
            </text>
          </div>
        )
        return t.rect("text").width
      } finally {
        t.unmount()
      }
    }
    // The container takes its text's width rounded up to a whole pixel.
    const plain = widthOf({})
    expect(plain).toBeGreaterThanOrEqual(8 * columnWidth() - 0.01)
    expect(plain).toBeLessThan(8 * columnWidth() + 1)
    expect(Math.abs(widthOf(SPACE) - plain - (LEADING + TRAILING))).toBeLessThan(1)
  })

  it("wraps the text earlier for the space around a word", () => {
    const column = columnWidth()
    const heightOf = (space: object) => {
      const t = setup()
      try {
        t.render(
          <div style={{ width: 400 }}>
            <text ref={t.ref("text")} style={{ ...TEXT, width: Math.ceil(9.5 * column) }}>
              aaaa <span style={space}>bbbb</span>
            </text>
          </div>
        )
        return t.rect("text").height
      } finally {
        t.unmount()
      }
    }
    expect(heightOf({})).toBeCloseTo(LINE, 1)
    expect(heightOf({ paddingRight: 2 * column })).toBeCloseTo(2 * LINE, 1)
    expect(heightOf({ marginLeft: 2 * column })).toBeCloseTo(2 * LINE, 1)
    expect(
      heightOf({ paddingTop: 30, marginBottom: 30, borderTopWidth: 5, borderTopStyle: "solid" }),
      "vertical space moves no line"
    ).toBeCloseTo(LINE, 1)
  })

  it("washes a match that fills a span from its first glyph to its last, not the space around", () => {
    const column = columnWidth()
    const t = setup()
    try {
      t.render(
        <div highlight={{ query: "bb" }}>
          <text ref={t.ref("text")} style={TEXT}>
            aa <span style={SPACE}>bb</span>cc
          </text>
        </div>
      )
      const text = t.rect("text")
      const wash = t.renderer.getPaintedHighlights()[0]!.rects
      expect(wash).toHaveLength(1)
      expect(wash[0]!.x).toBeCloseTo(text.x + 3 * column + LEADING, 1)
      expect(wash[0]!.width).toBeCloseTo(2 * column, 1)
    } finally {
      t.unmount()
    }
  })

  it("takes the space of a hover style while the span is hovered", () => {
    const column = columnWidth()
    const t = setup()
    try {
      t.render(
        <div style={{ width: 400 }}>
          <text testId="p" style={TEXT}>
            aa <span style={{ hover: { paddingLeft: 20 } }}>bbbbbbbbbb</span>
            <span ref={t.ref("after")}>cc</span>
          </text>
        </div>
      )
      const before = t.rect("after").x
      const hover = t.at(8.5 * column)
      t.renderer.nativeSimulateMouseMove(hover.x, hover.y)
      expect(t.rect("after").x - before).toBeCloseTo(20, 1)
      const away = t.at(0.5 * column)
      t.renderer.nativeSimulateMouseMove(away.x, away.y)
      expect(t.rect("after").x).toBeCloseTo(before, 1)
    } finally {
      t.unmount()
    }
  })
})
