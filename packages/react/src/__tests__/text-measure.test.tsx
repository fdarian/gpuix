/// How wide and how tall a text is laid out, in Chrome's unit at the scale of the window: the inline size of a text is its
/// advance ceiled to 1/64 of a device pixel (`ShapeResult::SnappedWidth`), so 1/128 of a pixel at scale 2, per style run
/// (an item), and a piece of an item (a word for `min-content`) is the difference of the ceils of its edges. A font with a
/// legacy kern table (Helvetica, Times) kerns a pair as HarfBuzz splits it between the two glyphs, so a word after a kerned
/// space starts half a kern later than CoreText puts it, as far as the measure goes. A line
/// height is a multiple of 1/64 of a device pixel too: a number (`12px` at `1.2`) is cut, a length or a percentage is rounded
/// (`14.95px` and `115%` of 13px are 14.953125px; `1.15` is 14.9375px at scale 1 and 14.9453125px at scale 2).
///
/// `fixtures/text-measure-oracle.html` is the page and `-oracle.json` / `-oracle-x2.json` the boxes headless Chrome reports for it at
/// device scale 1 and 2 (`sh fixtures/dsf-oracle.sh text-measure`; `--force-device-scale-factor`, which is a display of that
/// scale and measures a font at its device size). The id of a box holds the parameters of its case, see the page.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]

const SCALES = [1, 2] as const
type Scale = (typeof SCALES)[number]

const oracleOf = (file: string): Record<string, Rect> =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLE: Record<Scale, Record<string, Rect>> = {
  1: oracleOf("text-measure-oracle.json"),
  2: oracleOf("text-measure-oracle-x2.json"),
}

/** Both sides measure in multiples of 1/64 of a device pixel, so a box is Chrome's to the float. */
const EXACT = 1e-4

const lineHeightOf = (value: string): number | string => (/^[\d.]+$/.test(value) ? Number(value) : value)

const NUMBER = (value: string) => Number(value)

function font(family: string, size: string) {
  return { fontFamily: family, fontSize: NUMBER(size), color: "#000000" }
}

const column = { display: "flex", flexDirection: "column", alignItems: "flex-start", width: 1280 } as const

interface Case {
  id: string
  box: string
  element: (ref: (name: string) => (instance: PublicInstance | null) => void) => React.ReactNode
}

function casesOf(kind: string): Case[] {
  const cases: Case[] = []
  for (const id of Object.keys(ORACLE[1])) {
    const parts = id.split("|")
    if (parts[0] !== kind || id.endsWith(".label")) continue
    switch (kind) {
      case "lh": {
        const [, family, size, lineHeight] = parts as [string, string, string, string]
        cases.push({
          id,
          box: id,
          element: (ref) => (
            <div ref={ref(id)} style={{ width: 1280, ...font(family, size), lineHeight: lineHeightOf(lineHeight) }}>
              <span>Hxg</span>
            </div>
          ),
        })
        break
      }
      case "w": {
        const [, family, size, text] = parts as [string, string, string, string]
        cases.push({
          id,
          box: id,
          element: (ref) => (
            <div style={{ ...column, ...font(family, size), lineHeight: "20px" }}>
              <div ref={ref(id)}>{text}</div>
            </div>
          ),
        })
        break
      }
      case "mc": {
        const [, family, size, text] = parts as [string, string, string, string]
        cases.push({
          id,
          box: id,
          element: (ref) => (
            <div style={{ ...column, ...font(family, size), lineHeight: "20px" }}>
              <div ref={ref(id)} style={{ width: "min-content" }}>
                {text}
              </div>
            </div>
          ),
        })
        break
      }
      case "wrap": {
        const [, family, size, width, text] = parts as [string, string, string, string, string]
        cases.push({
          id,
          box: id,
          element: (ref) => (
            <div ref={ref(id)} style={{ ...font(family, size), lineHeight: 1.3, width: NUMBER(width) }}>
              {text}
            </div>
          ),
        })
        break
      }
      case "mix": {
        const [, family, size] = parts as [string, string, string]
        cases.push({
          id,
          box: id,
          element: (ref) => (
            <div style={{ ...column, ...font(family, size), lineHeight: "20px" }}>
              <div ref={ref(id)}>
                Fractional <span style={{ fontWeight: 700 }}>widths</span> of{" "}
                <span style={{ fontStyle: "italic" }}>the</span> quick
              </div>
            </div>
          ),
        })
        break
      }
      case "trim": {
        const [, family, size, lineHeight, text] = parts as [string, string, string, string, string]
        cases.push({
          id,
          box: `${id}.label`,
          element: (ref) => (
            <div
              ref={ref(id)}
              style={{ ...font(family, size), height: 40, width: 1280, display: "flex", alignItems: "center", lineHeight: lineHeightOf(lineHeight) }}
            >
              <span ref={ref(`${id}.label`)} style={{ textBox: "trim-both cap alphabetic" }}>
                {text}
              </span>
            </div>
          ),
        })
        break
      }
    }
  }
  return cases
}

