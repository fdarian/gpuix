/// The min-content width of a paragraph follows the rules its lines break by: the
/// longest word, one letter where a word may break anywhere, and the space of the
/// inline boxes in it, with the lengths of the style.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** A text is as wide as its advance up to 1/64 of a pixel, and the box is painted on device pixels. */
function expectNear(actual: number, advance: number, message?: string) {
  const laidOut = Math.ceil(advance * 64 - 0.064) / 64
  expect(Math.abs(actual - laidOut), `${message ?? "width"}: ${actual} != ${laidOut}`).toBeLessThanOrEqual(0.5)
}

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const

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

/** The bounds of a `<div style={{ width: "min-content" }}>` holding `children`. */
function minContentBox(children: React.ReactNode, boxStyle: object = {}) {
  const root = createTestRoot({ width: 600, height: 300 })
  try {
    root.render(
      <div style={{ width: 600 }}>
        <div testId="box" style={{ width: "min-content", ...boxStyle }}>
          {children}
        </div>
      </div>
    )
    const box = root.renderer.findByTestId("box")!
    return root.renderer.getElementBounds(box.id)!
  } finally {
    root.unmount()
  }
}

describeNative("min-content width of a paragraph", () => {
  const PROSE = "aa bbbbbb cccc"

  it("is the longest word, without the space that hangs after it", () => {
    const column = columnWidth()
    const box = minContentBox(<text style={TEXT}>{PROSE}</text>)
    expectNear(box.width, 6 * column)
    expect(box.height).toBeCloseTo(3 * LINE, 1)
  })

  it("is one letter with overflowWrap: anywhere, and the longest word with normal and break-word", () => {
    const column = columnWidth()
    const width = (overflowWrap: "normal" | "break-word" | "anywhere") =>
      minContentBox(<text style={{ ...TEXT, overflowWrap }}>{PROSE}</text>).width
    expectNear(width("normal"), 6 * column)
    // break-word breaks a word only once it does not fit, so it is no narrower.
    expect(width("break-word")).toBeCloseTo(width("normal"), 1)
    expectNear(width("anywhere"), column)
    expect(width("anywhere")).toBeLessThan(width("normal"))
  })

  it("is one letter with wordBreak: break-all or break-word", () => {
    const column = columnWidth()
    for (const wordBreak of ["break-all", "break-word"] as const) {
      const box = minContentBox(<text style={{ ...TEXT, wordBreak }}>{PROSE}</text>)
      expectNear(box.width, column, wordBreak)
    }
    // keep-all only keeps CJK together: text in letters is as wide as without it.
    const kept = minContentBox(<text style={{ ...TEXT, wordBreak: "keep-all" }}>{PROSE}</text>)
    expectNear(kept.width, 6 * column)
  })

  it("keeps the trailing space of a word when spaces do not hang", () => {
    const column = columnWidth()
    const box = minContentBox(<text style={{ ...TEXT, whiteSpace: "break-spaces" }}>{"aaaa bb"}</text>)
    // break-spaces keeps the space at the end of the line, and it takes room.
    expectNear(box.width, 5 * column)
    const hung = minContentBox(<text style={TEXT}>{"aaaa bb"}</text>)
    expectNear(hung.width, 4 * column)
  })

  it("counts the tab stops of the text", () => {
    const column = columnWidth()
    const box = minContentBox(
      <text style={{ ...TEXT, whiteSpace: "pre-wrap", tabSize: 4 }}>{"ab\tc"}</text>
    )
    // The tab goes to the stop at 4 columns, and it is a place to break after it; the
    // spaces and tabs at the end of a line hang in pre-wrap, so the line is "ab".
    expectNear(box.width, 2 * column)
    const unhung = minContentBox(
      <text style={{ ...TEXT, whiteSpace: "break-spaces", tabSize: 4 }}>{"ab\tc"}</text>
    )
    expectNear(unhung.width, 4 * column)
  })

  it("counts the space of the inline boxes in it, in the units of the style", () => {
    const column = columnWidth()
    const box = minContentBox(
      <text style={{ ...TEXT, fontSize: 20 }}>
        aa <span style={{ paddingLeft: "1em", paddingRight: "calc(0.5em + 2px)" }}>bb</span>
      </text>
    )
    const col20 = (column * 20) / 16
    // The span's line is its two letters and its padding: 20 + 12 around them.
    expectNear(box.width, 2 * col20 + 20 + 12)
  })
})
