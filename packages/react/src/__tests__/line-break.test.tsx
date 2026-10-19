/// Where a line may break, against headless Chrome: `fixtures/line-break-oracle.html` is the page and
/// `fixtures/line-break-oracle.json` the rows Chrome lays each text out in, how wide each is, and the width of the text at
/// `min-content`. Chrome breaks as Blink's `LazyLineBreakIterator` does: a table for the pairs of ASCII and Latin-1
/// characters (a break after `-` and `?` and before `(`, `<`, `[` and `{`, a hyphen before a digit that breaks after a
/// letter or a digit), the Unicode line breaking algorithm for the rest, and `word-break: keep-all` on top.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const ORACLE: {
  column: number
  cases: Record<
    string,
    { text: string; columns: number; style: Record<string, string>; lines: string[]; widths: number[]; minContent: number }
  >
} = JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/line-break-oracle.json"), "utf8"))

const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo" } as const

/** The rows are the rects of a wash over all the text, and a row's width is the width of its rect. */
function rowWidthsOf(text: string, columns: number, style: Record<string, string>) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(
      <div highlight={{ query: text }} style={{ width: (columns + 0.5) * ORACLE.column }}>
        <text style={{ ...TEXT, ...style }}>{text}</text>
      </div>
    )
    const hit = renderer.getPaintedHighlights()[0]
    expect(hit, "the text is painted").toBeDefined()
    return hit!.rects.map((rect) => rect.width)
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
    return renderer.getElementBounds(renderer.findByTestId("box").id).width
  } finally {
    unmount()
  }
}

/**
 * How far a width may be from Chrome's. Menlo is the same font on both sides, so a width is exact; the fonts that CJK
 * and emoji fall back to are laid out a pixel or two apart by Chrome and gpuix, which is well under a character.
 */
const tolerance = (text: string) => (/^[\u0000-\u00ff\u2000-\u206f]*$/.test(text) ? 0.6 : 2)

describeNative("a line breaks where it does in Chrome", () => {
  for (const [name, source] of Object.entries(ORACLE.cases)) {
    it(`${name}: rows`, () => {
      const widths = rowWidthsOf(source.text, source.columns, source.style)
      const expected = `${JSON.stringify(source.lines)} are ${source.widths.map((width) => width.toFixed(1)).join(", ")}px`
      expect(widths.length, `${widths.map((width) => width.toFixed(1)).join(", ")}px, Chrome has ${expected}`).toBe(source.lines.length)
      source.widths.forEach((width, row) => {
        expect(Math.abs(widths[row] - width), `row ${row}: ${widths[row].toFixed(1)}px, Chrome has ${expected}`).toBeLessThan(
          tolerance(source.lines[row])
        )
      })
    })

    it(`${name}: min-content`, () => {
      const width = minContentOf(source.text, source.style)
      expect(Math.abs(width - source.minContent), `${width}px, Chrome has ${source.minContent}px`).toBeLessThan(tolerance(source.text))
    })
  }
})
