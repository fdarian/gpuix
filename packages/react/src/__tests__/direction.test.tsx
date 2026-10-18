/// CSS `direction: rtl` against headless Chrome: `fixtures/direction-oracle.html` is the page and
/// `fixtures/direction-oracle.json` the boxes Chrome lays out for each case in a 300px wide box with
/// `direction: rtl`.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf, expectBoxes } from "./oracle-boxes.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const ORACLE: Record<string, { html: string; boxes: Record<string, [number, number, number, number]> }> = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/direction-oracle.json"), "utf8")
)

/** Cases the boxes of Chrome and gpuix cannot be compared in. */
const SKIPPED = new Set(["a scroller has its scrollbar on the left"])

/** Cases in which Chrome has columns of a fraction of a pixel where a collapsed table has whole device pixels. */
const TOLERANCE: Record<string, number> = { "collapsed table with colspan": 0.3 }

function check(name: string, tolerance?: number) {
  const source = ORACLE[name]!
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(
      <div testId="case" style={{ width: 300, direction: "rtl", fontFamily: "Menlo", fontSize: 16, lineHeight: "20px" }}>
        {elementsOf(source.html)}
      </div>
    )
    expectBoxes(renderer, source.boxes, tolerance)
  } finally {
    unmount()
  }
}

describeNative("direction: rtl against Chrome", () => {
  for (const name of Object.keys(ORACLE)) {
    if (SKIPPED.has(name)) continue
    it(name, () => check(name, TOLERANCE[name]))
  }
})
