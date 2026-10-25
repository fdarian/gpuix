/// Text under a transform is as sharp as Chrome's (#92). gpuix rasterized a glyph upright at the window's scale and
/// resampled the bitmap through the matrix, which blurs it under a rotation, a skew or a scale; Chrome (Skia)
/// rasterizes the glyph through the matrix. Now a text in a static transform is rasterized through its matrix and drawn
/// upright in the window, and a text in a transform that animates is rasterized upright at a scale kept across the
/// animation (the next power of the square root of two above the scale of the matrix) and drawn through the matrix,
/// as Chrome keeps the raster scale of an animating layer and rasters it again when the animation ends.
///
/// `fixtures/transform-text-oracle.html` is the page and `fixtures/transform-text-oracle.json` what headless Chrome
/// paints from it (regenerate with `transform-text-oracle.sh`): "Hamburgefon" in Inter Variable (wght 400, opsz 14,
/// `fixtures/inter-subset.ttf`) at 13 and 16 px, once with each of eight transforms about the top left of the text
/// (none, scale(2), scale(0.5), scale(1.5, 1), rotate(15deg), rotate(90deg), rotate(30deg) scale(1.5), skewX(20deg)), as
/// black on white with `-webkit-font-smoothing` `antialiased` and `auto`, and as white on black with `auto`; and the same
/// text untransformed at twice the font size (26 and 32 px), what scale(2) is compared with. Per cell of 260 x 150 CSS px
/// it holds, at device scale 1 and 2:
///  - `ink`: the sum of the coverage, c = |L - L_bg| / |L_fg - L_bg| of the gamma-encoded Rec.709 luma of the screenshot,
///    in CSS px squared;
///  - `box`: the pixels with a coverage over 0.1;
///  - `solid`: sum c^2 / sum c, how much of the ink is at full coverage, and `edge`: the sum of the squared differences of
///    neighbouring pixels over sum c, how steep the edges are. A glyph that is resampled loses both: at scale 2 the
///    13 px text under scale(2) had a `solid` of 0.74 and an `edge` of 0.12 in gpuix, where Chrome's (and that of the
///    26 px text) is 0.92 and 0.32.
///
/// What the oracle says about Chrome:
///  - Text keeps the smoothing it has untransformed under every transform: the ink of `auto` is 11-15 % over that of
///    `antialiased` under rotations, skews and scales as untransformed (13 % at 13 px), and `antialiased` is the plain
///    coverage. CoreGraphics' smoothing is not given up for a rotated glyph, so gpuix keeps it.
///  - Text under scale(2) is the text at twice the size (the same ink, `solid` and `edge` to the pixel), and under
///    rotate(90deg) the text turned: the ink within 0.6 % and the edges within 5 % of the untransformed text's, which is
///    the subpixel positions moving to the vertical axis.
///  - A skew or a rotation draws the glyph at a whole pixel, without the subpixel positions along the baseline; scales and
///    flips and quarter turns keep them (along x, and along y for a quarter turn).
///
/// Where gpuix is compared with Chrome (all 45 cells, both scales): the ink is within 1.5 % at scale 1 (1.1 % at most, for
/// the text under scale(0.5), 6.5 px high) and 0.5 % at scale 2 (0.3 % at most); `solid` is within 1.5 % and 1 %, `edge`
/// within 4 % and 3 % (CoreGraphics and Skia rasterize the same outline a hair apart); the box that holds the ink is
/// within 1 and 0.5 CSS px.
///
/// Under a transform that animates (a CSS animation, a transition, a Motion track or a motion value that moves it; the
/// element and everything in it), at scale 2: a 13 px text under scale(1) to scale(3) at scale(1.8) is rasterized at
/// 2x and drawn at 0.9 (`solid` 0.886 and `edge` 0.239, where exact is 0.931 and 0.325 and the resampled text of
/// scale(2) had 0.77 and 0.11); at scale(2), a bucket, it is the exact text to the pixel; and it is exact to the pixel
/// again once the run has ended. A rotation has a scale of 1, so an animating rotation is still resampled from the
/// upright raster (`solid` 0.779, `edge` 0.398 against exact 0.871 and 0.574), as the layer of a rotating element is.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { defineKeyframes, motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"
import { readPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const FONT = path.join(FIXTURES, "inter-subset.ttf")
const TIMEOUT = 120_000

type Cell = {
  id: string
  polarity: "dark" | "light"
  smoothing: "antialiased" | "auto"
  size: number
  transform: string
  ink: number
  box: [number, number, number, number]
  solid: number
  edge: number
}

type Scale = 1 | 2

const ORACLE: Record<"x1" | "x2", { scale: number; cells: Cell[] }> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "transform-text-oracle.json"), "utf8")
)

