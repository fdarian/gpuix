import fs from "fs"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const BLACK: Rgba = [0, 0, 0, 255]
const WHITE: Rgba = [255, 255, 255, 255]

const svg = (body: string, extra = 'viewBox="0 0 20 20"') =>
  `<svg xmlns="http://www.w3.org/2000/svg" ${extra}>${body}</svg>`

/** Left half `currentColor`, right half a fixed green. */
const TWO_TONE = svg(
  '<rect width="10" height="20" fill="currentColor"/><rect x="10" width="10" height="20" fill="#00ff00"/>'
)

let counter = 0

function paint(tree: ReactElement) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    const file = `${SHOTS_DIR}/svg-paint-${counter++}.png`
    fs.rmSync(file, { force: true })
    testRoot.renderer.captureScreenshot(file)
    return readLogicalPixels(file, testRoot.renderer)
  } finally {
    testRoot.unmount()
  }
}

function near(actual: Rgba, expected: Rgba, tolerance = 4) {
  for (let i = 0; i < 4; i++) {
    expect(Math.abs(actual[i]! - expected[i]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(tolerance)
  }
}

describeNative("svg paints its own colours", () => {
  it("keeps every fill and resolves currentColor to the element's color", () => {
    const at = paint(<svg source={TWO_TONE} style={{ width: 40, height: 40, color: "#ff0000" }} />).at
    near(at(10, 20), RED)
    near(at(30, 20), GREEN)
  })

  it("inherits color from an ancestor, like currentColor in the DOM", () => {
    const at = paint(
      <div style={{ color: "#0000ff" }}>
        <div>
          <svg source={TWO_TONE} style={{ width: 40, height: 40 }} />
        </div>
      </div>
    ).at
    near(at(10, 20), BLUE)
    near(at(30, 20), GREEN)
  })

  it("defaults to black, and white under a dark colour scheme", () => {
    near(paint(<svg source={TWO_TONE} style={{ width: 40, height: 40 }} />).at(10, 20), BLACK)
    near(paint(<svg source={TWO_TONE} style={{ width: 40, height: 40, colorScheme: "dark" }} />).at(10, 20), WHITE)
  })

  it("resolves colour functions in color", () => {
    const at = paint(
      <svg source={TWO_TONE} style={{ width: 40, height: 40, color: "color-mix(in srgb, red, red)" }} />
    ).at
    near(at(10, 20), RED)
  })

  it("paints an element with no fill black, as the DOM does", () => {
    near(paint(<svg source={svg('<rect width="20" height="20"/>')} style={{ width: 40, height: 40, color: "#ff0000" }} />).at(20, 20), BLACK)
  })

  it("lets the document's own color win over the inherited one", () => {
    const own = svg('<rect width="20" height="20" fill="currentColor"/>', 'viewBox="0 0 20 20" color="#0000ff"')
    near(paint(<svg source={own} style={{ width: 40, height: 40, color: "#ff0000" }} />).at(20, 20), BLUE)
  })

  it("renders strokes, gradients and opacity, which a mask would flatten", () => {
    const rich = svg(
      '<defs><linearGradient id="g"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs>' +
        '<rect width="20" height="20" fill="url(#g)"/>'
    )
    const at = paint(<svg source={rich} style={{ width: 100, height: 100 }} />).at
    near(at(3, 50), [250, 0, 5, 255], 12)
    near(at(96, 50), [5, 0, 250, 255], 12)
  })

  it("scales to the box at device resolution", () => {
    const at = paint(<svg source={TWO_TONE} style={{ width: 100, height: 100, color: "#ff0000" }} />).at
    near(at(48, 50), RED)
    near(at(52, 50), GREEN)
    // The edge between the halves is sharp, not a blurred upscale.
    near(at(46, 50), RED)
    near(at(54, 50), GREEN)
  })

  it("keeps the aspect ratio and centres the drawing in a taller box", () => {
    const wide = svg('<rect width="20" height="10" fill="#ff0000"/>', 'viewBox="0 0 20 10"')
    const at = paint(<svg source={wide} style={{ width: 100, height: 100 }} />).at
    near(at(50, 50), RED)
    // 100x50 centred in 100x100: nothing above y=25 or below y=75.
    const clear = paint(<div style={{ width: 100, height: 100 }} />).at(50, 10)
    expect(at(50, 10)).toEqual(clear)
    expect(at(50, 90)).toEqual(clear)
    near(at(50, 30), RED)
  })

  it("draws inside the content box, leaving the padding clear", () => {
    const full = svg('<rect width="20" height="20" fill="#ff0000"/>')
    const clear = paint(<div style={{ width: 100, height: 100 }} />).at(5, 5)
    const at = paint(
      <svg source={full} style={{ width: 100, height: 100, padding: 20, boxSizing: "border-box" }} />
    ).at
    near(at(50, 50), RED)
    near(at(25, 25), RED)
    near(at(75, 75), RED)
    expect(at(10, 10)).toEqual(clear)
    expect(at(90, 90)).toEqual(clear)
    expect(at(50, 10)).toEqual(clear)
  })

  it("loads a file src", () => {
    const file = `${SHOTS_DIR}/svg-paint-src.svg`
    fs.writeFileSync(file, TWO_TONE)
    near(paint(<svg src={file} style={{ width: 40, height: 40, color: "#ff0000" }} />).at(10, 20), RED)
  })

  // One screenshot per step, 12 in all: under 1 s on an idle machine, but a screenshot is a GPU
  // readback and the default 5 s is not enough on a busy one.
  it("repaints in place when the source changes, several times over", () => {
    const testRoot = createTestRoot()
    try {
      const colors: Rgba[] = []
      for (let step = 0; step < 12; step++) {
        const channel = step * 20
        testRoot.render(
          <svg
            source={svg(`<rect width="20" height="20" fill="rgb(${channel},0,0)"/>`)}
            style={{ width: 40, height: 40 }}
          />
        )
        const file = `${SHOTS_DIR}/svg-paint-anim.png`
        testRoot.renderer.captureScreenshot(file)
        colors.push(readLogicalPixels(file, testRoot.renderer).at(20, 20))
      }
      expect(colors[0]![0]).toBe(0)
      expect(colors[5]![0]).toBeGreaterThan(40)
      expect(colors[11]![0]).toBeGreaterThan(colors[5]![0])
    } finally {
      testRoot.unmount()
    }
  }, 30_000)

  it("paints nothing for markup that is not an svg, without throwing", () => {
    const at = paint(<svg source="not svg" style={{ width: 40, height: 40 }} />).at
    expect(at(20, 20)).toEqual(paint(<div style={{ width: 40, height: 40 }} />).at(20, 20))
  })

  it("clips the drawing to the border radius, as an svg's default overflow does", () => {
    const full = svg('<rect width="20" height="20" fill="#ff0000"/>')
    const clear = paint(<div style={{ width: 100, height: 100 }} />).at(1, 1)
    const at = paint(<svg source={full} style={{ width: 100, height: 100, borderRadius: 40 }} />).at
    expect(at(1, 1)).toEqual(clear)
    expect(at(98, 98)).toEqual(clear)
    near(at(50, 50), RED)
    near(at(50, 2), RED)
  })

  it("lets the drawing spill past the radius under overflow: visible", () => {
    const full = svg('<rect width="20" height="20" fill="#ff0000"/>')
    const at = paint(
      <svg source={full} style={{ width: 100, height: 100, borderRadius: 40, overflow: "visible" }} />
    ).at
    near(at(1, 1), RED)
  })

  it("still takes clicks and styles", () => {
    const testRoot = createTestRoot()
    try {
      let clicks = 0
      testRoot.render(
        <svg
          source={TWO_TONE}
          onClick={() => clicks++}
          style={{ width: 40, height: 40, backgroundColor: "#ffffff", borderRadius: 8 }}
        />
      )
      testRoot.renderer.nativeSimulateClick(20, 20)
      expect(clicks).toBe(1)
    } finally {
      testRoot.unmount()
    }
  })
})
