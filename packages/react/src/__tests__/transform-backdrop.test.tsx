/// A `backdrop-filter` under a transform is cut to the exact outline of its box, and its blur follows the matrix (#92).
/// gpuix clipped the backdrop to the bounding box of the transformed box, with square corners, and blurred by the
/// deviation of the style whatever the box was scaled to.
///
/// `fixtures/transform-backdrop-oracle.html` is the page and `fixtures/transform-backdrop-oracle.json` what headless Chrome
/// paints from it (regenerate with `transform-backdrop-oracle.sh`): a 100 x 60 box with a 12 px radius at the centre of a
/// 300 x 300 cell (200 x 120 with 24 px and a 8 px blur inside a scale(0.5) parent, which is the same box on screen), under
/// none, rotate(30deg), scale(2), scale(2, 1), skewX(20deg) and rotate(30deg) in a scale(0.5) parent, with
///  - `shape`: `backdrop-filter: invert(1)` over a flat colour, so the inverted pixels are the box's shape, sampled just
///    inside and outside each edge and each corner arc and just inside the corners of the bounding box;
///  - `blur-v` / `blur-h`: `backdrop-filter: blur(4px)` over a hard vertical / horizontal edge through the centre, and
///  - `filter`: `filter: blur(4px)` on a white box over black. `sigma` is the deviation of the blurred step that fits the
///    pixels across the edge best. The oracle also has the untransformed 8 px blurs (`none-blur8`).
///
/// What the oracle says about Chrome: the backdrop is filtered in the element's own space and the result transformed, so
/// the deviation scales with the matrix. Fitted deviations (the fit reads 4.25 for the untransformed 4 px blur and 7.65 for
/// the 8 px one, Skia's approximation of the Gaussian, so a doubled blur is not 8.5): 4.25
/// for a rotation, for the rotation in the scale(0.5) parent (8 px * 0.5) and for the skew, 7.65 for scale(2), and for
/// scale(2, 1) 7.65 across a vertical edge and 4.25 across a horizontal one. A `filter` blurs the same way. Skia keeps
/// the axes apart when the matrix does, and otherwise filters in the layer's scale and shears afterwards, which a
/// separable blur in the window's axes can't do: gpuix scales by the matrix along each axis when it keeps them apart
/// (a scale, or a quarter turn of one) and otherwise by the square root of the area scale, which is what the fits measure
/// for a rotation and the skew (4.25), so the profiles agree to the pixel there too.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const TIMEOUT = 120_000

type Line = { start: [number, number]; axis: "x" | "y"; values: string; sigma: number }
type Cell = {
  id: string
  kind: "shape" | "blur" | "filter"
  points?: Record<string, number>
  lines?: Record<string, Line>
}
const ORACLE: Cell[] = JSON.parse(fs.readFileSync(path.join(FIXTURES, "transform-backdrop-oracle.json"), "utf8"))

const TRANSFORMS: Record<string, { own: string; parent?: string; big?: boolean; blur?: number }> = {
  none: { own: "none" },
  rotate30: { own: "rotate(30deg)" },
  scale2: { own: "scale(2)" },
  scale2x1: { own: "scale(2, 1)" },
  skewx20: { own: "skewX(20deg)" },
  "rotate30-in-scale05": { own: "rotate(30deg)", parent: "scale(0.5)", big: true },
  "none-blur8": { own: "none", blur: 8 },
}
const COLUMNS = 5
const PAGE = { width: 1600, height: 1650 }

/** The cells of the oracle in the order the page lays them out. */
function origin(index: number) {
  return { x: (index % COLUMNS) * 320 + 10, y: Math.floor(index / COLUMNS) * 320 + 10 }
}