function chrome(scale: Scale, id: string) {
  const cell = ORACLE[`x${scale}`].cells.find((candidate) => candidate.id === id)
  if (!cell) throw new Error(`the oracle has no cell ${id}`)
  return cell
}

const PAGE = { width: 1200, height: 2060 }
const CELL = { width: 260, height: 150 }
const COLS = 4
const COLORS = { dark: { fg: "#000000", bg: "#ffffff" }, light: { fg: "#ffffff", bg: "#000000" } } as const
const TEXT = "Hamburgefon"
const BOX_THRESHOLD = 0.1

/** The bounds against Chrome, by scale: relative for the ink, `solid` and `edge`, in CSS px for the box. */
const AGREEMENT = {
  1: { ink: 0.015, solid: 0.015, edge: 0.04, box: 1 },
  2: { ink: 0.005, solid: 0.01, edge: 0.03, box: 0.5 },
} as const

function luma(rgb: ArrayLike<number>) {
  return (0.2126 * rgb[0]! + 0.7152 * rgb[1]! + 0.0722 * rgb[2]!) / 255
}

function hexRgb(hex: string) {
  return [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16))
}

type Measures = Pick<Cell, "ink" | "box" | "solid" | "edge">

/** The measures of `transform-text-oracle.py` for the cell whose top left is (`x`, `y`) in CSS px. */
function measure(pixels: Pixels, scale: number, polarity: Cell["polarity"], x: number, y: number): Measures {
  const lbg = luma(hexRgb(COLORS[polarity].bg))
  const span = Math.abs(luma(hexRgb(COLORS[polarity].fg)) - lbg)
  const width = CELL.width * scale
  const height = CELL.height * scale
  const grid: Float64Array[] = []
  for (let row = 0; row < height; row++) {
    const line = new Float64Array(width)
    for (let column = 0; column < width; column++) {
      line[column] = Math.abs(luma(pixels.at(x * scale + column, y * scale + row)) - lbg) / span
    }
    grid.push(line)
  }
  let total = 0
  let squares = 0
  let edge = 0
  let [x0, y0, x1, y1] = [Infinity, Infinity, -1, -1]
  for (let row = 0; row < height; row++) {
    const line = grid[row]!
    const below = grid[row + 1]
    for (let column = 0; column < width; column++) {
      const c = line[column]!
      if (c <= 0) {
        if (column + 1 < width && line[column + 1]! > 0) edge += line[column + 1]! ** 2
        if (below && below[column]! > 0) edge += below[column]! ** 2
        continue
      }
      total += c
      squares += c * c
      if (c > BOX_THRESHOLD) {
        x0 = Math.min(x0, column)
        x1 = Math.max(x1, column)
        y0 = Math.min(y0, row)
        y1 = Math.max(y1, row)
      }
      if (column + 1 < width) edge += (line[column + 1]! - c) ** 2
      if (below) edge += (below[column]! - c) ** 2
    }
  }
  return {
    ink: total / (scale * scale),
    box: [x0 / scale, y0 / scale, (x1 + 1) / scale, (y1 + 1) / scale],
    solid: squares / total,
    edge: edge / total,
  }
}

function relative(painted: number, reference: number) {
  return Math.abs(painted - reference) / reference
}

function got<T>(value: T | undefined, what: string): T {
  if (value === undefined) throw new Error(`no ${what}`)
  return value
}

