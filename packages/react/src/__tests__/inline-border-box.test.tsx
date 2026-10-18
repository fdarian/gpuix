/// The box of an inline element with space around it is its border box: the
/// padding and border are in, the margin is out. That is the bounds it reports,
/// the ring of its outline, where the pointer is on it and its cursor.

import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

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
/** What a span is tall: the content area of Menlo at 16px, not the 20px line. Chrome rounds
 *  its ascent (14.85px) and descent (3.77px) to whole pixels: 15 and 4. */
const CONTENT = 19
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

describeNative("border box of an inline element", () => {
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
    const click = (dx: number, line = 0) => {
      const point = at(dx, line)
      root.renderer.nativeSimulateClick(point.x, point.y)
    }
    const cursorAt = (dx: number, line = 0) => {
      const point = at(dx, line)
      root.renderer.nativeSimulateMouseMove(point.x, point.y)
      return root.renderer.getCursor()
    }
    return { ...root, ref, rect, at, click, cursorAt }
  }

  it("reports the border box: the padding and border in, the margin out", () => {
    const t = setup()
    try {
      t.render(
        <div style={{ width: 400 }}>
          <text ref={t.ref("text")} style={TEXT}>
            aa <span ref={t.ref("span")} style={SPACE}>bb</span>cc
          </text>
        </div>
      )
      const column = columnWidth()
      const text = t.rect("text")
      const span = t.rect("span")
      expect(span.x).toBeCloseTo(text.x + 3 * column + 10, 1)
      expect(span.width).toBeCloseTo(2 * column + 2 + 4 + 1 + 3, 1)
      expect(span.right).toBeCloseTo(text.x + 5 * column + LEADING + TRAILING - 6, 1)
      expect(span.height).toBeCloseTo(CONTENT, 1)
    } finally {
      t.unmount()
    }
  })

  it("keeps the outer border box around an inner one that starts and ends with it", () => {
    const t = setup()
    try {
      t.render(
        <div style={{ width: 400 }}>
          <text ref={t.ref("text")} style={TEXT}>
            aa{" "}
            <span ref={t.ref("outer")} style={{ marginLeft: 2, paddingLeft: 3, paddingRight: 5, marginRight: 7 }}>
              <span
                ref={t.ref("inner")}
                style={{
                  marginLeft: 11,
                  borderLeftWidth: 1,
                  borderRightWidth: 2,
                  borderLeftStyle: "solid",
                  borderRightStyle: "solid",
                  marginRight: 13,
                }}
              >
                bb
              </span>
            </span>
          </text>
        </div>
      )
      const column = columnWidth()
      const text = t.rect("text")
      const outer = t.rect("outer")
      const inner = t.rect("inner")
      // Outside in: margin 2, then padding 3, margin 11, border 1, "bb", border 2,
      // margin 13, padding 5, margin 7.
      expect(outer.x).toBeCloseTo(text.x + 3 * column + 2, 1)
      expect(outer.width).toBeCloseTo(3 + 11 + 1 + 2 * column + 2 + 13 + 5, 1)
      expect(inner.x).toBeCloseTo(text.x + 3 * column + 2 + 3 + 11, 1)
      expect(inner.width).toBeCloseTo(1 + 2 * column + 2, 1)
    } finally {
      t.unmount()
    }
  })

  it("puts the leading space on the first line of a span that wraps and the trailing on the last", () => {
    const column = columnWidth()
    const t = setup()
    try {
      t.render(
        <div style={{ width: 400 }}>
          <text ref={t.ref("text")} style={{ ...TEXT, width: Math.ceil(11.5 * column) }}>
            ab{" "}
            <span ref={t.ref("span")} style={{ marginLeft: 8, paddingRight: 20 }}>
              cdefg hijk
            </span>
          </text>
        </div>
      )
      // "ab " and 8 more: cdefg fits on the first line, hijk and its 20 are the second.
      const text = t.rect("text")
      const span = t.rect("span")
      expect(text.height).toBeCloseTo(2 * LINE, 1)
      expect(span.x, "the second line has no leading space").toBeCloseTo(text.x, 1)
      expect(span.right - text.x, "and the first one is the wider").toBeGreaterThan(8 * column + 8 - 0.01)
      expect(span.right - text.x).toBeLessThan(10 * column + 8 + 0.01)
    } finally {
      t.unmount()
    }
  })

  describe("the pointer", () => {
    function targets(span: object) {
      const seen: string[] = []
      const t = setup()
      const hear = (name: string) => (event: { target: unknown; currentTarget: unknown }) => {
        if (name === "span" || event.target === event.currentTarget) seen.push(name)
      }
      t.render(
        <div style={{ width: 400 }}>
          <text testId="p" style={TEXT} onClick={hear("text")}>
            aa <span style={span} onClick={hear("span")}>bb</span>cc
          </text>
        </div>
      )
      return { ...t, seen }
    }

    it("is on a span in its border and padding and on the text around it in its margin", () => {
      const column = columnWidth()
      const t = targets(SPACE)
      try {
        const start = 3 * column
        t.click(start + 5) // margin
        t.click(start + 10 + 1) // border
        t.click(start + 10 + 2 + 2) // padding
        t.click(start + LEADING + 0.5 * column) // glyph
        const end = start + LEADING + 2 * column
        t.click(end + 1) // padding
        t.click(end + 3 + 0.5) // border
        t.click(end + 3 + 1 + 3) // margin
        expect(t.seen).toEqual(["text", "span", "span", "span", "span", "span", "text"])
      } finally {
        t.unmount()
      }
    })

    it("measures offsetX from the padding edge of the border box, after the margin", () => {
      const column = columnWidth()
      const offsets: number[] = []
      const t = setup()
      try {
        t.render(
          <div style={{ width: 400 }}>
            <text testId="p" style={TEXT}>
              aa <span style={SPACE} onClick={(event) => offsets.push(event.offsetX)}>bb</span>
            </text>
          </div>
        )
        const x = 3 * column + LEADING + 0.5 * column
        t.click(x)
        // The border box starts after the 10 pixels of margin, and the 2 of border
        // are not in the padding box.
        expect(offsets[0]).toBeCloseTo(x - 3 * column - 10 - 2, 1)
      } finally {
        t.unmount()
      }
    })

    it("is on the first line of a wrapped span after its margin, and from the edge on the second", () => {
      const column = columnWidth()
      const seen: string[] = []
      const t = setup()
      try {
        t.render(
          <div style={{ width: 400 }}>
            <text testId="p" style={{ ...TEXT, width: Math.ceil(11.5 * column) }}>
              ab{" "}
              <span style={{ marginLeft: 8, marginRight: 20 }} onClick={() => seen.push("span")}>
                cdefg hijk
              </span>
            </text>
          </div>
        )
        t.click(3 * column + 4, 0) // the margin before c
        t.click(3 * column + 8 + 0.5 * column, 0) // c
        t.click(0.5 * column, 1) // h, with nothing before it on the line
        t.click(4 * column + 10, 1) // after k: the margin
        expect(seen).toEqual(["span", "span"])
      } finally {
        t.unmount()
      }
    })

    it("shows the span's cursor over its border box and not over its margin", () => {
      const column = columnWidth()
      const t = setup()
      try {
        t.render(
          <div style={{ width: 400 }}>
            <text testId="p" style={TEXT}>
              aa <span style={{ ...SPACE, cursor: "pointer" }}>bb</span>cc
            </text>
          </div>
        )
        const start = 3 * column
        expect(t.cursorAt(start + 5), "margin").toBe("default")
        expect(t.cursorAt(start + 10 + 1), "border").toBe("pointer")
        expect(t.cursorAt(start + 10 + 2 + 2), "padding").toBe("pointer")
        expect(t.cursorAt(start + LEADING + 0.5 * column), "glyph").toBe("pointer")
        const end = start + LEADING + 2 * column
        expect(t.cursorAt(end + 1), "padding after").toBe("pointer")
        expect(t.cursorAt(end + 3 + 1 + 3), "margin after").toBe("default")
        expect(t.cursorAt(end + TRAILING + 0.5 * column), "the text after").toBe("default")
      } finally {
        t.unmount()
      }
    })

    it("shows the cursor of the span around a nested one in the nested one's margin", () => {
      const column = columnWidth()
      const t = setup()
      try {
        t.render(
          <div style={{ width: 400 }}>
            <text testId="p" style={TEXT}>
              <span style={{ cursor: "pointer" }}>
                ab
                <span style={{ cursor: "text", marginLeft: 12, paddingLeft: 4 }}>cd</span>
              </span>
            </text>
          </div>
        )
        expect(t.cursorAt(2 * column + 6), "the margin of the inner box").toBe("pointer")
        expect(t.cursorAt(2 * column + 12 + 2), "its padding").toBe("text")
        expect(t.cursorAt(2 * column + 16 + 0.5 * column), "its glyph").toBe("text")
      } finally {
        t.unmount()
      }
    })
  })

  it("rings the border box of a focused span, and not its margin", () => {
    const column = columnWidth()
    const t = setup()
    try {
      t.render(
        <div style={{ width: 300, height: 100, padding: 20, backgroundColor: "#ffffff" }}>
          <text testId="p" style={TEXT}>
            aa{" "}
            <span
              tabIndex={0}
              style={{ marginLeft: 12, marginRight: 12, focusVisible: { outlineWidth: 4, outlineStyle: "solid", outlineColor: "#ff0000" } }}
            >
              bb
            </span>
          </text>
        </div>
      )
      t.renderer.simulateKeystrokes("Tab")
      const file = `${SHOTS_DIR}/inline-space-ring.png`
      t.renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, t.renderer)
      const box = t.renderer.getElementBounds(t.renderer.findByTestId("p")!.id)!
      const pixel = (dx: number) => pixels.at(Math.floor(box.x + dx), Math.floor(box.y + 10))
      const border = 3 * column + 12
      expect(pixel(border - 2), "just left of the border box").toEqual([255, 0, 0, 255])
      expect(pixel(border - 8), "in the margin").toEqual([255, 255, 255, 255])
      expect(pixel(3 * column - 2), "where a ring round the margin would be").toEqual([255, 255, 255, 255])
    } finally {
      t.unmount()
    }
  })
})

