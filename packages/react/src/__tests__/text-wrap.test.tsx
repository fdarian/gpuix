/// CSS `text-wrap`, `text-wrap-mode` and `text-wrap-style`, through the real GPUI
/// paint pipeline. Menlo has one advance for every character, so a row is as wide
/// as its characters are many, and a highlight wash over the whole text has one
/// rect per row.

import path from "path"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { expectScreenshotsEqual, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo" } as const

function paint(node: React.ReactNode) {
  const testRoot = createTestRoot()
  testRoot.render(node)
  return testRoot
}

/** The width of one column of Menlo at `TEXT`'s size. */
function measureColumn() {
  const testRoot = paint(
    <div highlight={{ query: "aaaaaaaaaa" }}>
      <text style={TEXT}>{"aaaaaaaaaa"}</text>
    </div>
  )
  return testRoot.renderer.getPaintedHighlights()[0]!.rects[0]!.width / 10
}

const COLUMN = measureColumn()

/** The widths of the rows `text` is painted in, in columns. The wash covers the space
 *  that hangs at the end of a row, which makes a row that wraps a column wider. */
function rowWidths(testRoot: TestRoot) {
  const hit = testRoot.renderer.getPaintedHighlights()[0]
  expect(hit, "the text is painted").toBeDefined()
  return hit!.rects.map((rect) => Math.round(rect.width / COLUMN))
}

/** `text` in a box `columns` wide, whose style is `style`, with a wash over all of it. */
function rowsOf(
  text: string,
  columns: number,
  style: React.CSSProperties & Record<string, unknown>,
  box: Record<string, unknown> = {}
) {
  const testRoot = paint(
    <div highlight={{ query: text }} style={{ width: (columns + 0.5) * COLUMN, ...box }}>
      <text style={{ ...TEXT, ...style }}>{text}</text>
    </div>
  )
  return { testRoot, widths: rowWidths(testRoot) }
}

const spread = (widths: number[]) => Math.max(...widths) - Math.min(...widths)

describeNative("text-wrap", () => {
  describe("balance", () => {
    // Greedy: a line of 19 columns, then one word of 8.
    const HEADING = "aaaa bbbb cccc dddd eeeeeeee"
    const WORD = 8

    it("wraps a long line and a word into near-equal lines", () => {
      const auto = rowsOf(HEADING, 20, {}).widths
      const balanced = rowsOf(HEADING, 20, { textWrap: "balance" }).widths
      expect(auto).toHaveLength(2)
      expect(spread(auto)).toBeGreaterThanOrEqual(WORD)
      expect(balanced).toHaveLength(2)
      expect(spread(balanced)).toBeLessThan(WORD)
    })

    it("evens out a paragraph whose first lines are already full", () => {
      const text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mm"
      const auto = rowsOf(text, 20, {}).widths
      const balanced = rowsOf(text, 20, { textWrap: "balance" }).widths
      expect(auto).toHaveLength(4)
      expect(balanced).toHaveLength(4)
      expect(spread(auto)).toBeGreaterThan(4)
      expect(spread(balanced)).toBeLessThan(4)
    })

    it("does not balance a paragraph of more than six lines", () => {
      // Two words to a line, so seven lines, the last of them a short one.
      const text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mm"
      const auto = rowsOf(text, 9, {}).widths
      expect(auto).toHaveLength(7)
      expect(rowsOf(text, 9, { textWrap: "balance" }).widths).toEqual(auto)
    })

    it("balances a paragraph of six lines", () => {
      const text = "aa bbbbbbb ccccccccc dd eeeee ffff gggg hhh iiiiii jj kkk lll mm"
      const auto = rowsOf(text, 13, {}).widths
      const balanced = rowsOf(text, 13, { textWrap: "balance" }).widths
      expect(auto).toHaveLength(6)
      expect(balanced).toHaveLength(6)
      expect(spread(balanced)).toBeLessThan(spread(auto))
    })

    it("balances each run of text between line feeds as its own paragraph", () => {
      const rows = rowsOf(`${HEADING}\n${HEADING}`, 20, {
        whiteSpace: "pre-line",
        textWrap: "balance",
      }).widths
      expect(rows).toHaveLength(4)
      expect(spread(rows)).toBeLessThan(WORD)
    })

    it("takes the longhand as it takes the shorthand", () => {
      expect(rowsOf(HEADING, 20, { textWrapStyle: "balance" }).widths).toEqual(
        rowsOf(HEADING, 20, { textWrap: "balance" }).widths
      )
    })

    it("leaves a single line as it is", () => {
      expect(rowsOf("aaaa bbbb", 20, { textWrap: "balance" }).widths).toEqual([9])
    })
  })

  describe("pretty", () => {
    it("keeps a short word from being the last line", () => {
      const text = "aaaa bbbb cccc dddd ee"
      const auto = rowsOf(text, 20, {}).widths
      const pretty = rowsOf(text, 20, { textWrap: "pretty" }).widths
      expect(auto).toEqual([20, 2])
      expect(pretty).toHaveLength(2)
      expect(pretty[1]).toBeGreaterThanOrEqual(7)
    })

    it("moves as little as it takes in a longer paragraph", () => {
      const text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mm"
      const auto = rowsOf(text, 20, {}).widths
      const pretty = rowsOf(text, 20, { textWrapStyle: "pretty" }).widths
      expect(auto).toEqual([20, 20, 20, 2])
      expect(pretty).toHaveLength(4)
      expect(pretty[3]).toBeGreaterThan(2)
      expect(pretty.slice(0, 2)).toEqual([20, 20])
    })

    it("leaves a last line that is not a single word, or is long", () => {
      for (const text of ["aaaa bbbb cccc dddd e f", "aaaa bbbb cccc dddd eeeeeeee"]) {
        expect(rowsOf(text, 20, { textWrap: "pretty" }).widths).toEqual(
          rowsOf(text, 20, {}).widths
        )
      }
    })
  })

  describe("stable", () => {
    it("lays a text out as auto does", () => {
      const text = "aaaa bbbb cccc dddd eeeeeeee"
      for (const style of [{ textWrap: "stable" }, { textWrapStyle: "stable" }]) {
        expect(rowsOf(text, 20, style).widths).toEqual(rowsOf(text, 20, {}).widths)
      }
    })
  })

  describe("nowrap", () => {
    const text = "aaaa bbbb cccc dddd eeeeeeee"

    it("keeps the text on one line, as textWrap and as textWrapMode", () => {
      expect(rowsOf(text, 20, {}).widths).toHaveLength(2)
      expect(rowsOf(text, 20, { textWrap: "nowrap" }).widths).toHaveLength(1)
      expect(rowsOf(text, 20, { textWrapMode: "nowrap" }).widths).toHaveLength(1)
    })

    it("takes a mode and a style in one value", () => {
      expect(rowsOf(text, 20, { textWrap: "nowrap balance" }).widths).toHaveLength(1)
      expect(rowsOf(text, 20, { textWrap: "balance wrap" }).widths).toEqual(
        rowsOf(text, 20, { textWrap: "balance" }).widths
      )
    })

    it("wins over whiteSpace on the same element, whichever way it points", () => {
      expect(
        rowsOf(text, 20, { whiteSpace: "normal", textWrapMode: "nowrap" }).widths
      ).toHaveLength(1)
      expect(
        rowsOf(text, 20, { whiteSpace: "nowrap", textWrapMode: "wrap" }).widths
      ).toHaveLength(2)
    })

    it("leaves what whiteSpace does to spaces and newlines alone", () => {
      // `pre` does not wrap; the mode makes it `pre-wrap`.
      expect(rowsOf(text, 20, { whiteSpace: "pre" }).widths).toHaveLength(1)
      expect(rowsOf(text, 20, { whiteSpace: "pre", textWrapMode: "wrap" }).widths).toHaveLength(2)
      const kept = rowsOf("aaaa\nbbbb", 20, { whiteSpace: "pre-line", textWrapMode: "nowrap" })
      expect(kept.widths).toEqual([4, 4])
    })

    it("lets the style of a nowrap text alone", () => {
      expect(rowsOf(text, 20, { textWrapMode: "nowrap", textWrapStyle: "balance" }).widths).toEqual([
        28,
      ])
    })
  })

  describe("inheritance", () => {
    const text = "aaaa bbbb cccc dddd eeeeeeee"

    function inherited(parent: Record<string, unknown>, child: Record<string, unknown> = {}) {
      const testRoot = paint(
        <div style={{ width: 20.5 * COLUMN, ...parent }}>
          <div highlight={{ query: text }}>
            <text style={{ ...TEXT, ...child }}>{text}</text>
          </div>
        </div>
      )
      return rowWidths(testRoot)
    }

    it("passes textWrap, textWrapMode and textWrapStyle down", () => {
      const balanced = rowsOf(text, 20, { textWrap: "balance" }).widths
      expect(inherited({ textWrap: "balance" })).toEqual(balanced)
      expect(inherited({ textWrapStyle: "balance" })).toEqual(balanced)
      expect(inherited({ textWrap: "nowrap" })).toHaveLength(1)
      expect(inherited({ textWrapMode: "nowrap" })).toHaveLength(1)
    })

    it("lets a descendant take it back", () => {
      expect(inherited({ textWrapStyle: "balance" }, { textWrapStyle: "auto" })).toEqual(
        rowsOf(text, 20, {}).widths
      )
      expect(inherited({ textWrap: "nowrap" }, { textWrapMode: "wrap" })).toHaveLength(2)
    })

    it("resets what the shorthand leaves out, as a shorthand does", () => {
      // `balance` is `wrap balance`: it wraps a text its parent keeps on one line.
      expect(inherited({ textWrap: "nowrap" }, { textWrap: "balance" })).toEqual(
        rowsOf(text, 20, { textWrap: "balance" }).widths
      )
      // And `nowrap` is `nowrap auto`.
      expect(inherited({ textWrapStyle: "balance" }, { textWrap: "nowrap" })).toHaveLength(1)
    })

    it("lets a longhand win over the shorthand on the same element", () => {
      expect(
        rowsOf(text, 20, { textWrap: "balance", textWrapStyle: "auto" }).widths
      ).toEqual(rowsOf(text, 20, {}).widths)
      expect(
        rowsOf(text, 20, { textWrap: "nowrap", textWrapMode: "wrap" }).widths
      ).toHaveLength(2)
    })
  })

  describe("state styles", () => {
    it("applies textWrapStyle in a hover state", () => {
      const text = "aaaa bbbb cccc dddd eeeeeeee"
      const testRoot = createTestRoot()
      try {
        testRoot.render(
          <div
            highlight={{ query: text }}
            style={{ width: 20.5 * COLUMN, hover: { textWrap: "balance" } }}
          >
            <text style={TEXT}>{text}</text>
          </div>
        )
        testRoot.renderer.nativeSimulateMouseMove(900, 900)
        testRoot.renderer.captureScreenshot("/tmp/text-wrap-hover-before.png")
        const before = rowWidths(testRoot)
        testRoot.renderer.nativeSimulateMouseMove(10, 10)
        testRoot.renderer.captureScreenshot("/tmp/text-wrap-hover-after.png")
        const after = rowWidths(testRoot)
        expect(before).toEqual(rowsOf(text, 20, {}).widths)
        expect(after).toEqual(rowsOf(text, 20, { textWrap: "balance" }).widths)
      } finally {
        testRoot.unmount()
      }
    })

    it("applies textWrapMode in a hover state", () => {
      const text = "aaaa bbbb cccc dddd eeeeeeee"
      const testRoot = createTestRoot()
      try {
        testRoot.render(
          <div
            highlight={{ query: text }}
            style={{ width: 20.5 * COLUMN, hover: { textWrapMode: "nowrap" } }}
          >
            <text style={TEXT}>{text}</text>
          </div>
        )
        testRoot.renderer.nativeSimulateMouseMove(900, 900)
        testRoot.renderer.captureScreenshot("/tmp/text-wrap-mode-hover-before.png")
        expect(rowWidths(testRoot)).toHaveLength(2)
        testRoot.renderer.nativeSimulateMouseMove(10, 10)
        testRoot.renderer.captureScreenshot("/tmp/text-wrap-mode-hover-after.png")
        expect(rowWidths(testRoot)).toHaveLength(1)
      } finally {
        testRoot.unmount()
      }
    })
  })

  describe("the box", () => {
    const text = "aaaa bbbb cccc dddd eeeeeeee"

    function boxWidth(style: Record<string, unknown>) {
      const testRoot = paint(
        <div style={{ width: 400, alignItems: "flex-start", flexDirection: "column", display: "flex" }}>
          <div testId="box" style={{ maxWidth: 20.5 * COLUMN }}>
            <text style={{ ...TEXT, ...style }}>{text}</text>
          </div>
        </div>
      )
      const box = testRoot.renderer.findByTestId("box")
      return testRoot.renderer.getElementBounds(box!.id)!
    }

    it("is as wide as it is with greedy wrapping, and as high", () => {
      const auto = boxWidth({})
      for (const style of [{ textWrap: "balance" }, { textWrap: "pretty" }]) {
        const styled = boxWidth(style)
        expect(styled.width).toBeCloseTo(auto.width, 3)
        expect(styled.height).toBeCloseTo(auto.height, 3)
      }
      expect(auto.width).toBeCloseTo(20.5 * COLUMN, 0)
    })

    it("aligns the lines within the full width", () => {
      const width = 20.5 * COLUMN
      const INK = { ...TEXT, color: "#ffffff" } as const
      function shoot(name: string, node: React.ReactNode) {
        const testRoot = paint(node)
        const file = path.join(SHOTS_DIR, `text-wrap-${name}.png`)
        testRoot.renderer.captureScreenshot(file)
        return file
      }
      for (const textAlign of ["center", "right"]) {
        const balanced = shoot(
          `${textAlign}-balanced`,
          <div style={{ width, backgroundColor: "#101010" }}>
            <text style={{ ...INK, textAlign, textWrap: "balance" }}>{text}</text>
          </div>
        )
        // The same two rows, broken by hand, with no space left on them.
        const broken = shoot(
          `${textAlign}-broken`,
          <div style={{ width, backgroundColor: "#101010" }}>
            <div style={{ ...INK, textAlign }}>aaaa bbbb cccc</div>
            <div style={{ ...INK, textAlign }}>dddd eeeeeeee</div>
          </div>
        )
        expectScreenshotsEqual(balanced, broken)
      }
    })
  })
})
