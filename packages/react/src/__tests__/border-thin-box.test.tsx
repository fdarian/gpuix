import fs from "fs"
import type { CSSProperties } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const WHITE: Rgba = [255, 255, 255, 255]

/**
 * Paints `style` on a box at (20, 20) of a white page and reads device pixels. The test
 * window takes the scale of the display it is on, so the widths under test are given in
 * device pixels as well as logical ones: a border of one device pixel is the 1px border of a
 * 1x display on a 2x one.
 */
function paint(name: string, style: CSSProperties) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(
      <div style={{ width: 400, height: 300, backgroundColor: "#ffffff" }}>
        <div style={{ position: "absolute", left: 20, top: 20, ...style }} />
      </div>
    )
    const file = `${SHOTS_DIR}/border-thin-box-${name}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    const pixels = readPixels(file)
    const scale = pixels.width / testRoot.renderer.getWindowSize().width
    return {
      scale,
      /** The pixel at a device offset from the box's top-left corner. */
      at: (x: number, y: number) => pixels.at(20 * scale + x, 20 * scale + y),
    }
  } finally {
    testRoot.unmount()
  }
}

const displayScale = hasNativeTestRenderer ? paint("scale", {}).scale : 1
const widths = [...new Set([1, 2, 1 / displayScale, 3 / displayScale])]

describeNative("a one-sided border on a box with no content on that axis", () => {
  for (const width of widths) {
    const label = `${width}px`
    it(`borderTop ${label} on an empty div paints the whole rule`, () => {
      const shot = paint(`top-${width}`, { width: 100, borderTop: `${width}px solid #ff0000` })
      const rows = width * shot.scale
      for (const x of [0, 1, 50 * shot.scale, 100 * shot.scale - 1]) {
        for (let y = 0; y < rows; y++) expect(shot.at(x, y), `row ${y} column ${x}`).toEqual(RED)
        expect(shot.at(x, rows), "below the rule").toEqual(WHITE)
      }
      expect(shot.at(100 * shot.scale, 0), "right of the rule").toEqual(WHITE)
      expect(shot.at(0, -1), "above the rule").toEqual(WHITE)
    })

    it(`borderBottom ${label} on an empty div paints the whole rule`, () => {
      const shot = paint(`bottom-${width}`, { width: 100, borderBottom: `${width}px solid #ff0000` })
      const rows = width * shot.scale
      for (const x of [0, 1, 50 * shot.scale, 100 * shot.scale - 1]) {
        for (let y = 0; y < rows; y++) expect(shot.at(x, y), `row ${y} column ${x}`).toEqual(RED)
        expect(shot.at(x, rows), "below the rule").toEqual(WHITE)
      }
    })

    it(`borderLeft ${label} on a zero-width box paints the whole column`, () => {
      const shot = paint(`left-${width}`, { height: 100, borderLeft: `${width}px solid #ff0000` })
      const columns = width * shot.scale
      for (const y of [0, 1, 50 * shot.scale, 100 * shot.scale - 1]) {
        for (let x = 0; x < columns; x++) expect(shot.at(x, y), `column ${x} row ${y}`).toEqual(RED)
        expect(shot.at(columns, y), "right of the rule").toEqual(WHITE)
      }
      expect(shot.at(0, 100 * shot.scale), "below the rule").toEqual(WHITE)
    })

    it(`borderRight ${label} on a zero-width box paints the whole column`, () => {
      const shot = paint(`right-${width}`, { height: 100, borderRight: `${width}px solid #ff0000` })
      const columns = width * shot.scale
      for (const y of [0, 1, 50 * shot.scale, 100 * shot.scale - 1]) {
        for (let x = 0; x < columns; x++) expect(shot.at(x, y), `column ${x} row ${y}`).toEqual(RED)
        expect(shot.at(columns, y), "right of the rule").toEqual(WHITE)
      }
    })
  }

  it("uses the colour of the side that owns the pixel", () => {
    const shot = paint("colour", { width: 100, borderTop: "1px solid #0000ff" })
    for (let y = 0; y < shot.scale; y++) expect(shot.at(10, y)).toEqual([0, 0, 255, 255])
  })

  it("a box between its top and bottom border paints both and none of the middle", () => {
    // `height` is the border box here, so 10 leaves 8 logical pixels between the rules.
    const shot = paint("both", { width: 100, height: 10, borderTop: "1px solid #ff0000", borderBottom: "1px solid #ff0000" })
    const rule = shot.scale
    const total = 10 * shot.scale
    for (let y = 0; y < rule; y++) expect(shot.at(10, y)).toEqual(RED)
    for (let y = rule; y < total - rule; y++) expect(shot.at(10, y), `inner row ${y}`).toEqual(WHITE)
    for (let y = total - rule; y < total; y++) expect(shot.at(10, y)).toEqual(RED)
    expect(shot.at(10, total)).toEqual(WHITE)
  })

  it("a box with content paints its borders as before", () => {
    const shot = paint("normal", { width: 100, height: 100, borderTop: "1px solid #ff0000", borderBottom: "4px solid #ff0000" })
    const total = 100 * shot.scale
    expect(shot.at(10, 0)).toEqual(RED)
    expect(shot.at(10, shot.scale - 1)).toEqual(RED)
    expect(shot.at(10, shot.scale)).toEqual(WHITE)
    expect(shot.at(10, total - 4 * shot.scale - 1)).toEqual(WHITE)
    expect(shot.at(10, total - 4 * shot.scale)).toEqual(RED)
    expect(shot.at(10, total - 1)).toEqual(RED)
    expect(shot.at(10, total)).toEqual(WHITE)
  })
})
