/// The pixel row a glyph is painted on where its baseline lands on a tie (#84). Skia rounds a glyph's position half up
/// on both axes (`floor(position + 1 / (2 * subpixel steps))`), so a text line whose box sits at y.5 of a device pixel is
/// painted a row lower than the row below its half, and gpuix rounded it half down: one row above Chrome, only at the
/// tie (a label at 20.5 at scale 1, at 20.25 or 20.75 at scale 2).
///
/// `fixtures/glyph-origin-oracle.html` is the page (the label and Sources row of a ThinkingSteps card: a label, then a
/// flex-wrap row with a block wrapper holding a 20px `inline-flex` chip with cap-trimmed 11px/14px text, in four bands of
/// label sizes, 13/20, 11/14, 16/24 and 24/36, with the top of the column of step k at the band's top + 20 + k/16 for
/// k = 0..16, so a baseline is on every 1/16 of a pixel from 20 to 21), and `fixtures/glyph-origin-oracle.json` what
/// headless Chrome lays out and paints from it at device scale 1 and 2 (regenerate with `glyph-origin-oracle.sh`): the
/// boxes, and the first and last device pixel rows that hold ink of the label and of the chip text. A row holds ink when
/// a pixel of it is more than 64 off the background in some channel, over the label's box (3 CSS px more above and
/// below) and over the chip's inner rows, 8 CSS px in from its sides; the test reads gpuix's screenshot over the
/// same device pixel boxes. The font is `fixtures/inter-subset.ttf`. The threshold is 64 because the faint overshoot row under a 13px
/// "Hamb" is 40 in Chrome and 41 in gpuix at scale 1, which a threshold of 40 reads as a row of ink in one and not the other;
/// every threshold from 50 to 160 gives the same rows in both.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const FONT = path.join(FIXTURES, "inter-subset.ttf")

type Rect = [number, number, number, number]

interface Case {
  id: string
  config: { size: number; line: number }
  k: number
  top: number
  label: Rect
  wrap: Rect
  chip: Rect
  chiptext: Rect
  labelRows: [number, number]
  chipTextRows: [number, number]
}

const ORACLE: Record<"x1" | "x2", { scale: number; cases: Case[] }> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "glyph-origin-oracle.json"), "utf8")
)

const PAGE = { width: 1920, height: 400 }
const PAGE_BG = [0x17, 0x17, 0x17]
const CHIP_BG = [0x52, 0x52, 0x52]
const THRESHOLD = 64
const LEFT = 10
const PITCH = 112
const UNIT = 1 / 64

function roundHalfUp(value: number) {
  return Math.floor(value + 0.5)
}

interface Region {
  x0: number
  x1: number
  y0: number
  y1: number
}

/** The first and last row of `region` that has a pixel more than the threshold off `background`. */
function inkRows(pixels: Pixels, region: Region, background: number[]): [number, number] | null {
  let first: number | null = null
  let last: number | null = null
  for (let y = region.y0; y < region.y1; y++) {
    for (let x = region.x0; x < region.x1; x++) {
      const pixel = pixels.at(x, y)
      if ([0, 1, 2].some((channel) => Math.abs(pixel[channel]! - background[channel]!) > THRESHOLD)) {
        first ??= y
        last = y
        break
      }
    }
  }
  return first === null ? null : [first, last!]
}

function labelRegion(box: Rect, scale: number): Region {
  return {
    x0: roundHalfUp(box[0] * scale),
    x1: roundHalfUp((box[0] + box[2]) * scale),
    y0: roundHalfUp((box[1] - 3) * scale),
    y1: roundHalfUp((box[1] + box[3] + 3) * scale),
  }
}

function chipRegion(box: Rect, scale: number): Region {
  return {
    x0: roundHalfUp((box[0] + 8) * scale),
    x1: roundHalfUp((box[0] + box[2] - 8) * scale),
    y0: roundHalfUp(box[1] * scale) + 1,
    y1: roundHalfUp((box[1] + box[3]) * scale) - 1,
  }
}

