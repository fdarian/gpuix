/// An authored length keeps its fraction, as in Chrome (#65): it is cut toward zero to 1/64 of a device pixel (`100.4px`
/// is `100.390625` at scale 1 and `100.3984375` at scale 2; `-10.01px` is `-10` and `-10.0078125`), a `calc()` and a
/// percentage are cut after they are resolved wherever one becomes a length (`25%` of `101.3px` is `25.3125` at scale 1
/// and `25.3203125` at scale 2, a negative one is cut toward zero), the edges of a box snap half up to the pixel it
/// paints on (33.3, 33.3 and 33.4 paint 33, 34 and 33 pixels with no gap), and a border width is snapped as a border
/// width (floored to whole device pixels, a width under one is one).
///
/// `fixtures/fractional-lengths-oracle.html` is the page, `fixtures/fractional-lengths-oracle.json` what headless Chrome
/// reports for each case at device scale 1 and 2 (regenerate with `fractional-lengths-oracle.sh`): the box of every
/// element with an id (`getBoundingClientRect` relative to the holder, `clientWidth`, `clientHeight`, `clientLeft` and
/// `clientTop`), and for the painted cases the first and last device pixel column and row of the colour of each box
/// (`painted`) and the pixel runs along lines (`scans`). The painted cases give every element a colour of its own, in the
/// order the page picks them, so the colours of the scans are known here. There is no text, so no font matters.
///
/// gpuix has no `offsetLeft`, `offsetTop`, `offsetWidth` or `offsetHeight`, and no computed style, so the oracle's
/// `offset`, `border` and `computed` are not read: the border widths are read as the painted border and `clientLeft` and
/// `clientTop`.
///
/// Everything is compared exactly but for the rows named in `KNOWN`, each with its reason.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf } from "./oracle-boxes.js"
import { readPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
type Extent = {
  x0: number
  x1: number
  y0: number
  y1: number
}
type Scan = {
  axis: "h" | "v"
  at: number
  start: number
  runs: [string, number, number][]
}
type Side = {
  boxes: Record<string, { rect: Rect; client: Rect }>
  painted?: Record<string, { bg?: Extent; bd?: Extent }>
  scans?: Scan[]
}
type Case = {
  html: string
  /** The markup at scale 2 where the page made another (a highlight positioned from the row it follows). */
  html2?: string
  paint: boolean
  "1": Side
  "2": Side
}

const ORACLE: Record<string, Case> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/fractional-lengths-oracle.json"),
    "utf8"
  )
)
const NAMES = Object.keys(ORACLE)
const SCALES = [1, 2] as const

const CONTENT_BOX_PERCENT =
  "gpuix has no content-box: a px height with a percentage or calc() padding cannot be written as a border-box height"

/** The window the cases are laid out in: the holder is at (20, 20), the cells of the oracle's page are 400 x 200. */
const WINDOW = { width: 400, height: 200 }
const ORIGIN = 20

/** Why a group of a case is not compared to Chrome, `"<case> <group>"` (the group is `rect`, `client`, `painted` or `scans`). */
const KNOWN: Record<string, string> = {
  "calc padding margin rect": CONTENT_BOX_PERCENT,
  "calc padding margin client": CONTENT_BOX_PERCENT,
  "percent of 33.3 with padding rect": CONTENT_BOX_PERCENT,
  "percent of 33.3 with padding client": CONTENT_BOX_PERCENT,
}

type Colors = Record<string, { bg: string; bd?: string }>

/** The colours the oracle page gives the elements of its painted cases: one counter over all of them, `bg` and then
 *  `bd` for an element with a `data-border`, none for a `data-nobg`. */
