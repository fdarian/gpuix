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
import { describe, it } from "vitest"
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

describeNative("scroll containers as the baseline of their parent against Chrome", () => {
  for (const oracle of ORACLES) {
    for (const [name, source] of Object.entries(oracle.cases)) {
      it(`${name} at scale ${oracle.scale}`, () => {
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
