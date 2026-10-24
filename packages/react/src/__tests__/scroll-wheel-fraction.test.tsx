/// A wheel over a scroller at its end that chains to the page, with trackpad deltas between two device pixels, against
/// Chrome (round 20, 2e). The scroller under the wheel keeps the offset it is given to the fraction and reads it back
/// rounded half up to a device pixel, so a delta of 0.5 on a page at 48.7 stores 49.2 and reads 49 as it did: the read
/// does not move on every event, in Chrome either (the page reads 49, 49, 50 for 0.7, 0.5, 1 at scale 1). Rounding the
/// stored offset instead would make a scroller at 399 of 399 take part of a 0.5 delta, and hold the page back.
///
/// `fixtures/wheel-fraction-oracle.ts` drives headless Chrome (regenerate with `bun fixtures/wheel-fraction-oracle.ts`):
/// the page and the scroller of `mount` below, wheel events of 40px over the scroller until the page moves, then those of
/// a case, with the page's and the scroller's `scrollTop` read after each. `fixtures/wheel-fraction-oracle.json` is
/// device scale 1, `-x2.json` scale 2.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface Case {
  start: number[]
  steps: number[][]
}

const oracleOf = (file: string): Record<string, Case> =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLE: Record<1 | 2, Record<string, Case>> = {
  1: oracleOf("wheel-fraction-oracle.json"),
  2: oracleOf("wheel-fraction-oracle-x2.json"),
}

function mount(rows: number, scale: number) {
  const root = createTestRoot({ width: 500, height: 500, scaleFactor: scale })
  root.render(
    <div testId="page" style={{ width: 400, height: 400, overflowY: "scroll", padding: 20 }}>
      <div testId="inner" style={{ width: 300, height: 163, overflowY: "scroll" }}>
        <div style={{ display: "flex", flexDirection: "column", padding: 8 }}>
          {Array.from({ length: rows }, (_, index) => (
            <div key={index} style={{ height: 19.5, flexShrink: 0 }} />
          ))}
        </div>
      </div>
      <div style={{ height: 3000, flexShrink: 0 }} />
    </div>
  )
  const renderer = root.renderer
  const top = (testId: string) => renderer.getScrollOffset(renderer.findByTestId(testId)!.id)![1] as number
  return { ...root, renderer, top }
}

describeNative("a wheel of fractions that chains from a scroller at its end to the page, against Chrome", () => {
  for (const scale of [1, 2] as const) {
    for (const [key, expected] of Object.entries(ORACLE[scale])) {
      it(`${key} at scale ${scale}`, () => {
        const [rows, over] = key.split(" ") as [string, "content" | "track"]
        const { renderer, top, unmount } = mount(Number(rows), scale)
        try {
          // The scroller is 300 wide at x 20 and its scrollbar track is the last 15px of that.
          const pointer = over === "track" ? { x: 316, y: 100 } : { x: 170, y: 100 }
          renderer.nativeSimulateMouseMove(pointer.x - 1, pointer.y)
          renderer.nativeSimulateMouseMove(pointer.x, pointer.y)
          for (let tick = 0; tick < 100 && top("page") === 0; tick++) renderer.nativeSimulateScrollWheel(70, 70, 0, 40)
          expect([top("page"), top("inner")]).toEqual(expected.start.slice(0, 2))
          const steps: number[][] = []
          for (const step of expected.steps) {
            renderer.nativeSimulateScrollWheel(pointer.x, pointer.y, 0, step[0] as number)
            steps.push([step[0] as number, top("page"), top("inner")])
          }
          expect(steps).toEqual(expected.steps)
        } finally {
          unmount()
        }
      })
    }
  }

  it("moves the page on a delta of 0.5 that the read does not show, and the scroller at its end keeps none of it", () => {
    const { renderer, top, unmount } = mount(28, 1)
    try {
      renderer.nativeSimulateMouseMove(169, 100)
      renderer.nativeSimulateMouseMove(170, 100)
      for (let tick = 0; tick < 100 && top("page") === 0; tick++) renderer.nativeSimulateScrollWheel(70, 70, 0, 40)
      for (const delta of [0.5, 1, 1.5, 2, 3, 0.7]) renderer.nativeSimulateScrollWheel(170, 100, 0, delta)
      expect(top("page")).toBe(49)
      renderer.nativeSimulateScrollWheel(170, 100, 0, 0.5)
      expect([top("page"), top("inner")]).toEqual([49, 399])
      renderer.nativeSimulateScrollWheel(170, 100, 0, 0.5)
      expect([top("page"), top("inner")]).toEqual([50, 399])
    } finally {
      unmount()
    }
  })
})