const PALETTE = (() => {
  const result = new Map<string, Colors>()
  let next = 0
  const color = () => {
    const k = next++
    const channels = [20 + 28 * (k % 8), 20 + 28 * (Math.floor(k / 8) % 8), 20 + 28 * Math.floor(k / 64)]
    return channels.map((value) => value.toString(16).padStart(2, "0")).join("")
  }
  for (const name of NAMES) {
    if (!ORACLE[name]!.paint) continue
    const colors: Colors = {}
    for (const tag of ORACLE[name]!.html.matchAll(/<[a-z]+((?:\s+[a-z-]+(?:="[^"]*")?)*)\s*>/g)) {
      const attributes = tag[1]!
      const id = /\sid="([^"]*)"/.exec(attributes)?.[1]
      if (id === undefined || /\sdata-nobg[\s="]|\sdata-nobg$/.test(attributes)) continue
      colors[id] = { bg: color() }
      if (/\sdata-border/.test(attributes)) colors[id]!.bd = color()
    }
    result.set(name, colors)
  }
  return result
})()

const css = (hex: string) => `rgb(${[0, 2, 4].map((at) => Number.parseInt(hex.slice(at, at + 2), 16)).join(",")})`

/** The box of Chrome's `*{box-sizing:content-box}` as gpuix, which only has the border box, takes it: the width and
 *  height in px grow by the padding and the border, each cut or snapped as Chrome does before they are added.
 *  A cell of a table with `border-collapse: collapse` has half of each of its borders, and the table the outer halves of
 *  the cells at its edges (`collapsedBorder`, its cells' border in px: an outer half on each side is one border).
 *  Percentages and `calc()` cannot be converted and are left (their rows are in `KNOWN`). */
function borderBoxStyle(style: string, scale: number, collapsed: "cell" | "table" | undefined, collapsedBorder: number) {
  const declarations = new Map(
    style
      .split(";")
      .filter(Boolean)
      .map((declaration) => {
        const colon = declaration.indexOf(":")
        return [declaration.slice(0, colon).trim(), declaration.slice(colon + 1).trim()] as const
      })
  )
  if (declarations.get("box-sizing") === "border-box") {
    declarations.delete("box-sizing")
    return [...declarations].map(([name, value]) => `${name}:${value}`).join(";")
  }
  const lengths = (value: string | undefined) => (value ?? "0px").split(/\s+/).map((part) => Number.parseFloat(part))
  /** `[top, right, bottom, left]` of a shorthand, or of its longhands. */
  const sides = (name: string, suffix: string) => {
    const [top = 0, right = top, bottom = top, left = right] = lengths(declarations.get(`${name}${suffix}`))
    const own = (side: string, fallback: number) => {
      const value = declarations.get(`${name}-${side}${suffix}`)
      return value === undefined ? fallback : Number.parseFloat(value)
    }
    return [own("top", top), own("right", right), own("bottom", bottom), own("left", left)]
  }
  const cut = (px: number) => Math.trunc(px * scale * 64) / 64
  const borderWidths = /(^|\s)(none|hidden)(\s|$)/.test(declarations.get("border") ?? declarations.get("border-style") ?? "")
    ? [0, 0, 0, 0]
    : declarations.has("border")
      ? Array(4).fill(Number.parseFloat(declarations.get("border")!))
      : sides("border", "-width")
  const snapped = borderWidths.map((px) => (px === 0 ? 0 : Math.max(1, Math.floor(px * scale))) / (collapsed === "cell" ? 2 : 1))
  const outer = collapsed === "table" ? Math.max(1, Math.floor(collapsedBorder * scale)) : 0
  const padding = sides("padding", "").map(cut)
  for (const [name, around] of [
    ["width", [1, 3]],
    ["height", [0, 2]],
  ] as const) {
    const value = declarations.get(name)
    if (!value || !/^-?[\d.]+px$/.test(value)) continue
    const total = cut(Number.parseFloat(value)) + outer + around.reduce((sum, side) => sum + padding[side]! + snapped[side]!, 0)
    if (Number.isFinite(total)) declarations.set(name, `${total / scale}px`)
  }
  return [...declarations].map(([name, value]) => `${name}:${value}`).join(";")
}

/** `html` as gpuix lays it out: the sizes of the page in the border box, `display: flow-root` as the `overflow` that
 *  makes a block formatting context here, and the colours of the case painted on its elements. */
function prepared(html: string, colors: Colors | undefined, scale: number) {
  const collapsing = html.includes("border-collapse:collapse")
  const cellBorder = Number.parseFloat(/<td[^>]*border:([\d.]+)px/.exec(html)?.[1] ?? "0")
  return html.replace(/<([a-z]+)((?:\s+[a-z-]+(?:="[^"]*")?)*)\s*>/g, (_tag, name: string, attributes: string) => {
    const id = /\sid="([^"]*)"/.exec(attributes)?.[1]
    const own = id === undefined ? undefined : colors?.[id]
    const paint = own
      ? `;background-color:${css(own.bg)};${own.bd ? `border-color:${css(own.bd)};background-clip:padding-box;` : ""}`
      : ""
    const style = /\sstyle="([^"]*)"/.exec(attributes)
    const converted = borderBoxStyle(style?.[1] ?? "", scale, !collapsing ? undefined : name === "table" ? "table" : name === "td" ? "cell" : undefined, cellBorder).replace("display:flow-root", "display:block;overflow:hidden")
    const rest = style ? attributes.replace(style[0], "") : attributes
    return `<${name}${rest} style="${converted}${paint}">`
  })
}

