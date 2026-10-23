/// The automatic height of a flex column against headless Chrome, at device scale 1 and 2: the sum of its items'
/// hypothetical main sizes, the flex basis clamped by the min and max height. The content of an item enters only through
/// its `min-height: auto`, so `flex: 1 1 0` with `min-height: 0` is as high as its padding, not as its content (gpuix#96).
/// `fixtures/flex-column-size-oracle.html` is the page, `flex-column-size-oracle.json` (scale 1) and
/// `flex-column-size-oracle-x2.json` (scale 2) the boxes it lays out (`sh fixtures/dsf-oracle.sh flex-column-size`).

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
  { scale: 1, cases: oracleOf("flex-column-size-oracle.json") },
  { scale: 2, cases: oracleOf("flex-column-size-oracle-x2.json") },
]

describeNative("the height of a flex column of automatic height against Chrome", () => {
  for (const oracle of ORACLES) {
    for (const [name, source] of Object.entries(oracle.cases)) {
      it(`${name} at scale ${oracle.scale}`, () => {
        const { render, renderer, unmount } = createTestRoot({ width: 1280, height: 800 })
        try {
          renderer.setWindowScaleFactor(oracle.scale)
          render(<div style={{ width: 700 }}>{elementsOf(source.html)}</div>)
          expectBoxes(renderer, source.boxes, 0.01)
        } finally {
          unmount()
        }
      })
    }
  }
})
