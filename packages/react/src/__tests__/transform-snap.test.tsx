/// A box moved by a translation is where layout snaps it, moved by exactly the translation (#109). Chrome adds the
/// translation of `transform`, `translate` and Motion's `x` to the snapped paint offset of the element and draws the
/// rect through the matrix, so a fractional translation antialiases the edges and an element under `will-change:
/// transform` or `translateZ(0)` is drawn the same. gpuix added the translation rounded to a whole device pixel
/// to the snapped bounds: a thumb at `left: 0; x: 559.6` inside a parent at 1.3 was drawn from 561, where
/// Chrome's edge is 560.6, and the same thumb as `left: 559.6` from 562 (a layout position is rounded as a whole).
///
/// `fixtures/transform-snap-oracle.html` is the page and `fixtures/transform-snap-oracle.json` what headless Chrome
/// paints from it (`transform-snap-oracle.sh`): 528 boxes of 24 x 12 on a grey page, in parents at 0 to 1.7 and with a
/// `left` of 0 or 0.3 of their own, moved by -0.6 to 559.6 as `left`, `transform`, `will-change: transform` or
/// `translateX() translateZ(0)`; a black background, a 2px black border, or both. The edges are read along the middle of
/// the box, in CSS px, at device scale 1 and 2 (`transform-snap-oracle.py` says how: a covered pixel moves an edge by its
/// coverage).
///
/// What the oracle says about Chrome:
///  - A transformed box is the snapped box of where layout puts it (the parent and its own `left` together, rounded to a
///    device pixel) moved by exactly the translation. `transform`, `will-change` and `translateZ(0)` give the same edges.
///  - A box moved by `left` has the edges layout rounds the whole position to, so the two rules differ by up to a pixel.
///  - Text moved by a fractional translation is on a subpixel position, as it is when `left` is fractional.
///
/// Where gpuix is compared with Chrome: every edge within a tenth of a device pixel, apart from the outer edge of a box
/// with a border and a background, which Chrome blends from two coverages (the background is painted under the border) and
/// the oracle does not read.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const TIMEOUT = 120_000

type Scale = 1 | 2
type Kind = "left" | "transform" | "willchange" | "z0"
type Type = "fill" | "border" | "both"

type Case = {
  id: string
  type: Type
  kind: Kind
  parentLeft: number
  left: number
  translate: number
  x: number
  y: number
  edges: number[]
}

const ORACLE: Record<"x1" | "x2", { scale: number; cases: Case[] }> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "transform-snap-oracle.json"), "utf8")
)

const PAGE = { width: 1280, height: 5900 }
const BOX = { width: 24, height: 12, top: 2 }
const GREY = 0x80 / 255
const BLACK = 0
const WHITE = 1
/** A pixel counts as a region's own value within this much of a unit of luma. */
const FLAT = 1.5 / 255
/** Edges are compared to a tenth of a device pixel: Skia's analytic coverage and the shader's differ a little. */
const EDGE_TOLERANCE = 0.1

function luma(rgb: ArrayLike<number>) {
  return (0.2126 * rgb[0]! + 0.7152 * rgb[1]! + 0.0722 * rgb[2]!) / 255
}

/** The edge between a region of value `a` on its left and `b` on its right, as `transform-snap-oracle.py` finds it. */
function edge(values: number[], start: number, a: number, b: number) {
  let at = start
  while (at < values.length && Math.abs(values[at]! - a) <= FLAT) at++
  const first = at
  let total = 0
  while (at < values.length && Math.abs(values[at]! - b) > FLAT) {
    total += (values[at]! - b) / (a - b)
    at++
  }
  return { position: first + total, end: at }
}

/** The edges of one kind of box in order, each searched for after the one before. */
function edgesBetween(values: number[], transitions: ReadonlyArray<readonly [number, number]>) {
  const found: number[] = []
  let at = 0
  for (const transition of transitions) {
    const next = edge(values, at, transition[0], transition[1])
    found.push(next.position)
    at = next.end
  }
  return found
}

function edges(type: Type, values: number[]): number[] {
  if (type === "fill") {
    return edgesBetween(values, [
      [GREY, BLACK],
      [BLACK, GREY],
    ])
  }
  if (type === "border") {
    return edgesBetween(values, [
      [GREY, BLACK],
      [BLACK, GREY],
      [GREY, BLACK],
      [BLACK, GREY],
    ])
  }
  // The outer edge is read to get past it, and left out.
  return edgesBetween(values, [
    [GREY, BLACK],
    [BLACK, WHITE],
    [WHITE, BLACK],
  ]).slice(1)
}