function cellElement(cell: Cell, index: number) {
  const at = origin(index)
  const name = cell.id.replace(/^(shape|blur-v|blur-h|filter)-/, "")
  const transform = TRANSFORMS[name]!
  const width = transform.big ? 200 : 100
  const height = transform.big ? 120 : 60
  const radius = transform.big ? 24 : 12
  const blur = transform.blur ?? (transform.big ? 8 : 4)
  const box: Record<string, unknown> = {
    position: "absolute",
    left: 150 - width / 2,
    top: 150 - height / 2,
    width,
    height,
    borderRadius: radius,
    transform: transform.own,
  }
  let backdrop: React.ReactNode
  if (cell.kind === "shape") {
    box.backdropFilter = "invert(1)"
    backdrop = <div style={{ position: "absolute", left: 0, top: 0, width: 300, height: 300, backgroundColor: "#ff8000" }} />
  } else if (cell.kind === "blur") {
    box.backdropFilter = `blur(${blur}px)`
    const vertical = cell.id.startsWith("blur-v")
    backdrop = (
      <>
        <div style={{ position: "absolute", left: 0, top: 0, width: 300, height: 300, backgroundColor: "#000000" }} />
        <div
          style={{
            position: "absolute",
            left: vertical ? 150 : 0,
            top: vertical ? 0 : 150,
            width: vertical ? 150 : 300,
            height: vertical ? 300 : 150,
            backgroundColor: "#ffffff",
          }}
        />
      </>
    )
  } else {
    box.backgroundColor = "#ffffff"
    box.filter = `blur(${blur}px)`
    backdrop = <div style={{ position: "absolute", left: 0, top: 0, width: 300, height: 300, backgroundColor: "#000000" }} />
  }
  const boxElement = <div style={box} />
  return (
    <div key={cell.id} style={{ position: "absolute", left: at.x, top: at.y, width: 300, height: 300, overflow: "hidden" }}>
      {backdrop}
      {transform.parent ? (
        <div
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            width: 300,
            height: 300,
            transformOrigin: "150px 150px",
            transform: transform.parent,
          }}
        >
          {boxElement}
        </div>
      ) : (
        boxElement
      )}
    </div>
  )
}

/** The page of the oracle, painted once. */
let page: Pixels | undefined
function painted() {
  if (page) return page
  const root = createTestRoot({ width: PAGE.width, height: PAGE.height, scaleFactor: 1 })
  try {
    root.render(
      <div style={{ position: "relative", width: PAGE.width, height: PAGE.height, backgroundColor: "#ffffff" }}>
        {ORACLE.map(cellElement)}
      </div>
    )
    root.renderer.flush()
    root.renderer.flush()
    const file = path.join(SHOTS_DIR, `transform-backdrop-${process.pid}.png`)
    fs.rmSync(file, { force: true })
    root.renderer.captureScreenshot(file)
    page = readPixels(file)
  } finally {
    root.unmount()
  }
  expect([page.width, page.height]).toEqual([PAGE.width, PAGE.height])
  return page
}

function red(index: number, x: number, y: number) {
  const at = origin(index)
  return painted().at(at.x + x, at.y + y)[0]
}

/** The deviation of the Gaussian-blurred step at `edge` that fits the line best, as the oracle fits Chrome's. */
function fitSigma(values: number[], edge: number) {
  const low = values[0]!
  const high = values[values.length - 1]!
  const target = values.map((value) => (value - low) / (high - low))
  let best = 0
  let bestError = Infinity
  for (let step = 5; step < 400; step++) {
    const sigma = step / 20
    let error = 0
    target.forEach((value, index) => {
      const model = 0.5 * (1 + erf((index + 0.5 - edge) / (sigma * Math.SQRT2)))
      error += (model - value) ** 2
    })
    if (error < bestError) {
      best = sigma
      bestError = error
    }
  }
  return best
}

function erf(x: number) {
  const sign = Math.sign(x)
  const t = 1 / (1 + 0.3275911 * Math.abs(x))
  const poly = ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t
  return sign * (1 - poly * Math.exp(-x * x))
}

function profile(index: number, oracle: Line) {
  return Array.from({ length: oracle.values.length / 2 }, (_, step) =>
    red(index, oracle.start[0] + (oracle.axis === "x" ? step : 0), oracle.start[1] + (oracle.axis === "y" ? step : 0))
  )
}

