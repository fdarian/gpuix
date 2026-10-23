/// Where a text breaks into lines in a box of a given width is where Chrome breaks it, down to the widths at which a line
/// is within a hair of fitting. Blink's line breaker fits a line when the sum of the widths of its pieces (one per text
/// node), each from the start of the line and ceiled to 1/64 of a device pixel, is at most the width of the box cut to
/// 1/64 of a device pixel and one 1/64 more; a line that was `<= width + 1/1024` in floats broke a word early at those
/// widths, and `word-break: break-all` has its own places to break.
///
/// `fixtures/line-fit-oracle.html` is the page and `line-fit-oracle.json` and `line-fit-oracle-x2.json` the lines headless
/// Chrome puts each text in at the widths of its rows, at device scale 1 and 2 (regenerate with `fixtures/dsf-oracle.sh
/// line-fit`). The widths are edges where the old rule and Chrome's differ, real widths, and rows both agreed on. The font
/// of the Inter texts is `fixtures/inter-text-subset.ttf`.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const FONT = path.join(FIXTURES, "inter-text-subset.ttf")

type Case = {
  family: string
  size: number
  lh: string
  vs: string | null
  wordBreak: string
  overflowWrap: string
  set: string
  tag: string | null
  text: string
}

/** The case, the width of the box and the lines Chrome puts the text in. */
type Row = [number, number, string[]]

type Oracle = {
  cases: Case[]
  rows: Row[]
}

const SCALES = [
  { scaleFactor: 1, file: "line-fit-oracle.json" },
  { scaleFactor: 2, file: "line-fit-oracle-x2.json" },
] as const

const segmenter = new Intl.Segmenter("en", { granularity: "grapheme" })
const isSpace = (s: string) => s === " " || s === "\t" || s === "\n"

function styleOf(c: Case) {
  const style: Record<string, unknown> = {
    fontFamily: c.family,
    fontSize: c.size,
    lineHeight: c.lh.endsWith("px") ? c.lh : Number(c.lh),
    color: "#000000",
    wordBreak: c.wordBreak,
    overflowWrap: c.overflowWrap,
  }
  if (c.vs) style.fontVariationSettings = c.vs.replace(/'/g, '"')
  return style
}

const lineHeightOf = (c: Case) => Math.ceil(c.lh.endsWith("px") ? Number(c.lh.slice(0, -2)) : Number(c.lh) * c.size)

/** The offsets the lines start at: a painted character whose left edge is not to the right of the one before it. */
function startsOf(matches: { start: number; rects: { x: number; width: number }[] }[]) {
  const starts: number[] = []
  let previousLeft: number | null = null
  for (const match of [...matches].sort((a, b) => a.start - b.start)) {
    const rect = match.rects[0]
    if (rect === undefined || rect.width <= 0.001) continue
    if (previousLeft === null || rect.x <= previousLeft + 1e-4) starts.push(match.start)
    previousLeft = rect.x
  }
  return starts
}

function caseOf(oracle: Oracle, row: Row) {
  const found = oracle.cases[row[0]]
  if (found === undefined) throw new Error(`no case ${row[0]}`)
  return found
}

describeNative("a text breaks into lines where Chrome breaks it", () => {
  for (const scale of SCALES) {
    const oracle: Oracle = JSON.parse(fs.readFileSync(path.join(FIXTURES, scale.file), "utf8"))

    it(`puts every text in the lines of its ${oracle.rows.length} rows at scale ${scale.scaleFactor}`, { timeout: 120_000 }, () => {
      const root = createTestRoot({ width: 1700, height: 6000, scaleFactor: scale.scaleFactor, fonts: [FONT] })
      const mismatches: string[] = []
      try {
        let batch: Row[] = []
        let batchHeight = 0
        const flush = () => {
          if (batch.length === 0) return
          const rows = batch
          batch = []
          batchHeight = 0
          root.render(
            <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
              {rows.map((row, i) => {
                const c = caseOf(oracle, row)
                const ranges = [...segmenter.segment(c.text)]
                  .filter((g) => !isSpace(g.segment))
                  .map((g) => [g.index, g.index + g.segment.length] as [number, number])
                return (
                  <div key={i} style={{ width: row[1], flexShrink: 0 }} highlight={{ ranges }}>
                    <text testId={`t${i}`} style={styleOf(c)}>
                      {c.text}
                    </text>
                  </div>
                )
              })}
            </div>
          )
          const matches = root.renderer.getPaintedHighlights()
          rows.forEach((row, i) => {
            const c = caseOf(oracle, row)
            const element = root.renderer.findByTestId(`t${i}`)
            if (element === null) throw new Error(`row ${i} is not rendered`)
            const starts = startsOf(matches.filter((m) => m.elementId === element.id))
            const actual = starts.map((s, k) => c.text.slice(s, starts[k + 1]).replace(/[ \t]+$/, ""))
            if (JSON.stringify(actual) !== JSON.stringify(row[2])) {
              mismatches.push(`${c.set} ${c.family} ${c.size} ${c.wordBreak}/${c.overflowWrap} @${row[1]}: ${JSON.stringify(row[2])} != ${JSON.stringify(actual)}`)
            }
          })
        }
        for (const row of oracle.rows) {
          const height = row[2].length * lineHeightOf(caseOf(oracle, row)) + 4
          if (batchHeight + height > 5000) flush()
          batch.push(row)
          batchHeight += height
        }
        flush()
      } finally {
        root.unmount()
      }
      expect(mismatches).toEqual([])
    })
  }
})