/** A cell of the oracle: its background, and the text in it, which `text` styles and `motion` may animate. */
function cellElement(
  cell: Pick<Cell, "polarity" | "smoothing" | "size">,
  at: { x: number; y: number },
  text: React.ReactElement,
  key?: string
) {
  return (
    <div
      key={key}
      style={{
        position: "absolute",
        left: at.x,
        top: at.y,
        width: CELL.width,
        height: CELL.height,
        overflow: "hidden",
        backgroundColor: COLORS[cell.polarity].bg,
        color: COLORS[cell.polarity].fg,
        WebkitFontSmoothing: cell.smoothing,
      }}
    >
      {text}
    </div>
  )
}

function textStyle(size: number, extra: Record<string, unknown>) {
  return {
    position: "absolute",
    left: 30,
    top: 40,
    height: 24,
    lineHeight: "24px",
    whiteSpace: "nowrap",
    fontFamily: "Inter Variable",
    fontSize: size,
    fontVariationSettings: "'wght' 400, 'opsz' 14",
    transformOrigin: "0 0",
    ...extra,
  } as React.CSSProperties
}

function cellOrigin(index: number) {
  return { x: (index % COLS) * 300 + 10, y: Math.floor(index / COLS) * 170 + 10 }
}

function screenshot(root: ReturnType<typeof createTestRoot>, name: string, scale: number, width: number) {
  root.renderer.dispatchNativeEvents()
  const file = path.join(SHOTS_DIR, `transform-text-${name}-x${scale}.png`)
  root.renderer.captureScreenshot(file)
  const pixels = readPixels(file)
  expect(pixels.width).toBe(width * scale)
  return pixels
}

/** What gpuix paints for each cell of the oracle, all in one window at `scale`. */
function paintedCells(scale: Scale) {
  const cells = ORACLE[`x${scale}`].cells
  const root = createTestRoot({ width: PAGE.width, height: PAGE.height, scaleFactor: scale, fonts: [FONT] })
  try {
    root.render(
      <div style={{ position: "relative", width: PAGE.width, height: PAGE.height, backgroundColor: "#808080" }}>
        {cells.map((cell, index) =>
          cellElement(
            cell,
            cellOrigin(index),
            <div style={textStyle(cell.size, { transform: cell.transform })}>{TEXT}</div>,
            cell.id
          )
        )}
      </div>
    )
    const pixels = screenshot(root, "page", scale, PAGE.width)
    const measured = new Map<string, Measures>()
    cells.forEach((cell, index) => {
      const at = cellOrigin(index)
      measured.set(cell.id, measure(pixels, scale, cell.polarity, at.x, at.y))
    })
    return measured
  } finally {
    root.unmount()
  }
}

/** The cells painted at each scale, once. */
const PAINTED = {} as Record<Scale, Map<string, Measures>>
function painted(scale: Scale) {
  return (PAINTED[scale] ??= paintedCells(scale))
}

function expectAgreement(own: Measures, reference: Measures, scale: Scale, what: string) {
  const bounds = AGREEMENT[scale]
  expect(relative(own.ink, reference.ink), `${what}: ink ${own.ink} against ${reference.ink}`).toBeLessThan(bounds.ink)
  expect(relative(own.solid, reference.solid), `${what}: solid ${own.solid} against ${reference.solid}`).toBeLessThan(
    bounds.solid
  )
  expect(relative(own.edge, reference.edge), `${what}: edge ${own.edge} against ${reference.edge}`).toBeLessThan(
    bounds.edge
  )
  own.box.forEach((side, index) => {
    expect(
      Math.abs(side - reference.box[index]!),
      `${what}: box [${own.box}] against [${reference.box}]`
    ).toBeLessThanOrEqual(bounds.box + 1e-6)
  })
}

