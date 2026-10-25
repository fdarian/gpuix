/// A row that breaks at a soft hyphen (U+00AD) ends with a hyphen, as in Chrome: a glyph of its own after the text, in
/// the style of the text before the break, that counts in the width of the row (so `text-align: center` and `right` place
/// the row with it) and is no part of the text. A soft hyphen in the middle of a row paints nothing.
///
/// `fixtures/soft-hyphen-oracle.html` is the page and `soft-hyphen-oracle.json` / `-x2.json` the rows headless Chrome
/// lays each text out in, at device scale 1 and 2 (`sh fixtures/dsf-oracle.sh soft-hyphen`). Chrome reports the hyphen as
/// a rect of a Range over the text after the rect of the row, and the rect of a `<span>` around the text holds it. Hyphen
/// ink is looked for in the screenshot at the end of the row.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const FONT = path.join(FIXTURES, "inter-text-subset.ttf")

type Rect = [number, number, number, number]

interface Case {
  box: Rect
  lines: string[]
  rows: Rect[]
  ends: number[]
  span: Rect
  spanRects: Rect[]
}

type Oracle = Record<string, Case | number>

const SCALES = [
  { scaleFactor: 1, file: "soft-hyphen-oracle.json" },
  { scaleFactor: 2, file: "soft-hyphen-oracle-x2.json" },
] as const

const SOFT_HYPHEN = "­"

/** Chrome's numbers are multiples of 1/64 of a device pixel, gpuix's are floats that are cut to the unit on the line. */
const TOLERANCE = 0.07

const TEXTS = [
  `Compu${SOFT_HYPHEN}ter pro${SOFT_HYPHEN}gram${SOFT_HYPHEN}ming is a hard${SOFT_HYPHEN}ly eas${SOFT_HYPHEN}y job`,
  `Extra${SOFT_HYPHEN}ordinary`,
]

interface Params {
  id: string
  family: string
  size: number
  align: "left" | "center" | "right"
  width: number
  text: string
  oracle: Case
}

function casesOf(oracle: Oracle): Params[] {
  const cases: Params[] = []
  for (const [id, entry] of Object.entries(oracle)) {
    if (id.startsWith("mc|")) continue
    const [family, size, align, width, textIx] = id.split("|") as [string, string, Params["align"], string, string]
    cases.push({
      id,
      family,
      size: parseFloat(size),
      align,
      width: Number(width),
      text: TEXTS[Number(textIx)]!,
      oracle: entry as Case,
    })
  }
  return cases
}

const fontOf = (c: Pick<Params, "family" | "size">) => ({
  fontFamily: c.family,
  fontSize: c.size,
  lineHeight: "24px",
  color: "#000000",
})

/** The rects Chrome reports for a Range over the text, as a row's text and, after it, the hyphen it ends with. */
function chromeRows(c: Case) {
  const rows: { text: Rect; hyphen: Rect | null }[] = []
  let next = 0
  for (const line of c.lines) {
    const text = c.rows[next++]!
    const hyphenated = line.endsWith(SOFT_HYPHEN)
    rows.push({ text, hyphen: hyphenated ? c.rows[next++]! : null })
  }
  expect(next, "every rect of the Range belongs to a row").toBe(c.rows.length)
  return rows
}

function page(cases: Params[], scaleFactor: number) {
  const height = cases.reduce((sum, c) => sum + c.oracle.box[3] + 8, 0) + 16
  return createTestRoot({ width: 700, height: Math.ceil(height), scaleFactor, fonts: [FONT] })
}

function stack(cases: Params[], withText: (c: Params) => React.ReactNode) {
  return (
    <div style={{ width: 700, backgroundColor: "#ffffff", display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 8, paddingTop: 0 }}>
      {cases.map((c, i) => (
        <div key={c.id} testId={`box${i}`} style={{ width: c.width, ...fontOf(c) }}>
          {withText(c)}
        </div>
      ))}
    </div>
  )
}

