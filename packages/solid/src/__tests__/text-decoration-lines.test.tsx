/// Strike-throughs, overlines and the marks of dashed lines against headless Chrome, the Solid half of the React
/// test of the same name: the cases are in `packages/react/src/__tests__/fixtures/text-decoration-lines-oracle.json`,
/// which `text-decoration-lines-oracle.sh` regenerates. A case is a band of text; the oracle holds the runs of red
/// pixels along x of each row of it that has some, at device scale 1.

import fs from "fs"
import os from "os"
import path from "path"
import { fileURLToPath } from "url"
import type { JSX } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels } from "./pixels.js"

type Group = [number[], [number, number][]]
interface OracleCase {
  height: number
  x1: { red?: Group[] }
}

const ORACLE: Record<string, OracleCase> = JSON.parse(
  fs.readFileSync(
    path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      "../../../react/src/__tests__/fixtures/text-decoration-lines-oracle.json"
    ),
    "utf8"
  )
).cases

const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-decoration-"))

const LINE = (value: string) => ({ textDecoration: value })
const BAND = { width: 600, paddingTop: 10, color: "#000000", fontFamily: "Helvetica", fontSize: 16, lineHeight: "24px" } as const

/** The cases of the oracle, as the spans their HTML stands for. */
const CASES: Record<string, { band?: JSX.CSSProperties; body: () => JSX.Element }> = {
  "strike parent child super": {
    body: () => (
      <span style={LINE("line-through #f00")}>
        <span>xx</span>
        <span style={{ verticalAlign: "super" }}>xx</span>
        <span>xx</span>
      </span>
    ),
  },
  "over parent child bigger font": {
    body: () => (
      <span style={LINE("overline #f00")}>
        <span>xx</span>
        <span style={{ fontSize: 28 }}>xx</span>
        <span>xx</span>
      </span>
    ),
  },
  "dashed nested bold": {
    body: () => (
      <span style={{ ...LINE("underline dashed #f00"), textDecorationThickness: "2px" }}>
        {"xxxxxx"}
        <span style={{ fontWeight: 700 }}>xxxxxx</span>
      </span>
    ),
  },
  "dashed wrapped": {
    band: { width: 130 },
    body: () => (
      <span style={{ ...LINE("underline dashed #f00"), textDecorationThickness: "2px" }}>xxxxx xxxxx xxxxxx xx</span>
    ),
  },
}

/** The groups of consecutive rows that have the same runs of red. */
function groups(file: string, renderer: ReturnType<typeof createTestRoot>["renderer"], height: number): Group[] {
  const pixels = readLogicalPixels(file, renderer)
  const found: Group[] = []
  for (let y = 0; y < height; y++) {
    const runs: [number, number][] = []
    let start = -1
    for (let x = 0; x < 600; x++) {
      const [r, g, b] = pixels.at(x, y)
      const red = r > 200 && g < 120 && b < 120
      if (red && start < 0) start = x
      else if (!red && start >= 0) {
        runs.push([start, x])
        start = -1
      }
    }
    if (runs.length === 0) continue
    const last = found.at(-1)
    if (last && last[0].at(-1) === y - 1 && JSON.stringify(last[1]) === JSON.stringify(runs)) last[0].push(y)
    else found.push([[y], runs])
  }
  return found
}

describe.skipIf(!hasNativeTestRenderer)("Solid text decoration lines against Chrome", () => {
  for (const [name, spec] of Object.entries(CASES)) {
    it(name, () => {
      const source = ORACLE[name]!
      const app = createTestRoot({ width: 600, height: source.height })
      try {
        app.render(() => (
          <div style={{ width: 600, height: source.height, backgroundColor: "#ffffff" }}>
            <text style={{ ...BAND, ...spec.band } as never}>{spec.body()}</text>
          </div>
        ))
        const file = path.join(shots, `${name.replaceAll(" ", "-")}.png`)
        app.renderer.captureScreenshot(file)
        // Logical pixels: the marks of a display of a larger scale are sampled, so a mark may differ by one.
        const have = groups(file, app.renderer, source.height)
        const want = source.x1.red ?? []
        // A raised line lands a row from Chrome's where a fractional shift is on a rounding tie.
        const rowSlack = name.includes("super") ? 1 : 0
        expect(have.length, JSON.stringify(have)).toBe(want.length)
        have.forEach(([rows, runs], index) => {
          expect(Math.abs(rows[0]! - want[index]![0][0]!)).toBeLessThanOrEqual(rowSlack)
          expect(runs.length, `${name}: ${JSON.stringify(runs)} against ${JSON.stringify(want[index]![1])}`).toBe(
            want[index]![1].length
          )
          runs.forEach(([start, end], run) => {
            expect(Math.abs(start - want[index]![1][run]![0])).toBeLessThanOrEqual(1)
            expect(Math.abs(end - want[index]![1][run]![1])).toBeLessThanOrEqual(1)
          })
        })
      } finally {
        app.unmount()
      }
    })
  }
})