function boxStyle(c: Pick<Case, "type" | "kind" | "left" | "translate">): React.CSSProperties {
  const style: React.CSSProperties = {
    position: "absolute",
    top: BOX.top,
    width: BOX.width,
    height: BOX.height,
    boxSizing: "border-box",
  }
  if (c.type === "fill") style.backgroundColor = "#000000"
  if (c.type === "border") style.border = "2px solid #000000"
  if (c.type === "both") {
    style.border = "2px solid #000000"
    style.backgroundColor = "#ffffff"
  }
  if (c.kind === "left") return { ...style, left: c.left + c.translate }
  style.left = c.left
  style.transform = c.kind === "z0" ? `translateX(${c.translate}px) translateZ(0)` : `translateX(${c.translate}px)`
  return style
}

function parentOf(c: Case, child: React.ReactNode) {
  return (
    <div key={c.id} style={{ position: "absolute", left: c.x + 10 + c.parentLeft, top: c.y, width: 30, height: 16 }}>
      {child}
    </div>
  )
}

function screenshot(root: ReturnType<typeof createTestRoot>, name: string, scale: number) {
  root.renderer.dispatchNativeEvents()
  const file = path.join(SHOTS_DIR, `transform-snap-${name}-x${scale}.png`)
  root.renderer.captureScreenshot(file)
  const pixels = readPixels(file)
  expect(pixels.width).toBe(PAGE.width * scale)
  return pixels
}

/** The edges of a case in a screenshot, the window the oracle reads: from 4 px before the nominal left edge to 30 after. */
function edgesIn(pixels: Pixels, scale: Scale, c: Case): number[] {
  const nominal = c.x + 10 + c.parentLeft + c.left + c.translate
  const lo = Math.floor((nominal - 4) * scale)
  const hi = Math.floor((nominal + 30) * scale)
  const y = Math.floor((c.y + BOX.top + BOX.height / 2) * scale)
  const values: number[] = []
  for (let x = lo; x < hi; x++) values.push(luma(pixels.at(x, y)))
  return edges(c.type, values).map((found) => (lo + found) / scale)
}

function paint(scale: Scale, element: (c: Case) => React.ReactNode, cases: Case[]) {
  const root = createTestRoot({ width: PAGE.width, height: PAGE.height, scaleFactor: scale })
  try {
    root.render(
      <div style={{ position: "relative", width: PAGE.width, height: PAGE.height, backgroundColor: "#808080" }}>
        {cases.map((c) => parentOf(c, element(c)))}
      </div>
    )
    return screenshot(root, "page", scale)
  } finally {
    root.unmount()
  }
}

const PAINTED = {} as Record<string, Pixels>
function painted(scale: Scale) {
  return (PAINTED[`css${scale}`] ??= paint(scale, (c) => <div style={boxStyle(c)} />, ORACLE[`x${scale}`].cases))
}

function expectEdges(own: number[], reference: Case, scale: Scale, what: string) {
  own.forEach((found, index) => {
    expect(
      Math.abs(found - reference.edges[index]!) * scale,
      `${what}: edge ${index} at ${found} against Chrome's ${reference.edges[index]} (device px off)`
    ).toBeLessThanOrEqual(EDGE_TOLERANCE + 1e-6)
  })
}

describeNative("a translated box against Chrome", () => {
  for (const scale of [1, 2] as const) {
    for (const kind of ["left", "transform", "willchange", "z0"] as const) {
      it(`has Chrome's edges for ${kind} at scale ${scale}`, () => {
        const cases = ORACLE[`x${scale}`].cases.filter((c) => c.kind === kind)
        expect(cases.length).toBeGreaterThan(100)
        const pixels = painted(scale)
        for (const c of cases) {
          expectEdges(
            edgesIn(pixels, scale, c),
            c,
            scale,
            `${c.type} ${c.kind} parent ${c.parentLeft} left ${c.left} translate ${c.translate} at scale ${scale}`
          )
        }
      }, TIMEOUT)
    }

    it(`puts a box moved by Motion's x where a transform puts it at scale ${scale}`, () => {
      const cases = ORACLE[`x${scale}`].cases.filter((c) => c.kind === "transform" && c.type !== "both")
      const pixels = paint(
        scale,
        (c) => (
          <motion.div
            initial={{ x: c.translate }}
            animate={{ x: c.translate }}
            style={{ ...boxStyle({ ...c, kind: "transform", translate: 0 }), transform: undefined }}
          />
        ),
        cases
      )
      for (const c of cases) {
        expectEdges(edgesIn(pixels, scale, c), c, scale, `${c.type} x ${c.translate} in a parent at ${c.parentLeft}, left ${c.left}`)
      }
    }, TIMEOUT)
  }
})
