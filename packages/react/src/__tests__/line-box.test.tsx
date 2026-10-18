/// The height of a line box as Chrome on macOS makes it, for system fonts at several weights,
/// sizes and line heights, and for a container with a span of another weight or size.
///
/// `fixtures/line-box-oracle.html` is the page and `fixtures/line-box-oracle.json` the boxes
/// headless Chrome reports for it (`getBoundingClientRect` relative to the box of the case,
/// `[x, y, width, height]`; regenerate with the command in the HTML). Chrome rounds the ascent and
/// descent of a font at its size to whole pixels, widens the ascent of Times, Helvetica and Courier,
/// takes `line-height: normal` as the ascent, descent and line gap together, lays a number or a
/// length out in sixty-fourths of a pixel, and shares what a line height has over the font as
/// half-leading that is floored above the text. A span in another font, weight or size is a box of
/// its own on the line, so the line is as tall as the strut and every box together need.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]

const ORACLE: Record<string, Rect> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/line-box-oracle.json"),
    "utf8"
  )
)

const FAMILIES: Record<string, string> = {
  "helvetica-neue": "Helvetica Neue",
  helvetica: "Helvetica",
  "system-ui": ".SystemUIFont",
  menlo: "Menlo",
  times: "Times",
  courier: "Courier",
}

const LINE_HEIGHTS: Record<string, number | string> = {
  normal: "normal",
  "1": 1,
  "1_2": 1.2,
  "24px": "24px",
  "20px": "20px",
}

/** The boxes of an inline element are not on the pixel grid, so they are as exact as Chrome lays
 *  a height out: in sixty-fourths of a pixel. */
const TOLERANCE = 1 / 64 + 1e-3

/** A block is on the device pixel grid, so its height is that of Chrome to the nearest pixel. */
const BLOCK_TOLERANCE = 0.51

const chrome = (id: string): Rect => {
  const box = ORACLE[id]
  expect(box, `no oracle box ${id}`).toBeDefined()
  return box ?? [0, 0, 0, 0]
}

function expectNear(id: string, what: string, actual: number, expected: number, tolerance = TOLERANCE) {
  expect(Math.abs(actual - expected), `${id}: ${what} ${actual} != ${expected}`).toBeLessThanOrEqual(tolerance)
}

/** All the cases of the oracle in one window, one below the other: the height of each is its own. */
function layout(cases: { id: string; style: Record<string, unknown>; spanStyle?: Record<string, unknown> }[]) {
  const root = createTestRoot({ width: 1280, height: 800 })
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  root.render(
    <div style={{ width: 1280 }}>
      {cases.map(({ id, style, spanStyle }) => (
        // Each case starts on a whole pixel, so the box of one is snapped at its far edge only.
        <div key={id} style={{ height: 40, flexShrink: 0 }}>
          <div ref={ref(id)} style={{ width: 1280, color: "#000000", ...style }}>
            {spanStyle ? (
              <>
                H
                <span ref={ref(`${id}.span`)} style={spanStyle}>
                  xg
                </span>
                H
              </>
            ) : (
              <span ref={ref(`${id}.text`)}>Hxg</span>
            )}
          </div>
        </div>
      ))}
    </div>
  )
  root.renderer.dispatchNativeEvents()
  const rect = (name: string): Rect => {
    const box = refs.get(name)?.getBoundingClientRect()
    expect(box, `no element ${name}`).toBeDefined()
    const origin = refs.get(name.replace(/\.(text|span)$/, ""))?.getBoundingClientRect() ?? { x: 0, y: 0 }
    return [(box?.x ?? 0) - origin.x, (box?.y ?? 0) - origin.y, box?.width ?? 0, box?.height ?? 0]
  }
  return { ...root, rect }
}

function singleRunCases() {
  const cases: { id: string; style: Record<string, unknown> }[] = []
  for (const [family, name] of Object.entries(FAMILIES)) {
    for (const weight of [400, 700]) {
      for (const size of [12, 13, 14, 16, 20, 24]) {
        for (const [token, lineHeight] of Object.entries(LINE_HEIGHTS)) {
          cases.push({
            id: `${family}.${weight}.${size}.${token}`,
            style: { fontFamily: name, fontWeight: weight, fontSize: size, lineHeight },
          })
        }
      }
    }
  }
  return cases
}