/** The cases of a kind in one window, one below the other, each starting on a whole pixel. */
function layout(scale: Scale, cases: Case[]) {
  const root = createTestRoot({ width: 1280, height: 800 })
  root.renderer.setWindowScaleFactor(scale)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  root.render(
    <div style={{ width: 1280 }}>
      {cases.map(({ id, element }) => (
        <div key={id} style={{ height: 60, flexShrink: 0 }}>
          {element(ref)}
        </div>
      ))}
    </div>
  )
  root.renderer.dispatchNativeEvents()
  const rect = (name: string, origin: string): Rect => {
    const box = refs.get(name)?.getBoundingClientRect()
    expect(box, `no element ${name}`).toBeDefined()
    const from = refs.get(origin)?.getBoundingClientRect()
    return [(box?.x ?? 0) - (from?.x ?? 0), (box?.y ?? 0) - (from?.y ?? 0), box?.width ?? 0, box?.height ?? 0]
  }
  return { ...root, rect }
}

/** One line per case whose width or height is not Chrome's. */
function differences(scale: Scale, kind: string, read: (id: string, rect: Rect, chrome: Rect) => string[]) {
  const cases = casesOf(kind)
  expect(cases.length, `no ${kind} cases`).toBeGreaterThan(0)
  const { rect, unmount } = layout(scale, cases)
  try {
    return cases.flatMap(({ id, box }) => {
      const chrome = ORACLE[scale][box]
      expect(chrome, `no oracle box ${box}`).toBeDefined()
      const origin = box === id ? id : id
      return read(id, rect(box, origin), chrome as Rect)
    })
  } finally {
    unmount()
  }
}

const off = (what: string, actual: number, expected: number) =>
  Math.abs(actual - expected) > EXACT ? [`${what} ${actual} != Chrome's ${expected}`] : []

describeNative.each(SCALES)("text measured at scale %i", (scale) => {
  it("has the line height of Chrome: a number cut, a length and a percentage rounded", () => {
    const found = differences(scale, "lh", (id, rect, chrome) => off(`${id}: height`, rect[3], chrome[3]))
    expect(found).toEqual([])
  })

  it("is as wide as Chrome's inline size of it, ceiled to a device 64th", () => {
    const found = differences(scale, "w", (id, rect, chrome) => [
      ...off(`${id}: width`, rect[2], chrome[2]),
      ...off(`${id}: height`, rect[3], chrome[3]),
    ])
    expect(found).toEqual([])
  })

  // A word is the difference of the ceils of its edges from the start of its run, so the second word of `Hello world` is
  // a unit narrower than the word alone where the edges fall that way, and a word after a kerned space (`a To`) is narrower
  // by half the kern.
  it("has the min-content width of Chrome: the widest word as the difference of the ceils of its edges", () => {
    const found = differences(scale, "mc", (id, rect, chrome) => [
      ...off(`${id}: width`, rect[2], chrome[2]),
      ...off(`${id}: height`, rect[3], chrome[3]),
    ])
    expect(found).toEqual([])
  })

  it("wraps in a width as Chrome does", () => {
    const found = differences(scale, "wrap", (id, rect, chrome) => off(`${id}: height`, rect[3], chrome[3]))
    expect(found).toEqual([])
  })

  // Chrome ceils the width of each run of a line (a text item has its own `SnappedWidth`) and adds them up.
  it("is as wide as the runs of a text, each ceiled on its own", () => {
    const found = differences(scale, "mix", (id, rect, chrome) => off(`${id}: width`, rect[2], chrome[2]))
    expect(found).toEqual([])
  })

  it("centres a cap-trimmed label to Chrome's layout units", () => {
    const found = differences(scale, "trim", (id, rect, chrome) => [
      ...off(`${id}: y`, rect[1], chrome[1]),
      ...off(`${id}: width`, rect[2], chrome[2]),
      ...off(`${id}: height`, rect[3], chrome[3]),
    ])
    expect(found).toEqual([])
  })
})
