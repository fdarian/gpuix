/// Where the underline of a decorating box is drawn, and how thick, against headless Chrome:
/// `fixtures/text-decoration-oracle.html` is the page (`text-decoration-oracle.sh` regenerates
/// `fixtures/text-decoration-oracle.json`). Each case is one line of text in a band; the oracle holds the
/// rows of red (or blue) underline pixels of every `data-seg` stretch of it, in device pixels, at the
/// scale of the window the test renderer has.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { nodesOf, styleOf } from "./oracle-html.js"
import { readPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface Marks {
  red?: number[]
  blue?: number[]
}
interface OracleCase {
  html: string
  bandStyle: string
  segs: Record<string, { rect: [number, number, number, number] } & Record<string, Marks | [number, number, number, number]>>
}

const ORACLE: Record<string, OracleCase> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/text-decoration-oracle.json"),
    "utf8"
  )
).cases

let shots = 0

/** What `case`'s band has painted: for each `data-seg`, the rows of its red and blue pixels. */
function painted(source: OracleCase): { scale: number; segs: Record<string, Marks> } {
  const testRoot = createTestRoot()
  try {
    testRoot.render(
      <div style={{ width: 600, height: 60, backgroundColor: "#ffffff" }}>
        <text
          style={
            {
              width: 600,
              paddingTop: 10,
              color: "#000000",
              fontFamily: "Helvetica",
              fontSize: 16,
              lineHeight: "24px",
              ...styleOf(source.bandStyle),
            } as never
          }
        >
          {nodesOf(source.html)}
        </text>
      </div>
    )
    const file = `${SHOTS_DIR}/text-decoration-oracle-${shots++}.png`
    testRoot.renderer.captureScreenshot(file)
    const pixels = readPixels(file)
    const scale = pixels.width / testRoot.renderer.getWindowSize().width
    const segs: Record<string, Marks> = {}
    for (const [name, seg] of Object.entries(source.segs)) {
      const [left, , right] = seg.rect
      const x0 = Math.floor((left + 1) * scale)
      const x1 = Math.floor((right - 1) * scale)
      const marks: Marks = {}
      for (let y = 0; y < 60 * scale; y++) {
        let red = 0
        let blue = 0
        for (let x = x0; x < x1; x++) {
          const [r, g, b] = pixels.at(x, y)
          if (r > 200 && g < 120 && b < 120) red++
          if (b > 200 && r < 120 && g < 120) blue++
        }
        if (red >= 3) (marks.red ??= []).push(y)
        if (blue >= 3) (marks.blue ??= []).push(y)
      }
      segs[name] = marks
    }
    return { scale, segs }
  } finally {
    testRoot.unmount()
  }
}

/** The oracle's marks of a segment at `scale`. */
function chrome(source: OracleCase, seg: string, scale: number): Marks {
  const marks = source.segs[seg]![`x${scale}`] as Marks | undefined
  expect(marks, `the oracle has no x${scale} for ${seg}`).toBeDefined()
  return marks!
}

describeNative("underlines against Chrome", () => {
  for (const [name, source] of Object.entries(ORACLE)) {
    if (!/^(parent|nested|raised|inner|plain|thickness)/.test(name)) continue
    it(name, () => {
      const { scale, segs } = painted(source)
      for (const seg of Object.keys(source.segs)) {
        const expected = chrome(source, seg, scale)
        // Chrome cuts an underline where it crosses the ink of a glyph (`text-decoration-skip-ink: auto`),
        // which leaves it nothing to measure where the box sits across the line; gpuix draws it whole.
        const skipped = !expected.red && !expected.blue && Boolean(segs[seg]!.red ?? segs[seg]!.blue)
        if (skipped) continue
        expect(segs[seg], `${name} / ${seg}`).toEqual(expected)
      }
    })
  }
})