const MIXED: Record<string, { weight: number; size: number }> = {
  reg16: { weight: 400, size: 16 },
  reg20: { weight: 400, size: 20 },
  bold16: { weight: 700, size: 16 },
  bold20: { weight: 700, size: 20 },
}

function mixedCases() {
  const cases: { id: string; style: Record<string, unknown>; spanStyle: Record<string, unknown> }[] = []
  for (const [family, name] of Object.entries(FAMILIES)) {
    for (const pair of ["reg16-bold16", "reg16-bold20", "reg20-bold20", "bold20-reg20", "bold20-reg16", "bold16-reg20"]) {
      const [outer, inner] = pair.split("-") as [string, string]
      const container = MIXED[outer]
      const span = MIXED[inner]
      if (!container || !span) throw new Error(pair)
      for (const [token, lineHeight] of Object.entries(LINE_HEIGHTS)) {
        cases.push({
          id: `mixed.${family}.${pair}.${token}`,
          style: { fontFamily: name, fontWeight: container.weight, fontSize: container.size, lineHeight },
          spanStyle: { fontWeight: span.weight, fontSize: span.size },
        })
      }
    }
  }
  return cases
}

describeNative("a line box", () => {
  it("is as tall as Chrome makes it for one font, weight, size and line height", () => {
    const cases = singleRunCases()
    const { rect, unmount } = layout(cases)
    try {
      for (const { id } of cases) {
        const expected = chrome(id)
        const actual = rect(id)
        expectNear(id, "height", actual[3], expected[3], BLOCK_TOLERANCE)
        const text = chrome(`${id}.text`)
        const painted = rect(`${id}.text`)
        expectNear(id, "text height", painted[3], text[3])
        expectNear(id, "text y", painted[1], text[1])
      }
    } finally {
      unmount()
    }
  })

  it("is as tall as Chrome makes it for a span of another weight or size", () => {
    const cases = mixedCases()
    const { rect, unmount } = layout(cases)
    try {
      for (const { id } of cases) {
        const expected = chrome(id)
        expectNear(id, "height", rect(id)[3], expected[3], BLOCK_TOLERANCE)
        const span = chrome(`${id}.span`)
        const painted = rect(`${id}.span`)
        expectNear(id, "span height", painted[3], span[3])
        expectNear(id, "span y", painted[1], span[1])
      }
    } finally {
      unmount()
    }
  })

  it("makes a bold 20px Helvetica Neue span in a regular container 25px tall at 24px", () => {
    const { rect, unmount } = layout([
      {
        id: "mixed.helvetica-neue.reg20-bold20.24px",
        style: { fontFamily: "Helvetica Neue", fontWeight: 400, fontSize: 20, lineHeight: "24px" },
        spanStyle: { fontWeight: 700 },
      },
    ])
    try {
      expect(rect("mixed.helvetica-neue.reg20-bold20.24px")[3]).toBe(25)
    } finally {
      unmount()
    }
  })

  it("lays a number or a length line height out in sixty-fourths of a pixel", () => {
    // A block is on the pixel grid, so a stack of 64 lines is what shows the fraction of one:
    // Chrome makes 12px text at 1.2 a line of 14.390625px, and 13px text one of 15.59375px.
    for (const size of [12, 13, 14]) {
      const id = `helvetica-neue.400.${size}.1_2`
      const root = createTestRoot({ width: 1280, height: 800 })
      const refs = new Map<string, PublicInstance>()
      root.render(
        <div
          ref={(instance) => {
            if (instance) refs.set("stack", instance)
          }}
          style={{ width: 1280, fontFamily: "Helvetica Neue", fontSize: size, lineHeight: 1.2, color: "#000000" }}
        >
          {Array.from({ length: 64 }, (_, ix) => (
            <div key={ix}>Hxg</div>
          ))}
        </div>
      )
      root.renderer.dispatchNativeEvents()
      try {
        const height = refs.get("stack")?.getBoundingClientRect().height ?? 0
        expectNear(id, "64 lines", height, 64 * chrome(id)[3], BLOCK_TOLERANCE)
      } finally {
        root.unmount()
      }
    }
  })
})
