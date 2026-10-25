/// The width of a flex row that is fitted to its items against headless Chrome, at device scale 1 and 2: the row is
/// `min(max-content, max(min-content, available))` wide, and its max-content and min-content widths are the sums of the
/// contributions of its items, the `width` of an item or its content between its `min-width` and `max-width`. The flex basis
/// does not enter, so `flex: 1 1 0` around a 100px box is 100px wide and not 0 (Blink `ComputeMinMaxSizeOfRowContainer`).
/// An item that cannot move (it would have to grow with `flex-grow: 0`, or shrink with `flex-shrink: 0`, to its
/// contribution, and its basis is a length) is its hypothetical main size: `flex: 1 0 150px` is 150px wide.
/// The row is an absolutely positioned box, an inline-flex, a `width: fit-content`, `max-content` or `min-content` box, an
/// item of a column that aligns it at the start, or the item of a `max-content` grid column. A float is sized the same way
/// (gpuix has no `float` style: taffy's `row_fit_tests` have it).
/// `fixtures/flex-row-fit-oracle.html` is the page, `flex-row-fit-oracle.json` (scale 1) and `flex-row-fit-oracle-x2.json`
/// (scale 2) the boxes it lays out (`sh fixtures/flex-row-fit-oracle.sh`).

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import React from "react"
import { describe, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf, expectBoxes } from "./oracle-boxes.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Oracle = Record<string, { html: string; boxes: Record<string, [number, number, number, number]> }>

const fixture = (file: string) => path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file)
const oracleOf = (file: string): Oracle => JSON.parse(fs.readFileSync(fixture(file), "utf8"))
const SQUARE_FONT = fs.readFileSync(fixture("gpuix-test-square.ttf"))

const ORACLES = [
  { scale: 1, cases: oracleOf("flex-row-fit-oracle.json") },
  { scale: 2, cases: oracleOf("flex-row-fit-oracle-x2.json") },
]

describeNative("the width of a flex row fitted to its items against Chrome", () => {
  for (const oracle of ORACLES) {
    for (const [name, source] of Object.entries(oracle.cases)) {
      it(`${name} at scale ${oracle.scale}`, () => {
        const { render, renderer, unmount } = createTestRoot({ width: 1280, height: 800 })
        try {
          renderer.registerFont(SQUARE_FONT)
          renderer.setWindowScaleFactor(oracle.scale)
          render(
            <div style={{ position: "relative", width: 700, fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000", wordBreak: "break-all" }}>
              {elementsOf(source.html)}
            </div>,
          )
          expectBoxes(renderer, source.boxes, 0.01)
        } finally {
          unmount()
        }
      })
    }
  }
})
