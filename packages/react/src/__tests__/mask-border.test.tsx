/// CSS `mask-border` against headless Chrome: `fixtures/mask-border-oracle.html` is the page and
/// `fixtures/mask-border-oracle.json` the alpha of the mask Chrome paints for each case, sampled every
/// fifth pixel of a 200x200 cell. Chrome knows the longhands only as `-webkit-mask-box-image-*`.

import fs from "fs"
import path from "path"
import type { ReactElement } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsDiffer, expectScreenshotsEqual, readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface OracleCase {
  name: string
  style: Record<string, string>
  samples: string
  lines?: Record<string, { low: string; high: string }>
}

const ORACLE: OracleCase[] = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/mask-border-oracle.json"), "utf8")
)

const camel = (property: string) => property.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())

function styleOf(style: Record<string, string>) {
  return Object.fromEntries(Object.entries(style).map(([property, value]) => [camel(property), value]))
}

let counter = 0

type Sampler = (x: number, y: number) => [number, number, number, number]

function capture(tree: ReactElement): { file: string; at: Sampler } {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    testRoot.renderer.dispatchNativeEvents()
    const file = `${SHOTS_DIR}/mask-border-${counter++}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    const pixels = readLogicalPixels(file, testRoot.renderer)
    return { file, at: (x, y) => pixels.at(x, y) }
  } finally {
    testRoot.unmount()
  }
}

/** A white 200x200 cell with a red 100x100 box at (50, 50) and a red child over the whole cell. */
function cell(style: Record<string, unknown>): ReactElement {
  return (
    <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
      <div style={{ position: "absolute", left: 50, top: 50, width: 100, height: 100, backgroundColor: "#ff0000", ...style }}>
        <div style={{ position: "absolute", left: -50, top: -50, width: 200, height: 200, backgroundColor: "#ff0000" }} />
      </div>
    </div>
  )
}

const TOLERANCE = 12

const bytes = (hex: string) => Array.from({ length: hex.length / 2 }, (_, index) => parseInt(hex.slice(index * 2, index * 2 + 2), 16))

/** The alpha of the mask at a pixel: how far the red has covered the white. */
function alphaAt(at: Sampler, x: number, y: number) {
  return 255 - at(x, y)[1]
}

/** The positions where `got` is outside what Chrome painted within `reach` of it, plus the tolerance.
 *  Chrome draws a vector image at the size it is shown at and antialiases the edge of a tile that falls
 *  between pixels, where gpuix scales the pixels it drew at the image's own size and snaps a tile to
 *  whole pixels: an edge differs by a pixel or two, but never by more. */
function outside(got: number[], low: (index: number) => number, high: (index: number) => number, within: (index: number) => number[]) {
  const found: [number, number, number, number][] = []
  got.forEach((value, index) => {
    const near = within(index)
    const floor = Math.min(...near.map(low))
    const ceiling = Math.max(...near.map(high))
    if (value < floor - TOLERANCE || value > ceiling + TOLERANCE) found.push([index, value, floor, ceiling])
  })
  return found
}

function compareGrid(at: Sampler, wanted: number[]) {
  const got: number[] = []
  for (let top = 0; top < 200; top += 5) {
    for (let left = 0; left < 200; left += 5) {
      let total = 0
      for (let y = top; y < top + 5; y++) {
        for (let x = left; x < left + 5; x++) total += alphaAt(at, x, y)
      }
      got.push(Math.round(total / 25))
    }
  }
  const within = (index: number) => {
    const column = index % 40
    const row = Math.floor(index / 40)
    return [-1, 0, 1].flatMap((dy) =>
      [-1, 0, 1]
        .filter((dx) => column + dx >= 0 && column + dx < 40 && row + dy >= 0 && row + dy < 40)
        .map((dx) => index + dy * 40 + dx)
    )
  }
  const found = outside(got, (index) => wanted[index]!, (index) => wanted[index]!, within)
  return { got, found: found.map(([index, value, low, high]) => [index % 40, Math.floor(index / 40), value, low, high]) }
}

function compareLine(name: string, at: Sampler, band: { low: string; high: string }) {
  const [axis, fixed] = [name.split(" ")[0]!, Number(name.split(" ")[1])]
  const low = bytes(band.low)
  const high = bytes(band.high)
  const got = low.map((_, along) => (axis === "row" ? alphaAt(at, along, fixed) : alphaAt(at, fixed, along)))
  const within = (index: number) =>
    [-2, -1, 0, 1, 2].map((delta) => index + delta).filter((other) => other >= 0 && other < low.length)
  return { got, found: outside(got, (index) => low[index]!, (index) => high[index]!, within) }
}

/** Cases where Chrome antialiases the gap between spaced tiles, which gpuix cannot: it rounds each edge of a
 *  tile to a pixel, so a gap of 1.25 pixels is 1 or 2, and the blocks that hold it differ by a fraction. */
const SNAPPED_GAPS = new Map([
  ["spaced fill", 6],
  ["space and stretch, edges of different widths", 6],
])

describeNative("mask-border against Chrome", () => {
  for (const source of ORACLE) {
    it(source.name, () => {
      const { at, file } = capture(cell(styleOf(source.style)))
      const grid = compareGrid(at, bytes(source.samples))
      const lines = Object.entries(source.lines ?? {}).map(([line, band]) => ({ line, ...compareLine(line, at, band) }))
      if (process.env.MASK_BORDER_DUMP) {
        fs.mkdirSync(process.env.MASK_BORDER_DUMP, { recursive: true })
        fs.writeFileSync(`${process.env.MASK_BORDER_DUMP}/${source.name}.json`, JSON.stringify({ grid: grid.got, lines: Object.fromEntries(lines.map((line) => [line.line, line.got])) }))
        fs.copyFileSync(file, `${process.env.MASK_BORDER_DUMP}/${source.name}.png`)
      }
      const allowed = SNAPPED_GAPS.get(source.name) ?? 0
      expect(grid.found.length, `grid blocks outside Chrome's [column, row, gpuix, low, high]: ${JSON.stringify(grid.found.slice(0, 10))}`).toBeLessThanOrEqual(allowed)
      for (const line of lines) {
        expect(line.found.length, `${line.line} outside Chrome's [position, gpuix, low, high]: ${JSON.stringify(line.found.slice(0, 10))}`).toBeLessThanOrEqual(allowed)
      }
    })
  }
})

