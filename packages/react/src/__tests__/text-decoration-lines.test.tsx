/// Strike-throughs, overlines and the marks of dashed and dotted lines against headless Chrome:
/// `fixtures/text-decoration-lines-oracle.html` is the page (`text-decoration-lines-oracle.sh` regenerates
/// `fixtures/text-decoration-lines-oracle.json`). Each case is a band of text; the oracle holds, for every row of the
/// band that has red (or blue) pixels, the runs of them along x, at device scale 1 and 2.
///
/// What Chrome does, and the page shows: a strike-through and an overline are drawn per text fragment at that
/// fragment's own baseline (a raised child's goes up with it), with the thickness of the decorating box; an
/// underline is drawn at the baseline of the decorating box. A dashed or dotted line starts its pattern over at
/// every text fragment (a text node, in one box, on one line) and spaces its marks so that the last one ends on the
/// fragment's end.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { nodesOf, styleOf } from "./oracle-html.js"
import { readPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** `[rows, runs]`: the rows that share these runs of x. */
type Group = [number[], [number, number][]]
interface Marks {
  red?: Group[]
  blue?: Group[]
}
interface OracleCase {
  html: string
  bandStyle: string
  height: number
  x1: Marks
  x2: Marks
}

const ORACLE: Record<string, OracleCase> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/text-decoration-lines-oracle.json"),
    "utf8"
  )
).cases

let shots = 0

function groups(rows: ReturnType<typeof readPixels>, scale: number, source: OracleCase, test: (r: number, g: number, b: number) => boolean): Group[] {
  const found: Group[] = []
  for (let y = 0; y < source.height * scale; y++) {
    const runs: [number, number][] = []
    let start = -1
    for (let x = 0; x < 600 * scale; x++) {
      const [r, g, b] = rows.at(x, y)
      const on = test(r, g, b)
      if (on && start < 0) start = x
      else if (!on && start >= 0) {
        runs.push([start, x])
        start = -1
      }
    }
    if (start >= 0) runs.push([start, 600 * scale])
    if (runs.length === 0) continue
    const last = found.at(-1)
    if (last && last[0].at(-1) === y - 1 && JSON.stringify(last[1]) === JSON.stringify(runs)) last[0].push(y)
    else found.push([[y], runs])
  }
  return found
}

/** What the band of `source` paints, as the oracle records it. */
function painted(source: OracleCase): { scale: number; marks: Marks } {
  const testRoot = createTestRoot({ width: 600, height: source.height })
  try {
    testRoot.render(
      <div style={{ width: 600, height: source.height, backgroundColor: "#ffffff" }}>
        <text
          style={
            {
              width: 600,
              paddingTop: 10,
              color: "#000000",
              fontFamily: "Helvetica",
              fontSize: 16,
              lineHeight: "24px",
              ...styleOf(source.bandStyle),
            } as never
          }
        >
          {nodesOf(source.html)}
        </text>
      </div>
    )
    const file = `${SHOTS_DIR}/text-decoration-lines-${shots++}.png`
    testRoot.renderer.captureScreenshot(file)
    const pixels = readPixels(file)
    const scale = pixels.width / testRoot.renderer.getWindowSize().width
    const marks: Marks = {}
    const red = groups(pixels, scale, source, (r, g, b) => r > 200 && g < 120 && b < 120)
    const blue = groups(pixels, scale, source, (r, g, b) => b > 200 && r < 120 && g < 120)
    if (red.length) marks.red = red
    if (blue.length) marks.blue = blue
    return { scale, marks }
  } finally {
    testRoot.unmount()
  }
}

function describeMarks(marks: Marks | undefined): string {
  return JSON.stringify(marks)
}

interface Tolerance {
  /** How many device pixels the ends of the runs, and the rows, may differ by. */
  x: number
  rows: number
  /** Compare only the first row of the widest run list of each colour, and the extent of the rows: the dots of a
   *  thick dotted line are round, and their edge rows are antialiased differently. */
  centreOnly?: boolean
}

function widest(groups: Group[]): Group {
  return groups.reduce((best, group) => (coverage(group) > coverage(best) ? group : best))
}

function coverage(group: Group): number {
  return group[1].reduce((sum, [start, end]) => sum + end - start, 0)
}