const hex = (pixel: number[]) => pixel.slice(0, 3).map((value) => value.toString(16).padStart(2, "0")).join("")

/** The pixel runs of a screenshot along a scan line, the way the oracle records them. */
function runsOf(pixels: ReturnType<typeof readPixels>, scan: Scan, scale: number) {
  const runs: [string, number, number][] = []
  const length = scan.runs.at(-1)![2]
  for (let index = 0; index < length; index++) {
    const along = scan.start + index
    const color = hex(
      scan.axis === "h"
        ? pixels.at(along + ORIGIN * scale, (ORIGIN + scan.at) * scale)
        : pixels.at((ORIGIN + scan.at) * scale, along + ORIGIN * scale)
    )
    const last = runs.at(-1)
    if (last && last[0] === color) last[2] = index + 1
    else runs.push([color, index, index + 1])
  }
  return runs
}

/** The first and last device pixel column and row of each of `wanted` in the holder's region of a screenshot. */
function extentsOf(pixels: ReturnType<typeof readPixels>, scale: number, wanted: Set<string>) {
  const found = new Map<string, Extent>()
  for (let y = (ORIGIN - 20) * scale; y < (ORIGIN + 180) * scale; y++) {
    for (let x = (ORIGIN - 20) * scale; x < (ORIGIN + 380) * scale; x++) {
      const color = hex(pixels.at(x, y))
      if (!wanted.has(color)) continue
      const extent = found.get(color)
      const [px, py] = [x - ORIGIN * scale, y - ORIGIN * scale]
      if (!extent) found.set(color, { x0: px, x1: px, y0: py, y1: py })
      else {
        extent.x0 = Math.min(extent.x0, px)
        extent.x1 = Math.max(extent.x1, px)
        extent.y1 = py
      }
    }
  }
  return found
}

const same = (actual: unknown, expected: unknown) => JSON.stringify(actual) === JSON.stringify(expected)

