/// An empty `<span>` with margin, border or padding along the line, against headless Chrome at device scale 1 and 2: it takes
/// the space of its insets, makes a line of the strut's height with its box on it, and a line breaks around it where it
/// would without it. One with no space along the line (or vertical padding alone) makes no line, as before.
/// `fixtures/empty-inline-box-oracle.html` is the page, `empty-inline-box-oracle.json` (scale 1) and
/// `empty-inline-box-oracle-x2.json` (scale 2) the boxes it lays out (`sh fixtures/dsf-oracle.sh empty-inline-box`).
/// The text is GpuixTestSquare, which has `A`, `B` and `m` and no space: a space is the fallback font's, 5 px wide in
/// Chrome and 5.55 in gpuix, so a case with spaces is compared to within that.

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
  { scale: 1, cases: oracleOf("empty-inline-box-oracle.json") },
  { scale: 2, cases: oracleOf("empty-inline-box-oracle-x2.json") },
]

/** How far a case with `html` may be from Chrome: a space in the markup is 0.55 px wider in gpuix. */
const toleranceOf = (html: string) => 0.06 + 0.6 * (html.replace(/<[^>]*>/g, "").match(/ /g)?.length ?? 0)

/** Rows gpuix lays out differently, each `it.fails` so that fixing one takes it off the list. */
const KNOWN_GAPS: RegExp[] = []
const isKnownGap = (name: string) => KNOWN_GAPS.some((gap) => gap.test(name))

describeNative("an empty inline box that takes space along the line against Chrome", () => {
  it("has a case for every kind of box and place", () => {
    expect(Object.keys(ORACLES[0]!.cases).length).toBeGreaterThan(400)
  })

  for (const oracle of ORACLES) {
    for (const [name, source] of Object.entries(oracle.cases)) {
      const test = isKnownGap(name) ? it.fails : it
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
          expectBoxes(renderer, source.boxes, toleranceOf(source.html))
        } finally {
          unmount()
        }
      })
    }
  }
})