describeNative("text under a static transform against Chrome", () => {
  for (const scale of [1, 2] as const) {
    it(`has the ink, the sharpness and the box of Chrome's in all 45 cells at scale ${scale}`, () => {
      const cells = ORACLE[`x${scale}`].cells
      expect(cells).toHaveLength(45)
      for (const cell of cells) {
        expectAgreement(got(painted(scale).get(cell.id), cell.id), cell, scale, `${cell.id} at scale ${scale}`)
      }
    }, TIMEOUT)

    it(`keeps the smoothing of auto under every transform, as Chrome does, at scale ${scale}`, () => {
      const cells = ORACLE[`x${scale}`].cells
      const transforms = [...new Set(cells.map((cell) => cell.transform))]
      expect(transforms).toHaveLength(8)
      for (const size of [13, 16]) {
        for (const transform of transforms) {
          const find = (smoothing: Cell["smoothing"]) =>
            got(
              cells.find(
                (cell) =>
                  cell.smoothing === smoothing &&
                  cell.polarity === "dark" &&
                  cell.size === size &&
                  cell.transform === transform
              ),
              `the ${smoothing} cell of ${size}px ${transform}`
            )
          const [auto, plain] = [find("auto"), find("antialiased")]
          const ratio = got(painted(scale).get(auto.id), auto.id).ink / got(painted(scale).get(plain.id), plain.id).ink
          const chromeRatio = auto.ink / plain.ink
          expect(ratio, `${size}px ${transform}: auto over antialiased`).toBeGreaterThan(1.08)
          expect(
            Math.abs(ratio - chromeRatio),
            `${size}px ${transform}: ${ratio} against Chrome's ${chromeRatio}`
          ).toBeLessThan(0.01)
        }
      }
    }, TIMEOUT)

    it(`draws text under scale(2) as the text at twice the size at scale ${scale}`, () => {
      for (const [smoothing, size, doubled] of [
        ["antialiased", 13, 26],
        ["antialiased", 16, 32],
        ["auto", 13, 26],
        ["auto", 16, 32],
      ] as const) {
        const scaled = got(painted(scale).get(`${smoothing}-dark-${size}-scale2`), "the scaled text")
        const plain = got(painted(scale).get(`${smoothing}-dark-${doubled}-ref`), "the text at twice the size")
        const what = `${smoothing} ${size}px under scale(2) against ${doubled}px`
        expect(relative(scaled.ink, plain.ink), `${what}: ink`).toBeLessThan(0.003)
        expect(relative(scaled.solid, plain.solid), `${what}: solid`).toBeLessThan(0.005)
        expect(relative(scaled.edge, plain.edge), `${what}: edge`).toBeLessThan(0.02)
      }
    }, TIMEOUT)
  }

  it("is no heavier or blurrier for a rotation than the text it turns, to what Chrome shows", () => {
    // The text under rotate(15deg) loses 0.1 % of its ink at most to the antialiasing of a slanted edge.
    for (const id of ["antialiased-dark-13", "auto-dark-13", "auto-dark-16"]) {
      const turned = got(painted(2).get(`${id}-rotate15`), id)
      const upright = got(painted(2).get(`${id}-none`), id)
      expect(relative(turned.ink, upright.ink), `${id}: ink`).toBeLessThan(0.01)
      expect(turned.solid, `${id}: solid`).toBeGreaterThan(upright.solid * 0.98)
      expect(turned.edge, `${id}: edge`).toBeGreaterThan(upright.edge * 0.95)
    }
  })
})

const SINGLE = { width: 300, height: 170 }
const AT = { x: 10, y: 10 }
const DARK_13 = { polarity: "dark", smoothing: "auto", size: 13 } as const

/** The measures of the 13px dark text in a window of one cell at scale 2, with the style `extra` on the text, and the
 *  props of the element the text is in: after each of `times` ms of a paused clock, or once when there are none. */
function measuredText(
  name: string,
  props: Record<string, unknown>,
  extra: Record<string, unknown>,
  times: number[] = [],
  Tag: "div" | "motion" = "div"
) {
  const root = createTestRoot({ width: SINGLE.width, height: SINGLE.height, scaleFactor: 2, fonts: [FONT] })
  try {
    root.renderer.clockPause()
    const Element = Tag === "motion" ? motion.div : "div"
    root.render(
      <div style={{ position: "relative", width: SINGLE.width, height: SINGLE.height, backgroundColor: "#808080" }}>
        {cellElement(
          DARK_13,
          AT,
          <Element style={textStyle(13, extra)} {...props}>
            {TEXT}
          </Element>
        )}
      </div>
    )
    root.renderer.flush()
    const at = (label: string) => measure(screenshot(root, `${name}-${label}`, 2, SINGLE.width), 2, "dark", AT.x, AT.y)
    if (times.length === 0) return [at("static")]
    let elapsed = 0
    return times.map((time) => {
      root.renderer.clockFastForward(time - elapsed)
      elapsed = time
      root.renderer.flush()
      return at(`${time}`)
    })
  } finally {
    root.unmount()
  }
}

