/// `vertical-align: baseline` cells whose content has no text, against headless Chrome at device scale 1 and 2: a cell
/// that has children but no baseline of its own is aligned by the bottom of its content, a cell with nothing in it takes
/// no part in aligning its row, and a row that no cell is aligned in has its baseline above the least bottom padding
/// and border of its cells, which is where an `inline-table` or a table in a flex row sits on the text beside it.
/// `fixtures/table-baseline-cell-oracle.html` is the page, `table-baseline-cell-oracle.json` (scale 1) and
/// `table-baseline-cell-oracle-x2.json` (scale 2) the boxes it lays out (`sh fixtures/dsf-oracle.sh table-baseline-cell`).
/// The text is GpuixTestSquare, every character one em wide with the whole em for the ascent and no descent.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf, expectBoxes } from "./oracle-boxes.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

type Oracle = Record<string, { html: string; boxes: Record<string, [number, number, number, number]> }>

const oracleOf = (file: string): Oracle =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLES = [
  { scale: 1, cases: oracleOf("table-baseline-cell-oracle.json") },
  { scale: 2, cases: oracleOf("table-baseline-cell-oracle-x2.json") },
]

/**
 * Rows gpuix lays out differently, each `it.fails` so that fixing one fails the test and takes it off the list. An empty
 * `<span></span>` is a child of the cell in Chrome (its cell is on the baseline, with the bottom of its content for the
 * baseline); gpuix builds no box for it, so the cell has nothing in it.
 */
const KNOWN_GAPS = new Set([
  "empty span with padding and border: next to text",
  "no baseline, a cell with an empty span and a cell aligned to the top: inline-table",
  "no baseline, a cell with an empty span and a cell aligned to the top: flex row",
])

describeNative("baseline cells without text against Chrome", () => {
  it("lists only rows of the oracle as known gaps", () => {
    for (const name of KNOWN_GAPS) expect(Object.keys(ORACLES[0]!.cases), name).toContain(name)
  })

  for (const oracle of ORACLES) {
    for (const [name, source] of Object.entries(oracle.cases)) {
      const test = KNOWN_GAPS.has(name) ? it.fails : it
      test(`${name} at scale ${oracle.scale}`, () => {
        const { render, renderer, unmount } = createTestRoot()
        try {
          renderer.setWindowScaleFactor(oracle.scale)
          renderer.registerFont(SQUARE_FONT)
          render(
            <div style={{ width: 300, fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" }}>
              {elementsOf(source.html)}
            </div>
          )
          expectBoxes(renderer, source.boxes, 0.02)
        } finally {
          unmount()
        }
      })
    }
  }
})