describeNative("mask-border style props", () => {
  const RING = { maskBorderSource: "linear-gradient(#000, #000)", maskBorderSlice: "10", maskBorderWidth: "10px" }
  const FILL = { ...RING, maskBorderSlice: "10 fill" }

  it("keeps the ring and hides what it leaves out", () => {
    const { at } = capture(cell(RING))
    expect(at(55, 100)[1]).toBeLessThan(10)
    expect(at(100, 55)[1]).toBeLessThan(10)
    expect(at(100, 100)[1]).toBeGreaterThan(245)
    expect(at(20, 20)[1]).toBeGreaterThan(245)
  })

  it("the shorthand is the longhands", () => {
    const longhands = capture(cell({ ...RING, maskBorderRepeat: "round", maskBorderOutset: "5px" })).file
    const shorthand = capture(cell({ maskBorder: "linear-gradient(#000, #000) 10 / 10px / 5px round" })).file
    expectScreenshotsEqual(longhands, shorthand)
  })

  it("the one written last wins, as in CSS", () => {
    const shorthand = "linear-gradient(#000, #000) 10 / 10px"
    const longhandLast = capture(cell({ maskBorder: shorthand, maskBorderWidth: "20px" })).file
    const shorthandLast = capture(cell({ maskBorderWidth: "20px", maskBorder: shorthand })).file
    expectScreenshotsEqual(longhandLast, capture(cell({ ...RING, maskBorderWidth: "20px" })).file)
    expectScreenshotsEqual(shorthandLast, capture(cell(RING)).file)
    expectScreenshotsDiffer(longhandLast, shorthandLast)
  })

  it("ignores an invalid value as if it were not written", () => {
    for (const [key, invalid] of [
      ["maskBorderRepeat", "no-repeat"],
      ["maskBorderMode", "match-source"],
      ["maskBorderOutset", "5%"],
      ["maskBorderSlice", "-1"],
      ["maskBorderWidth", "bogus"],
      ["maskBorderSource", "url(a.png), url(b.png)"],
    ] as const) {
      const rest = Object.fromEntries(Object.entries(RING).filter(([other]) => other !== key))
      expectScreenshotsEqual(capture(cell(rest)).file, capture(cell({ ...rest, [key]: invalid })).file)
    }
    const ring = capture(cell(RING)).file
    expectScreenshotsEqual(ring, capture(cell({ ...RING, maskBorder: "bogus" })).file)
  }, 30_000)

  it("is no border with no source, or with none", () => {
    const plain = capture(cell({})).file
    expectScreenshotsEqual(plain, capture(cell({ maskBorderSlice: "10", maskBorderWidth: "10px" })).file)
    expectScreenshotsEqual(plain, capture(cell({ ...RING, maskBorderSource: "none" })).file)
    expectScreenshotsEqual(plain, capture(cell({ maskBorder: "none" })).file)
  })

  it("adds to the mask image instead of intersecting with it", () => {
    const { at } = capture(cell({ ...FILL, maskImage: "linear-gradient(to right, #000 50%, transparent 50%)" }))
    // The left half is shown by the mask image and the whole box by the border's fill.
    expect(at(60, 100)[1]).toBeLessThan(10)
    expect(at(140, 100)[1]).toBeLessThan(10)
    const ring = capture(cell({ ...RING, maskImage: "linear-gradient(to right, #000 50%, transparent 50%)" })).at
    expect(ring(80, 100)[1]).toBeLessThan(10)
    expect(ring(120, 100)[1]).toBeGreaterThan(245)
  })

  it("makes a stacking context of its own", () => {
    const child = (
      <div style={{ position: "absolute", left: 0, top: 0, width: 100, height: 100, backgroundColor: "#0000ff", zIndex: -1 }} />
    )
    // The wrapper is the context the tree starts with, so the stage is not one: a negative z-index sits
    // behind its background unless the masked box makes a context.
    const stage = (style: Record<string, unknown>) => (
      <div>
        <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
          <div style={{ position: "absolute", left: 50, top: 50, width: 100, height: 100, ...style }}>{child}</div>
        </div>
      </div>
    )
    expect(capture(stage({})).at(100, 100)).toEqual([255, 255, 255, 255])
    expect(capture(stage(FILL)).at(100, 100)).toEqual([0, 0, 255, 255])
  })
})
