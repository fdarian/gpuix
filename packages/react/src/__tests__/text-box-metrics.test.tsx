/// A label trimmed to the x-height or the cap height is as tall as Chrome makes it in a font whose heights CoreText
/// does not report: Inter Variable has an x-height of 1118 of 2048 units in its OS/2 table and CoreText says 1056.
/// Skia's Mac scaler context takes both heights from the OS/2 table of the default instance, so they do not follow a
/// `wght` or an `opsz` either (#85).
///
/// `fixtures/x-height-oracle.html` is the page (a block `Hxg` trimmed by each edge, at 12/18, 13/20, 16/24 and 20/28, in
/// five instances of the font: the default, `font-weight: 700`, and three `font-variation-settings`) and
/// `fixtures/x-height-oracle.json` and `x-height-oracle-x2.json` the boxes headless Chrome reports for it at device
/// scale 1 and 2 (`[x, y, width, height]` relative to the line; regenerate with `fixtures/dsf-oracle.sh x-height`).
/// The font is `fixtures/inter-subset.ttf`. Heights are compared to 1/64 px.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const FONT = path.join(FIXTURES, "inter-subset.ttf")

type Rect = [number, number, number, number]

const SCALES = [1, 2] as const

const ORACLE: Record<(typeof SCALES)[number], Record<string, Rect>> = {
  1: JSON.parse(fs.readFileSync(path.join(FIXTURES, "x-height-oracle.json"), "utf8")),
  2: JSON.parse(fs.readFileSync(path.join(FIXTURES, "x-height-oracle-x2.json"), "utf8")),
}

const SIZES = [
  [12, 18],
  [13, 20],
  [16, 24],
  [20, 28],
] as const

const INSTANCES: Record<string, Record<string, number | string>> = {
  normal: {},
  bold: { fontWeight: 700 },
  wght550: { fontVariationSettings: "'wght' 550, 'opsz' 18" },
  opsz32: { fontVariationSettings: "'opsz' 32" },
  heavy: { fontVariationSettings: "'wght' 900, 'opsz' 32" },
}

const TRIMS = ["trim-both", "trim-start", "trim-end"] as const
const EDGES = ["ex alphabetic", "cap alphabetic", "ex text", "text"] as const

const UNIT = 1 / 64

function caseId(size: number, line: number, instance: string, trim: string, edge: string) {
  return `${size}-${line}.${instance}.${trim}.${edge.replace(" ", "-")}`
}

describeNative("text-box metrics from the OS/2 table", () => {
  for (const scaleFactor of SCALES) {
    it(`trims to the x-height and cap height of Chrome in every instance of the font at scale ${scaleFactor}`, () => {
      const root = createTestRoot({ width: 600, height: 400, scaleFactor, fonts: [FONT] })
      const refs = new Map<string, PublicInstance>()
      const cases: string[] = []
      const rows: React.ReactNode[] = []
      for (const [size, line] of SIZES) {
        for (const [instance, style] of Object.entries(INSTANCES)) {
          for (const trim of TRIMS) {
            for (const edge of EDGES) {
              const id = caseId(size, line, instance, trim, edge)
              cases.push(id)
              rows.push(
                <div key={id} style={{ width: 300 }}>
                  <text
                    ref={(element: PublicInstance | null) => {
                      if (element) refs.set(id, element)
                    }}
                    style={{
                      fontFamily: "Inter Variable",
                      fontSize: size,
                      lineHeight: `${line}px`,
                      color: "#000000",
                      ...style,
                      textBoxTrim: trim,
                      textBoxEdge: edge,
                    }}
                  >
                    Hxg
                  </text>
                </div>
              )
            }
          }
        }
      }
      try {
        root.render(<div>{rows}</div>)
        for (const id of cases) {
          const expected = ORACLE[scaleFactor][`${id}.label`]
          expect(expected, `no oracle box ${id}`).toBeDefined()
          const box = refs.get(id)!.getBoundingClientRect()
          expect(
            Math.abs(box.height - expected![3]),
            `${id} at scale ${scaleFactor}: ${box.height}, Chrome ${expected![3]}`
          ).toBeLessThanOrEqual(UNIT / 2)
        }
      } finally {
        root.unmount()
      }
    })
  }
})