describeNative("lengths of an inline element", () => {
  function setup() {
    columnWidth()
    const refs = new Map<string, PublicInstance>()
    const ref = (name: string) => (instance: PublicInstance | null) => {
      if (instance) refs.set(name, instance)
    }
    const root = createTestRoot({ width: 500, height: 200 })
    return { ...root, ref, rect: (name: string) => refs.get(name)!.getBoundingClientRect() }
  }

  it("takes em from the font size of its text, rem from the root and calc() of them", () => {
    const column = columnWidth()
    const t = setup()
    try {
      t.render(
        <div style={{ width: 400 }}>
          <text style={{ ...TEXT, fontSize: 20, lineHeight: "24px" }}>
            aa{" "}
            <span
              ref={t.ref("em")}
              style={{ paddingLeft: "1em", paddingRight: "0.5em", marginRight: "calc(1em + 2px)" }}
            >
              bb
            </span>{" "}
            <span ref={t.ref("rem")} style={{ paddingLeft: "2rem", borderLeftWidth: "0.25em", borderLeftStyle: "solid" }}>
              cc
            </span>
          </text>
        </div>
      )
      const wide = (column * 20) / 16
      expect(t.rect("em").width).toBeCloseTo(2 * wide + 20 + 10, 1)
      // The margin takes space but is outside the box: the next word starts after it.
      const em = t.rect("em")
      const rem = t.rect("rem")
      expect(rem.x - em.right).toBeCloseTo(22 + wide, 1)
      expect(rem.width).toBeCloseTo(2 * wide + 32 + 5, 1)
    } finally {
      t.unmount()
    }
  })

  it("takes a percentage of the width the paragraph is laid out in", () => {
    const column = columnWidth()
    for (const width of [400, 200]) {
      const t = setup()
      try {
        t.render(
          <div style={{ width }}>
            <text style={TEXT}>
              aa{" "}
              <span ref={t.ref("span")} style={{ paddingLeft: "10%", marginRight: "calc(5% + 1px)" }}>
                bb
              </span>
              cc
            </text>
          </div>
        )
        const span = t.rect("span")
        expect(span.width, `in ${width}`).toBeCloseTo(2 * column + width / 10, 1)
      } finally {
        t.unmount()
      }
    }
  })
})