describeNative("a row that breaks at a soft hyphen is laid out with its hyphen as Chrome lays it out", () => {
  for (const scale of SCALES) {
    const oracle: Oracle = JSON.parse(fs.readFileSync(path.join(FIXTURES, scale.file), "utf8"))
    const cases = casesOf(oracle)

    it(`puts the rows, with the hyphen in their width, where Chrome does at scale ${scale.scaleFactor}`, { timeout: 120_000 }, () => {
      const root = page(cases, scale.scaleFactor)
      const mismatches: string[] = []
      try {
        root.render(
          stack(cases, (c) => (
            <div highlight={{ query: c.text }}>
              <text testId={`text${cases.indexOf(c)}`} style={{ textAlign: c.align }}>
                <span testId={`span-${c.id}`}>{c.text}</span>
              </text>
            </div>
          ))
        )
        const matches = root.renderer.getPaintedHighlights()
        cases.forEach((c, i) => {
          const box = root.renderer.getElementBounds(root.renderer.findByTestId(`box${i}`)!.id)!
          const rows = chromeRows(c.oracle)
          // The wash over the text: one rect for each row, without the hyphen.
          const textId = root.renderer.findByTestId(`text${i}`)!.id
          const rects = matches.filter((m) => m.elementId === textId).flatMap((m) => m.rects)
          if (rects.length !== rows.length) {
            mismatches.push(`${c.id}: ${rects.length} rows, Chrome has ${rows.length}`)
            return
          }
          rows.forEach((row, r) => {
            const rect = rects[r]!
            const x = rect.x - box.x
            // A row wider than its box starts at its left edge in Chrome, where gpuix centres or right aligns it.
            const overflows = row.text[2] + (row.hyphen?.[2] ?? 0) > c.width
            if (!overflows && Math.abs(x - row.text[0]) > TOLERANCE) mismatches.push(`${c.id} row ${r}: x ${x}, Chrome ${row.text[0]}`)
            // A row that ends in spaces holds them in its wash where Chrome's rect leaves them out.
            if ((row.hyphen !== null || r === rows.length - 1) && Math.abs(rect.width - row.text[2]) > TOLERANCE) {
              mismatches.push(`${c.id} row ${r}: width ${rect.width}, Chrome ${row.text[2]}`)
            }
          })
          // The box of the span holds the hyphens: its left and right are those of Chrome's. A space that hangs at the
          // end of a row is part of it in gpuix and not in Chrome, and so is a row that overflows.
          const hangs = c.oracle.lines.slice(0, -1).some((line) => line.endsWith(" "))
          if (hangs || rows.some((row) => row.text[2] + (row.hyphen?.[2] ?? 0) > c.width)) return
          const span = root.renderer.getElementBounds(root.renderer.findByTestId(`span-${c.id}`)!.id)
          if (span === null) {
            mismatches.push(`${c.id}: the span has no bounds`)
            return
          }
          const [spanX, , spanWidth] = c.oracle.span
          if (Math.abs(span.x - box.x - spanX) > TOLERANCE || Math.abs(span.width - spanWidth) > TOLERANCE) {
            mismatches.push(`${c.id}: span x ${span.x - box.x} width ${span.width}, Chrome ${spanX} ${spanWidth}`)
          }
        })
      } finally {
        root.unmount()
      }
      expect(mismatches).toEqual([])
    })

    it(`is as wide as Chrome at min-content, which a hyphen counts in, at scale ${scale.scaleFactor}`, () => {
      const root = createTestRoot({ width: 700, height: 300, scaleFactor: scale.scaleFactor, fonts: [FONT] })
      const mismatches: string[] = []
      try {
        for (const [id, expected] of Object.entries(oracle)) {
          if (!id.startsWith("mc|")) continue
          const [, family, size, textIx] = id.split("|") as [string, string, string, string]
          root.render(
            <div style={{ width: 700, display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
              <div testId="box" style={{ width: "min-content", ...fontOf({ family, size: parseFloat(size) }) }}>
                <text>{TEXTS[Number(textIx)]}</text>
              </div>
            </div>
          )
          const width = root.renderer.getElementBounds(root.renderer.findByTestId("box")!.id)!.width
          if (Math.abs(width - (expected as number)) > TOLERANCE) mismatches.push(`${id}: ${width}, Chrome ${expected}`)
        }
      } finally {
        root.unmount()
      }
      expect(mismatches).toEqual([])
    })
  }
})

function isInk(pixels: Pixels, x: number, y: number) {
  const [r, g, b] = pixels.at(x, y)
  return (r + g + b) / 3 < 150
}

/** Whether any pixel of the columns `from` to `to` (logical pixels, fractions cut inwards) and rows `top` to `bottom` is dark. */
function hasInk(pixels: Pixels, from: number, to: number, top: number, bottom: number) {
  for (let y = Math.ceil(top); y < Math.floor(bottom); y++) {
    for (let x = Math.ceil(from); x < Math.floor(to); x++) {
      if (isInk(pixels, x, y)) return true
    }
  }
  return false
}

describeNative("the hyphen of a row that breaks at a soft hyphen is painted", () => {
  for (const scale of SCALES) {
    const oracle: Oracle = JSON.parse(fs.readFileSync(path.join(FIXTURES, scale.file), "utf8"))
    const cases = casesOf(oracle).filter((c) => c.width === 80 || (c.width === 60 && c.text === TEXTS[1]))

    it(`has ink after the text of each hyphenated row and none after the others at scale ${scale.scaleFactor}`, () => {
      const root = page(cases, scale.scaleFactor)
      try {
        root.render(
          stack(cases, (c) => (
            <text style={{ textAlign: c.align }}>{c.text}</text>
          ))
        )
        root.renderer.dispatchNativeEvents()
        const file = path.join(SHOTS_DIR, `soft-hyphen-${scale.scaleFactor}.png`)
        root.renderer.captureScreenshot(file)
        const pixels = readLogicalPixels(file, root.renderer)
        const problems: string[] = []
        cases.forEach((c, i) => {
          const box = root.renderer.getElementBounds(root.renderer.findByTestId(`box${i}`)!.id)!
          chromeRows(c.oracle).forEach((row, r) => {
            const top = box.y + r * 24
            const end = box.x + c.oracle.ends[r]!
            if (row.hyphen !== null) {
              const width = row.hyphen[2]
              if (!hasInk(pixels, end + 1, end + width - 1, top, top + 24)) {
                problems.push(`${c.id} row ${r}: no hyphen ink in ${end + 1}..${end + width - 1}`)
              }
              if (hasInk(pixels, end + width + 1.5, end + width + 6, top, top + 24)) {
                problems.push(`${c.id} row ${r}: ink past the hyphen`)
              }
            } else if (hasInk(pixels, end + 1.5, end + 6, top, top + 24)) {
              problems.push(`${c.id} row ${r}: ink after a row that is not hyphenated`)
            }
          })
        })
        expect(problems).toEqual([])
      } finally {
        root.unmount()
      }
    })
  }

  it("is in the colour of the text before the break, and the underline of that text goes under it", () => {
    const root = createTestRoot({ width: 200, height: 80, fonts: [FONT] })
    try {
      root.render(
        <div style={{ width: 62, backgroundColor: "#ffffff", fontFamily: "Inter Variable", fontSize: 16, lineHeight: "24px" }}>
          <text>
            <span style={{ color: "#ff0000", textDecorationLine: "underline", textDecorationThickness: "2px" }}>
              {`Extra${SOFT_HYPHEN}ordinary`}
            </span>
          </text>
        </div>
      )
      root.renderer.dispatchNativeEvents()
      const file = path.join(SHOTS_DIR, "soft-hyphen-style.png")
      root.renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, root.renderer)
      const isRed = (x: number, y: number) => {
        const [r, g, b] = pixels.at(x, y)
        return r > 200 && g < 90 && b < 90
      }
      // "Extra" is 32 wide in Inter at 16px and the hyphen after it 5.7: red ink is in the band the hyphen is in,
      // and the underline, which is red, runs to the end of the hyphen.
      const redIn = (from: number, to: number, top: number, bottom: number) => {
        for (let y = top; y < bottom; y++) for (let x = from; x < to; x++) if (isRed(x, y)) return true
        return false
      }
      expect(redIn(38, 42, 4, 20), "a red hyphen").toBe(true)
      expect(redIn(40, 43, 19, 23), "the underline under the hyphen").toBe(true)
    } finally {
      root.unmount()
    }
  })

  it("is not painted for a soft hyphen in the middle of a row", () => {
    const shot = (name: string, text: string) => {
      const root = createTestRoot({ width: 200, height: 40, fonts: [FONT] })
      try {
        root.render(
          <div style={{ width: 200, backgroundColor: "#ffffff", fontFamily: "Inter Variable", fontSize: 16, lineHeight: "24px" }}>
            <text>{text}</text>
          </div>
        )
        root.renderer.dispatchNativeEvents()
        const file = path.join(SHOTS_DIR, `soft-hyphen-${name}.png`)
        root.renderer.captureScreenshot(file)
        return fs.readFileSync(file)
      } finally {
        root.unmount()
      }
    }
    expect(shot("middle", `ab${SOFT_HYPHEN}cd efg`).equals(shot("whole", "abcd efg"))).toBe(true)
  })
})
