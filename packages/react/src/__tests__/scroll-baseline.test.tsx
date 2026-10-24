/// A box with `overflow` other than `visible` or `clip` is a scroll container, and what a parent takes a baseline from
/// it is clamped to its border box, against headless Chrome at device scale 1 and 2: a cell beside a cell of text, an
/// inline-table, a flex row or grid that aligns by baseline, a block, flex container or grid around the box, and an
/// inline-block, inline-flex or inline-grid on a line of text, whose box or whose child is the scroll container.
/// `fixtures/scroll-baseline-oracle.html` is the page, `scroll-baseline-oracle.json` (scale 1) and
/// `scroll-baseline-oracle-x2.json` (scale 2) the boxes it lays out (`sh fixtures/dsf-oracle.sh scroll-baseline`).
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
  { scale: 1, cases: oracleOf("scroll-baseline-oracle.json") },
  { scale: 2, cases: oracleOf("scroll-baseline-oracle-x2.json") },
]

/**
 * Rows gpuix lays out differently, each `it.fails` so that fixing one fails the test and takes it off the list. None
 * is about a scroll container: they are `visible` and `clip` boxes whose line is not inside the box.
 *  - A table cell holding a 10px block with a 20px line in it, or a line 30px above it (`margin-top: -30px`): the
 *    baseline is outside the cell's content, and the row (and the inline-table or flex row it sits in) is taller
 *    in gpuix than in Chrome.
 *  - A block in a flex row or grid holding a box with a line 30px above it: the line is 60 high in gpuix and 50 in
 *    Chrome.
 */
const KNOWN_GAPS = new Set<string>([
  "block in a flex row, overflow clip, text above the box",
  "block in a flex row, overflow visible, text above the box",
  "block in a grid, overflow clip, text above the box",
  "block in a grid, overflow visible, text above the box",
  "inline-table, overflow clip, text above the box",
  "inline-table, overflow clip, text in a short box",
  "inline-table, overflow visible, text above the box",
  "inline-table, overflow visible, text in a short box",
  "table in a flex row, overflow clip, text above the box",
  "table in a flex row, overflow clip, text in a short box",
  "table in a flex row, overflow visible, text above the box",
  "table in a flex row, overflow visible, text in a short box",
])

describeNative("scroll containers as the baseline of their parent against Chrome", () => {
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
