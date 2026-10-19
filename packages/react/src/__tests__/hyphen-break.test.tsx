/// Line breaks after hyphens against headless Chrome: `fixtures/hyphen-break-oracle.html` is the page and
/// `fixtures/hyphen-break-oracle.json` the rows Chrome lays each text out in, and its width at `min-content`.
/// A line may break after U+002D, U+2010 and U+2013 in front of a letter or a digit, never before them, and so the
/// narrowest a hyphenated word goes is its longest segment.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const ORACLE: {
  column: number
  cases: Record<string, { text: string; columns: number; style: Record<string, string>; lines: string[]; minContent: number }>
} = JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/hyphen-break-oracle.json"), "utf8"))

const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo" } as const

/** The rows are the rects of a wash over all the text, and a row's width is how many characters it holds. */
function rowsOf(text: string, columns: number, style: Record<string, string>) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(
      <div highlight={{ query: text }} style={{ width: (columns + 0.5) * ORACLE.column }}>
        <text style={{ ...TEXT, ...style }}>{text}</text>
      </div>
    )
    const hit = renderer.getPaintedHighlights()[0]
    expect(hit, "the text is painted").toBeDefined()
    return hit!.rects.map((rect) => rect.width / ORACLE.column)
  } finally {
    unmount()
  }
}

function minContentOf(text: string, style: Record<string, string>) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(
      <div testId="box" style={{ width: "min-content" }}>
        <text style={{ ...TEXT, ...style }}>{text}</text>
      </div>
    )
    return renderer.getElementBounds(renderer.findByTestId("box").id).width / ORACLE.column
  } finally {
    unmount()
  }
}

/** Chrome's `lines` measured in columns: the width of a row of characters that are all one column wide. */
const columnsOf = (lines: string[]) => lines.map((line) => line.length)

describeNative("a line breaks after a hyphen as it does in Chrome", () => {
  for (const [name, source] of Object.entries(ORACLE.cases)) {
    it(`${name}: rows`, () => {
      const rows = rowsOf(source.text, source.columns, source.style)
      expect(rows.map(Math.round), `${JSON.stringify(source.lines)}`).toEqual(columnsOf(source.lines))
    })

    it(`${name}: min-content`, () => {
      const width = minContentOf(source.text, source.style)
      expect(Math.abs(width - source.minContent), `${width} columns, Chrome has ${source.minContent}`).toBeLessThan(0.05)
    })
  }
})
