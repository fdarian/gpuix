/// Percentage heights of table rows, row groups and cells, against headless Chrome:
/// `fixtures/table-percent-heights-oracle.html` is the page and `fixtures/table-percent-heights-oracle.json` the
/// boxes Chrome lays out for each case in a 300px wide box.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf, expectBoxes } from "./oracle-boxes.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const ORACLE: Record<string, { html: string; boxes: Record<string, [number, number, number, number]> }> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/table-percent-heights-oracle.json"),
    "utf8"
  )
)

describeNative("percentage heights of table parts against Chrome", () => {
  for (const [name, source] of Object.entries(ORACLE)) {
    it(name, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        render(<div style={{ width: 300 }}>{elementsOf(source.html)}</div>)
        expectBoxes(renderer, source.boxes, 0)
      } finally {
        unmount()
      }
    })
  }
})
