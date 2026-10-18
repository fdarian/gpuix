/// `text-wrap-mode`, `white-space` and `text-wrap-style` on spans, against what headless Chrome does.
/// `fixtures/text-wrap-spans-oracle.html` is the page and `fixtures/text-wrap-spans-oracle.json` the
/// rows Chrome breaks each case into. Menlo has one advance for every character, so a row is as wide as
/// its characters are many and the wash over the whole text has one rect per row.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { nodesOf, styleOf, textOf } from "./oracle-html.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface OracleCase {
  columns: number
  blockStyle: string
  html: string
  rows: string[]
  gaps: boolean[]
}

const ORACLE: Record<string, OracleCase> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/text-wrap-spans-oracle.json"),
    "utf8"
  )
).cases

const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo" } as const

const COLUMN = (() => {
  const testRoot = createTestRoot()
  testRoot.render(
    <div highlight={{ query: "aaaaaaaaaa" }}>
      <text style={TEXT}>{"aaaaaaaaaa"}</text>
    </div>
  )
  const width = testRoot.renderer.getPaintedHighlights()[0]!.rects[0]!.width / 10
  testRoot.unmount()
  return width
})()

/** The widths in columns of the rows the text of `source` is painted in. */
function rowWidths(source: OracleCase, textStyle: object = {}) {
  const text = textOf(source.html)
  const testRoot = createTestRoot()
  try {
    testRoot.render(
      <div highlight={{ query: text }} style={{ width: (source.columns + 0.5) * COLUMN }}>
        <text style={{ ...TEXT, ...styleOf(source.blockStyle), ...textStyle } as never}>
          {nodesOf(source.html)}
        </text>
      </div>
    )
    const hit = testRoot.renderer.getPaintedHighlights()[0]
    expect(hit, "the text is painted").toBeDefined()
    // A span splits the wash of a row into rects: the width of a row is where its rects reach.
    const rows = new Map<number, { left: number; right: number }>()
    for (const rect of hit!.rects) {
      const row = rows.get(rect.y) ?? { left: Infinity, right: -Infinity }
      row.left = Math.min(row.left, rect.x)
      row.right = Math.max(row.right, rect.x + rect.width)
      rows.set(rect.y, row)
    }
    return [...rows.entries()]
      .sort(([a], [b]) => a - b)
      .map(([, row]) => Math.round((row.right - row.left) / COLUMN))
  } finally {
    testRoot.unmount()
  }
}

/** What the oracle's rows are as the wash sees them: a row that ends at a space hangs one column more. */
function expectedWidths(source: OracleCase) {
  return source.rows.map((row, ix) => row.length + (source.gaps[ix] ? 1 : 0))
}

/** Cases whose boxes are not text. */
const SKIPPED = /inline-block/

describeNative("text-wrap on spans, as Chrome", () => {
  for (const [name, source] of Object.entries(ORACLE)) {
    if (SKIPPED.test(name)) continue
    it(name, () => {
      expect(rowWidths(source)).toEqual(expectedWidths(source))
    })
  }
})

describeNative("text-wrap on spans, updated", () => {
  const source = ORACLE["nowrap span in the middle"]!

  it("breaks inside a span again once it stops being nowrap, and back", () => {
    const text = textOf(source.html)
    const testRoot = createTestRoot()
    const render = (mode: "nowrap" | "wrap") =>
      testRoot.render(
        <div highlight={{ query: text }} style={{ width: (source.columns + 0.5) * COLUMN }}>
          <text style={TEXT}>
            {"aaaa bbbb "}
            <span style={{ textWrapMode: mode }}>{"cccc dddd eeee"}</span>
            {" ffff"}
          </text>
        </div>
      )
    try {
      const widths = () =>
        testRoot.renderer.getPaintedHighlights()[0]!.rects.map((rect) => Math.round(rect.width / COLUMN))
      render("nowrap")
      expect(widths()).toEqual(expectedWidths(source))
      render("wrap")
      expect(widths()).toEqual([20, 9])
      render("nowrap")
      expect(widths()).toEqual(expectedWidths(source))
    } finally {
      testRoot.unmount()
    }
  })
})