/** What differs from Chrome in the case `name` at `scale`, one line per difference, keyed by the group it is in. */
function differences(name: string, scale: 1 | 2) {
  const source = ORACLE[name]!
  const expected = source[String(scale) as "1" | "2"]
  const colors = PALETTE.get(name)
  const html = prepared(scale === 2 ? (source.html2 ?? source.html) : source.html, colors, scale)
  const found: Record<string, string[]> = { rect: [], client: [], painted: [], scans: [] }
  // The screenshot is the top-left of the window in device pixels, as many as its display has: a window of the size of
  // the page in device pixels shows all of it at scale 2 on a display of scale 1.
  const root = createTestRoot({ width: WINDOW.width * scale, height: WINDOW.height * scale })
  try {
    root.renderer.setWindowScaleFactor(scale)
    root.render(
      <div style={{ ...WINDOW, backgroundColor: "#ffffff" }}>
        <div testId="holder" style={{ position: "absolute", left: ORIGIN, top: ORIGIN, width: 360, height: 160 }}>
          {elementsOf(html)}
        </div>
      </div>
    )
    root.renderer.dispatchNativeEvents()
    expect(root.renderer.getA11yTree().frame?.scale_factor, "the window has the scale asked for").toBe(scale)
    const read = (id: string) => {
      const element = root.renderer.findByTestId(id)
      expect(element, `${id} is mounted`).toBeDefined()
      return element!.id
    }
    const holder = root.renderer.getElementBounds(read("holder"))!
    for (const [id, want] of Object.entries(expected.boxes)) {
      const element = read(id)
      const bounds = root.renderer.getElementBounds(element)
      if (!bounds) {
        found.rect!.push(`${id}: has no box`)
        continue
      }
      const rect = [bounds.x - holder.x, bounds.y - holder.y, bounds.width, bounds.height]
      const apart = !same(rect, want.rect)
      if (apart) found.rect!.push(`${id}: ${JSON.stringify(rect)} != Chrome's ${JSON.stringify(want.rect)}`)
      const metrics = root.renderer.getScrollMetrics(element)
      const client = [metrics?.[4], metrics?.[5], bounds.clientLeft, bounds.clientTop]
      if (!same(client, want.client)) found.client!.push(`${id}: client ${JSON.stringify(client)} != Chrome's ${JSON.stringify(want.client)}`)
    }
    if (!source.paint) return found
    const file = `${SHOTS_DIR}/fractional-lengths-${NAMES.indexOf(name)}-${scale}.png`
    fs.rmSync(file, { force: true })
    root.renderer.captureScreenshot(file)
    const pixels = readPixels(file)
    expect(pixels.width, "the screenshot holds the page").toBeGreaterThanOrEqual(WINDOW.width * scale)
    const wanted = new Set(Object.values(colors!).flatMap((entry) => [entry.bg, entry.bd].filter((c): c is string => !!c)))
    const extents = extentsOf(pixels, scale, wanted)
    for (const [id, entry] of Object.entries(colors!)) {
      for (const kind of ["bg", "bd"] as const) {
        const color = entry[kind]
        if (!color) continue
        const want = expected.painted![id]?.[kind]
        const got = extents.get(color)
        if (!same(got, want)) found.painted!.push(`${id} ${kind}: ${JSON.stringify(got)} != Chrome's ${JSON.stringify(want)}`)
      }
    }
    for (const scan of expected.scans!) {
      const runs = runsOf(pixels, scan, scale)
      if (!same(runs, scan.runs)) found.scans!.push(`${scan.axis} ${scan.at}: ${JSON.stringify(runs)} != Chrome's ${JSON.stringify(scan.runs)}`)
    }
    return found
  } finally {
    root.unmount()
  }
}

describeNative("authored lengths keep their fraction", () => {
  it("has an oracle for both scales", () => {
    expect(NAMES.length).toBeGreaterThan(100)
    expect(NAMES.filter((name) => ORACLE[name]!.paint).length).toBeGreaterThan(40)
  })

  for (const scale of SCALES) {
    for (const name of NAMES) {
      it(`is laid out and painted as Chrome does at scale ${scale}: ${name}`, () => {
        const found = differences(name, scale)
        const lines = Object.entries(found).flatMap(([group, list]) => (`${name} ${group}` in KNOWN ? [] : list.map((line) => `${group} ${line}`)))
        expect(lines, `${name} at scale ${scale}`).toEqual([])
      })
    }
  }
})