function column(testCase: Case, ref: (key: string) => (instance: PublicInstance | null) => void) {
  const key = (name: string) => ref(`${name}-${testCase.id}`)
  return (
    <div
      key={testCase.id}
      style={{
        position: "absolute",
        left: LEFT + PITCH * testCase.k,
        top: testCase.top,
        width: 100,
        display: "flex",
        flexDirection: "column",
        gap: 4,
      }}
    >
      <div
        ref={key("label")}
        style={{ whiteSpace: "nowrap", fontSize: testCase.config.size, lineHeight: `${testCase.config.line}px` }}
      >
        Hamb
      </div>
      <div style={{ display: "flex", flexDirection: "row", flexWrap: "wrap", gap: 6, marginTop: 4 }}>
        <div ref={key("wrap")}>
          <div
            ref={key("chip")}
            style={{
              display: "inline-flex",
              flexDirection: "row",
              alignItems: "center",
              height: 20,
              paddingLeft: 8,
              paddingRight: 8,
              gap: 4,
              backgroundColor: "#525252",
              borderRadius: 6,
              whiteSpace: "nowrap",
              fontSize: 11,
              lineHeight: "14px",
            }}
          >
            <span ref={key("chiptext")} style={{ display: "block", textBox: "trim-both cap alphabetic" }}>
              Hamb
            </span>
          </div>
        </div>
      </div>
    </div>
  )
}

function paint(scale: 1 | 2) {
  const root = createTestRoot({ width: PAGE.width, height: PAGE.height, scaleFactor: scale, fonts: [FONT] })
  try {
    const refs = new Map<string, PublicInstance>()
    const ref = (key: string) => (instance: PublicInstance | null) => {
      if (instance) refs.set(key, instance)
      else refs.delete(key)
    }
    root.render(
      <div
        style={{
          position: "relative",
          width: PAGE.width,
          height: PAGE.height,
          backgroundColor: "#171717",
          color: "#fafafa",
          fontFamily: "Inter Variable",
        }}
      >
        {ORACLE[`x${scale}`].cases.map((testCase) => column(testCase, ref))}
      </div>
    )
    root.renderer.dispatchNativeEvents()
    expect(root.renderer.getA11yTree().frame?.scale_factor, "the window has the scale asked for").toBe(scale)
    const boxes = new Map<string, Rect>()
    for (const [key, instance] of refs) {
      const box = instance.getBoundingClientRect()
      boxes.set(key, [box.x, box.y, box.width, box.height])
    }
    const file = path.join(SHOTS_DIR, `glyph-origin-x${scale}.png`)
    root.renderer.captureScreenshot(file)
    const pixels = readPixels(file)
    expect([pixels.width, pixels.height]).toEqual([PAGE.width * scale, PAGE.height * scale])
    return { boxes, pixels }
  } finally {
    root.unmount()
  }
}

describeNative("the row a glyph is painted on, against Chrome", () => {
  for (const scale of [1, 2] as const) {
    it(`a label and a chip text are laid out as Chrome lays them out, at scale ${scale}`, () => {
      const { boxes } = paint(scale)
      const off: string[] = []
      for (const testCase of ORACLE[`x${scale}`].cases) {
        for (const name of ["label", "wrap", "chip", "chiptext"] as const) {
          const own = boxes.get(`${name}-${testCase.id}`)!
          const chrome = testCase[name]
          if (own.some((value, at) => Math.abs(value - chrome[at]!) > UNIT)) {
            off.push(`${name}-${testCase.id}: ${own.map((value) => value.toFixed(4))} against Chrome's ${chrome.map((value) => value.toFixed(4))}`)
          }
        }
      }
      expect(off).toEqual([])
    })

    it(`the first and last rows of ink of a label and of a chip text are Chrome's, at scale ${scale}`, () => {
      const { pixels } = paint(scale)
      const cases = ORACLE[`x${scale}`].cases
      expect(cases).toHaveLength(68)
      const off: string[] = []
      for (const testCase of cases) {
        const label = inkRows(pixels, labelRegion(testCase.label, scale), PAGE_BG)
        const chipText = inkRows(pixels, chipRegion(testCase.chip, scale), CHIP_BG)
        if (label?.join() !== testCase.labelRows.join()) {
          off.push(`label ${testCase.id} (top ${testCase.top}): rows ${label} against Chrome's ${testCase.labelRows}`)
        }
        if (chipText?.join() !== testCase.chipTextRows.join()) {
          off.push(`chip text ${testCase.id} (top ${testCase.top}): rows ${chipText} against Chrome's ${testCase.chipTextRows}`)
        }
      }
      expect(off).toEqual([])
    })
  }
})