function chromeProfile(oracle: Line) {
  return Array.from({ length: oracle.values.length / 2 }, (_, step) => parseInt(oracle.values.slice(step * 2, step * 2 + 2), 16))
}

/** What gpuix and Chrome paint across the edge of a cell. */
function across(id: string, name: string) {
  const index = ORACLE.findIndex((cell) => cell.id === id)
  const oracle = ORACLE[index]?.lines?.[name]
  if (!oracle) throw new Error(`no line ${name} in ${id}`)
  const gpuix = profile(index, oracle)
  const chrome = chromeProfile(oracle)
  return {
    gpuix: fitSigma(gpuix, 24),
    chrome: oracle.sigma,
    worst: Math.max(...gpuix.map((value, step) => Math.abs(value - chrome[step]!))),
  }
}

const SIGMA_CASES = [
  ...["v", "h"].flatMap((edge) => Object.keys(TRANSFORMS).filter((name) => name !== "none-blur8").map((name) => ({ id: `blur-${edge}-${name}`, base: `blur-${edge}-none`, line: "across" }))),
  ...["left edge", "top edge"].flatMap((line) => ["scale2", "scale2x1"].map((name) => ({ id: `filter-${name}`, base: "filter-none", line }))),
]

describeNative("transform: backdrop-filter and filter", () => {
  it(
    "cuts a backdrop to the rotated, rounded outline of its box as Chrome does",
    () => {
      const failures: string[] = []
      for (const [index, cell] of ORACLE.entries()) {
        if (cell.kind !== "shape") continue
        for (const [name, [x, y, expected]] of Object.entries(cell.points ?? {})) {
          const actual = 1 - red(index, Math.floor(x), Math.floor(y)) / 255
          if (Math.abs(actual - expected) >= 0.2) failures.push(`${cell.id} ${name}: gpuix ${actual.toFixed(2)}, Chrome ${expected}`)
        }
      }
      expect(failures).toEqual([])
    },
    TIMEOUT
  )

  it(
    "blurs by the deviation Chrome blurs by under the same matrix",
    () => {
      // The fit reads Chrome's untransformed 4 px blur as 4.25 and its 8 px blur as 7.65, where gpuix's Gaussian reads 4 and
      // 8: Skia approximates the Gaussian. So how far gpuix is from Chrome without a transform, at the two deviations
      // the matrices lead to, is the room a transformed blur gets.
      const untransformed = [across("blur-v-none", "across"), across("blur-v-none-blur8", "across")]
      const room = Math.max(...untransformed.map((blur) => Math.abs(blur.gpuix - blur.chrome))) + 0.15
      const failures: string[] = []
      for (const sample of SIGMA_CASES) {
        const measured = across(sample.id, sample.line)
        if (Math.abs(measured.gpuix - measured.chrome) > room) {
          failures.push(`${sample.id} ${sample.line}: gpuix ${measured.gpuix}, Chrome ${measured.chrome}`)
        }
      }
      expect(failures).toEqual([])
      // The deviations are the ones the rule gives: twice as much across a scaled edge as across a rotated one.
      const ratio = across("blur-v-scale2", "across").gpuix / across("blur-v-rotate30", "across").gpuix
      expect(ratio).toBeGreaterThan(1.9)
      expect(ratio).toBeLessThan(2.2)
      expect(across("blur-h-scale2x1", "across").gpuix).toBeCloseTo(across("blur-h-none", "across").gpuix, 1)
    },
    TIMEOUT
  )

  it(
    "paints the blur profile of Chrome across the edge, to the pixel, under every matrix",
    () => {
      const baseline = across("blur-v-none", "across").worst
      const failures: string[] = []
      for (const sample of SIGMA_CASES) {
        const measured = across(sample.id, sample.line)
        if (measured.worst > baseline + 16) failures.push(`${sample.id} ${sample.line}: ${measured.worst} against ${baseline} untransformed`)
      }
      expect(failures).toEqual([])
    },
    TIMEOUT
  )
})
