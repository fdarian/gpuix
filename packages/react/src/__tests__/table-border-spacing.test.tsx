/// A fractional `border-spacing` against headless Chrome, at device scale 1 and 2: Chrome keeps the spacing as
/// whole device pixels, cut off, so `2.5px` is 2px on a display of scale 1 and 2.5 CSS px (5 device pixels) on one of
/// scale 2. `fixtures/table-border-spacing-oracle.html` is the page, `table-border-spacing-oracle.json` (scale 1)
/// and `table-border-spacing-oracle-x2.json` (scale 2) the boxes it lays out
/// (`sh fixtures/dsf-oracle.sh table-border-spacing`).

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf, expectBoxes } from "./oracle-boxes.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Oracle = Record<string, { html: string; boxes: Record<string, [number, number, number, number]> }>

const oracleOf = (file: string): Oracle =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLES = [
  { scale: 1, cases: oracleOf("table-border-spacing-oracle.json") },
  { scale: 2, cases: oracleOf("table-border-spacing-oracle-x2.json") },
]

describeNative("border-spacing with a fraction against Chrome", () => {
  for (const oracle of ORACLES) {
    for (const [name, source] of Object.entries(oracle.cases)) {
      it(`${name} at scale ${oracle.scale}`, () => {
        const { render, renderer, unmount } = createTestRoot()
        try {
          renderer.setWindowScaleFactor(oracle.scale)
          render(<div style={{ width: 300 }}>{elementsOf(source.html)}</div>)
          expectBoxes(renderer, source.boxes, 0.01)
        } finally {
          unmount()
        }
      })
    }
  }
})
