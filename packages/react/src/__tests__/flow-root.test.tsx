/// `display: flow-root` is the root of a formatting context of its own, against headless Chrome at device scale 1 and 2:
/// the margins of its children stay inside it and make it tall, they do not collapse with its own margins, and an
/// empty one does not let its margins through. `fixtures/flow-root-oracle.html` is the page, with the same boxes as
/// `display: block` to tell the two apart, `flow-root-oracle.json` (scale 1) and `flow-root-oracle-x2.json` (scale 2)
/// the boxes it lays out (`sh fixtures/dsf-oracle.sh flow-root`).

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
  { scale: 1, cases: oracleOf("flow-root-oracle.json") },
  { scale: 2, cases: oracleOf("flow-root-oracle-x2.json") },
]

describeNative("display: flow-root against Chrome", () => {
  for (const oracle of ORACLES) {
    for (const [name, source] of Object.entries(oracle.cases)) {
      it(`${name} at scale ${oracle.scale}`, () => {
        const { render, renderer, unmount } = createTestRoot()
        try {
          renderer.setWindowScaleFactor(oracle.scale)
          render(<div style={{ width: 300 }}>{elementsOf(source.html)}</div>)
          expectBoxes(renderer, source.boxes, 0.02)
        } finally {
          unmount()
        }
      })
    }
  }
})