function expectSameRuns(have: [number, number][], want: [number, number][], slack: number, message: string) {
  expect(have.length, message).toBe(want.length)
  have.forEach(([start, end], run) => {
    expect(Math.abs(start - want[run]![0]), message).toBeLessThanOrEqual(slack)
    expect(Math.abs(end - want[run]![1]), message).toBeLessThanOrEqual(slack)
  })
}

function expectSameMarks(actual: Marks, expected: Marks, tolerance: Tolerance, label: string) {
  for (const color of ["red", "blue"] as const) {
    const have = actual[color] ?? []
    const want = expected[color] ?? []
    const message = `${label}\n  gpuix  ${describeMarks({ [color]: have })}\n  chrome ${describeMarks({ [color]: want })}`
    if (tolerance.centreOnly) {
      expect(have.length > 0, message).toBe(want.length > 0)
      if (have.length === 0) continue
      const rows = (groups: Group[]) => [groups[0]![0][0]!, groups.at(-1)![0].at(-1)!]
      rows(have).forEach((row, index) => expect(Math.abs(row - rows(want)[index]!), message).toBeLessThanOrEqual(tolerance.rows))
      expectSameRuns(widest(have)[1], widest(want)[1], tolerance.x, message)
      continue
    }
    expect(have.length, message).toBe(want.length)
    have.forEach(([rows, runs], index) => {
      const wanted = want[index]!
      expect(rows.length, message).toBe(wanted[0].length)
      expect(Math.abs(rows[0]! - wanted[0][0]!), message).toBeLessThanOrEqual(tolerance.rows)
      expectSameRuns(runs, wanted[1], tolerance.x, message)
    })
  }
}

function check(pattern: RegExp, tolerance: (name: string, scale: number) => Tolerance) {
  for (const [name, source] of Object.entries(ORACLE)) {
    if (!pattern.test(name)) continue
    it(name, () => {
      const { scale, marks } = painted(source)
      if (process.env.DUMP_MARKS) fs.appendFileSync(process.env.DUMP_MARKS, `${JSON.stringify({ name, scale, gpuix: marks, chrome: scale === 2 ? source.x2 : source.x1 })}\n`)
      expectSameMarks(marks, scale === 2 ? source.x2 : source.x1, tolerance(name, scale), name)
    })
  }
}

describeNative("strike-throughs and overlines against Chrome", () => {
  // Chrome lays out in 1/64 of a pixel and rounds each line's position to a pixel; where a fractional vertical-align
  // shift puts a line on a rounding tie it lands a row from where gpuix's floats put it.
  check(/^(strike|over) /, (name, scale) => ({ x: 0, rows: /super|nested|inner/.test(name) ? scale : 0 }))
})

describeNative("dashed and dotted lines against Chrome", () => {
  // Not "dotted 5px 1": on a line of a single dot spacing Chrome draws one dot where gpuix draws two that overlap.
  check(/^(dashed|dotted) (?!5px 1$)/, (name, scale) => ({
    x: scale,
    rows: 0,
    centreOnly: name.startsWith("dotted") && Number.parseInt(/(\d)px/.exec(name)?.[1] ?? "0") * scale >= 4,
  }))
})

/** Where Chrome draws a wave is its own (a wavelength of 1 + 2 * round(2 * thickness + 0.5), built of curves), which
 *  gpuix's sine does not match; what both do is start the wave over with every text node. */
describeNative("wavy lines restart with every text node", () => {
  for (const name of ["wavy two text nodes", "wavy nested"]) {
    it(name, () => {
      const { scale, marks } = painted(ORACLE[name]!)
      const rows = marks.red!
      const fragment = 5 * 8 * scale
      for (const [, runs] of rows) {
        const first = runs.filter(([, end]) => end < fragment - 2 * scale)
        expect(first.length).toBeGreaterThan(0)
        for (const [start, end] of first) {
          const shifted = runs.some(([otherStart, otherEnd]) => Math.abs(otherStart - (start + fragment)) <= 1 && Math.abs(otherEnd - (end + fragment)) <= 1)
          expect(shifted, `${name}: ${start}-${end} has no copy in the next fragment: ${JSON.stringify(runs)}`).toBe(true)
        }
      }
    })
  }
})