defineKeyframes("tx-grow", { "0%": { transform: "scale(1)" }, "100%": { transform: "scale(3)" } })
defineKeyframes("tx-turn", { "0%": { transform: "rotate(0deg)" }, "100%": { transform: "rotate(30deg)" } })

describeNative("text under a transform that animates", () => {
  // scale(1) to scale(3) in a second: scale(1.8) at 400 ms, scale(2) at 500 ms, scale(3) from 1000 ms on.
  const grow = { animation: "tx-grow 1s linear forwards" }
  const still = (transform: string) => measuredText(`still-${transform}`, {}, { transform })[0]!

  it("is rasterized at the bucket of the scale and drawn through the matrix: sharper than resampled, no sharper than exact", () => {
    const [at400, at500, ended] = measuredText("grow", {}, grow, [400, 500, 1100])
    const exact18 = still("scale(1.8)")
    const exact2 = still("scale(2)")
    const exact3 = still("scale(3)")
    // Between buckets: the raster is the one of scale 2, drawn at 0.9. It is the ink of the scale it is drawn at.
    expect(relative(got(at400, "400 ms").ink, exact18.ink), "scale(1.8): ink").toBeLessThan(0.01)
    // Resampled from a raster at scale 1, the text under scale(2) had a `solid` of 0.77 and an `edge` of 0.11.
    expect(at400!.solid, "scale(1.8): solid").toBeGreaterThan(0.86)
    expect(at400!.solid, "scale(1.8): solid, no more than exact").toBeLessThan(exact18.solid + 0.005)
    expect(at400!.edge, "scale(1.8): edge").toBeGreaterThan(0.2)
    expect(at400!.edge, "scale(1.8): edge, not as steep as exact").toBeGreaterThan(exact18.edge * 0.6)
    expect(at400!.edge, "scale(1.8): edge, no more than exact").toBeLessThan(exact18.edge * 1.02)
    // On a bucket: the raster is drawn as it is rasterized, so it is the exact one to what the pixel grid allows.
    expect(relative(got(at500, "500 ms").ink, exact2.ink), "scale(2): ink").toBeLessThan(0.002)
    expect(relative(at500!.solid, exact2.solid), "scale(2): solid").toBeLessThan(0.005)
    expect(relative(at500!.edge, exact2.edge), "scale(2): edge").toBeLessThan(0.01)
    // The run has ended: the text is rasterized through scale(3) itself.
    expect(relative(got(ended, "1100 ms").ink, exact3.ink), "ended: ink").toBeLessThan(0.002)
    expect(relative(ended!.solid, exact3.solid), "ended: solid").toBeLessThan(0.002)
    expect(relative(ended!.edge, exact3.edge), "ended: edge").toBeLessThan(0.01)
  }, TIMEOUT)

  it("draws a rotation that animates resampled, which a raster scale of 1 leaves, and exact when it has ended", () => {
    const [middle, ended] = measuredText("turn", {}, { animation: "tx-turn 1s linear forwards" }, [500, 1100])
    const exact15 = still("rotate(15deg)")
    const exact30 = still("rotate(30deg)")
    expect(relative(got(middle, "500 ms").ink, exact15.ink), "rotate(15deg): ink").toBeLessThan(0.01)
    expect(middle!.solid, "resampled").toBeLessThan(exact15.solid * 0.97)
    expect(middle!.edge, "resampled").toBeLessThan(exact15.edge * 0.9)
    expect(relative(got(ended, "1100 ms").solid, exact30.solid), "ended: solid").toBeLessThan(0.002)
    expect(relative(ended!.edge, exact30.edge), "ended: edge").toBeLessThan(0.01)
  }, TIMEOUT)

  it("is rasterized at a scale kept across a Motion track as well", () => {
    const track = { initial: { scale: 1 }, animate: { scale: 3 }, transition: { duration: 1, ease: "linear" } }
    const [at500, ended] = measuredText("motion", track, {}, [500, 1500], "motion")
    const exact2 = still("scale(2)")
    const exact3 = still("scale(3)")
    expect(relative(got(at500, "500 ms").ink, exact2.ink), "scale(2): ink").toBeLessThan(0.01)
    expect(relative(at500!.solid, exact2.solid), "scale(2): solid").toBeLessThan(0.005)
    expect(relative(got(ended, "1500 ms").solid, exact3.solid), "ended: solid").toBeLessThan(0.002)
    expect(relative(ended!.edge, exact3.edge), "ended: edge").toBeLessThan(0.01)
  }, TIMEOUT)

  it("rasterizes the glyphs of an animation once per bucket and still patches every tick", async () => {
    const root = createTestRoot({ width: 760, height: 620, scaleFactor: 2, fonts: [FONT] })
    const renderer = root.renderer
    try {
      renderer.clockPause()
      root.render(<TickedPage />)
      for (let frame = 0; frame < 12; frame++) {
        renderer.flush()
        renderer.dispatchNativeEvents()
        await new Promise((resolve) => setTimeout(resolve, 8))
      }
      renderer.setInlineDraws(false)
      renderer.setA11yActive(false)
      const tick = () => {
        renderer.clockAdvance(33)
        renderer.drawFrame()
      }
      tick()
      const rasterizedAtStart = renderer.getFrameWorkCounters().rasterizedGlyphs
      const patches = renderer.getPatchCounters()
      // The animation runs for a second: scale(1) to scale(3), 30 ticks, then it ends.
      for (let index = 0; index < 29; index++) tick()
      const after = renderer.getPatchCounters()
      const rasterizedInRun = renderer.getFrameWorkCounters().rasterizedGlyphs - rasterizedAtStart
      tick()
      tick()
      const rasterizedWithEnd = renderer.getFrameWorkCounters().rasterizedGlyphs - rasterizedAtStart
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)

      // Rasters at 1.41, 2 and 2.83 (and 4 for the last stretch): the 11 glyphs of the text each, not each tick.
      expect(rasterizedInRun, "a raster for the buckets the scale passes through").toBeGreaterThanOrEqual(3 * 11)
      expect(rasterizedInRun, "and not for the ticks").toBeLessThanOrEqual(4 * 11)
      expect(rasterizedWithEnd - rasterizedInRun, "the end of the run rasters the text through its matrix").toBeGreaterThan(0)
      expect(rasterizedWithEnd - rasterizedInRun).toBeLessThanOrEqual(11)
      if (process.env.GPUIX_RETAIN_OFF !== undefined || process.env.GPUIX_PATCH_OFF !== undefined || verifying) return
      expect(after.patchedFrames - patches.patchedFrames).toBeGreaterThanOrEqual(28)
      expect(JSON.parse(after.declines)).toEqual(JSON.parse(patches.declines))
    } finally {
      root.root.unmount()
      renderer.dispose()
    }
  }, TIMEOUT)
})

/** The text above 100 rows of cells, the shape of the pages the patch counters run on. */
function TickedPage() {
  return (
    <div style={{ display: "flex", flexDirection: "column", width: 700, height: 560, overflow: "hidden" }}>
      <div style={{ padding: 100 }}>
        <div
          style={textStyle(13, { position: "relative", left: 0, top: 0, animation: "tx-grow 1s linear forwards" })}
        >
          {TEXT}
        </div>
      </div>
      {Array.from({ length: 100 }, (_, row) => (
        <div key={row} style={{ display: "flex", flexDirection: "row", gap: 4, padding: 2 }}>
          {Array.from({ length: 4 }, (_, cell) => (
            <div key={cell} style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
              <text style={{ fontSize: 11, color: "#333333" }}>{`r${row} c${cell}`}</text>
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}
