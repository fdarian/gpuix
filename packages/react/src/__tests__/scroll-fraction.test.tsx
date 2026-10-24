/// A fractional scroll offset against a scroller of divs in Chrome (round 20, 2e). Chrome snaps the offset a script writes
/// to a device pixel, rounding half up, before it clamps and stores it, and reads that back: `scrollTop = 10.5` is 11 at
/// scale 1 and `10.3` is 10.5 at scale 2. `scrollBy` adds to the offset as it reads back, so `scrollBy(0, 0.5)` three times
/// is 1, 2, 3 at scale 1. The range is `scrollHeight - clientHeight` in device pixels, each rounded, so a write past a
/// fractional range stops on it.
///
/// `fixtures/scroll-fraction-oracle.html` is the page, `fixtures/scroll-fraction-oracle.json` (device scale 1) and
/// `fixtures/scroll-fraction-oracle-x2.json` (scale 2) what headless Chrome reports for it (regenerate with
/// `fixtures/dsf-oracle.sh scroll-fraction`). Each scene is a scroller with rows, written to by a series of operations; the
/// page records what `scrollLeft` and `scrollTop` read back and the box of every row after each. The same scene is rendered
/// here as a div, and as a `<virtual-list>` when it scrolls along y only (`axes: "y"`), and compared exactly.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { styleOf } from "./oracle-html.js"
import { readPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
type Op = ["top", number] | ["left", number] | ["to", number, number] | ["by", number, number]
interface Scene {
  spec: { axes: "x" | "y" | "xy"; list: string; rows: { style: string }[]; ops: Op[] }
  list: Rect
  scrollWidth: number
  scrollHeight: number
  clientWidth: number
  clientHeight: number
  steps: { op: Op; scrollLeft: number; scrollTop: number; rows: Rect[] }[]
}

const SCALES = [1, 2] as const
type Scale = (typeof SCALES)[number]

const oracleOf = (file: string): Record<string, Scene> =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLE: Record<Scale, Record<string, Scene>> = {
  1: oracleOf("scroll-fraction-oracle.json"),
  2: oracleOf("scroll-fraction-oracle-x2.json"),
}

type Kind = "div" | "virtual-list"

const rectOf = (instance: PublicInstance): Rect => {
  const box = instance.getBoundingClientRect()
  return [box.x, box.y, box.width, box.height]
}

const RED = "#ff0000"
const BLUE = "#0000ff"

function mount(scene: Scene, scale: Scale, kind: Kind, colored = false) {
  const root = createTestRoot({ width: 600, height: 400, scaleFactor: scale })
  const rows: (PublicInstance | null)[] = []
  let list: PublicInstance | null = null
  const style: Record<string, unknown> = { ...styleOf(scene.spec.list), ...(colored ? { backgroundColor: "#ffffff" } : {}) }
  // A list scrolls along y only and `overflow` is not its to say.
  if (kind === "virtual-list") delete style.overflow
  const children = scene.spec.rows.map((row, index) => (
    <div
      key={index}
      ref={(instance: PublicInstance | null) => {
        rows[index] = instance
      }}
      style={{ ...styleOf(row.style), flexShrink: 0, ...(colored ? { backgroundColor: index % 2 === 0 ? RED : BLUE } : {}) } as never}
    />
  ))
  const ref = (instance: PublicInstance | null) => {
    list = instance
  }
  root.render(
    <div style={{ position: "relative", width: 600, height: 400, ...(colored ? { backgroundColor: "#ffffff" } : {}) }}>
      {kind === "div" ? (
        <div ref={ref} style={style as never}>
          {children}
        </div>
      ) : (
        <virtual-list ref={ref} style={style as never}>
          {children}
        </virtual-list>
      )}
    </div>
  )
  root.renderer.dispatchNativeEvents()
  return { root, rows, list: () => list! }
}

function run(list: PublicInstance, op: Op) {
  if (op[0] === "top") list.scrollTop = op[1]
  else if (op[0] === "left") list.scrollLeft = op[1]
  else if (op[0] === "to") list.scrollTo(op[1], op[2])
  else list.scrollBy(op[1], op[2])
}

/** The rows of the step with a device pixel in view in the list, the ones a list builds. */
function inView(scene: Scene, step: Scene["steps"][number], kind: Kind, scale: Scale): number[] {
  if (kind === "div") return step.rows.map((_, index) => index)
  const top = scene.list[1]
  const bottom = scene.list[1] + scene.list[3]
  return step.rows.flatMap(([, y, , height], index) => (Math.min(y + height, bottom) - Math.max(y, top) >= 1 / scale ? [index] : []))
}

const KINDS: Kind[] = ["div", "virtual-list"]

describeNative("a fractional scroll offset against Chrome", () => {
  for (const scale of SCALES) {
    for (const [name, scene] of Object.entries(ORACLE[scale])) {
      for (const kind of KINDS) {
        if (kind === "virtual-list" && scene.spec.axes !== "y") continue
        it(`${name} as a ${kind} at scale ${scale}`, () => {
          const mounted = mount(scene, scale, kind)
          try {
            const list = mounted.list()
            expect(rectOf(list), "the box of the list").toEqual(scene.list)
            expect([list.scrollWidth, list.scrollHeight, list.clientWidth, list.clientHeight], "scroll sizes").toEqual([
              scene.scrollWidth,
              scene.scrollHeight,
              scene.clientWidth,
              scene.clientHeight,
            ])
            for (const step of scene.steps) {
              const label = JSON.stringify(step.op)
              run(list, step.op)
              mounted.root.renderer.dispatchNativeEvents()
              expect([list.scrollLeft, list.scrollTop], `scrollLeft and scrollTop after ${label}`).toEqual([step.scrollLeft, step.scrollTop])
              expect(mounted.root.renderer.getScrollOffset(list.id), `getScrollOffset after ${label}`).toEqual([step.scrollLeft, step.scrollTop])
              for (const index of inView(scene, step, kind, scale)) {
                expect(rectOf(mounted.rows[index]!), `row ${index} after ${label}`).toEqual(step.rows[index])
              }
            }
          } finally {
            mounted.root.unmount()
          }
        })
      }
    }
  }
})

/** Where the rows paint: a row covers the device pixel rows from its top to its bottom, each rounded half up as Chrome
 *  snaps the edges of a box, inside the list. */
function expectedRuns(scene: Scene, step: Scene["steps"][number], scale: Scale): string[] {
  const snap = (value: number) => Math.floor(value * scale + 0.5)
  const colors: string[] = []
  for (let y = snap(scene.list[1]); y < snap(scene.list[1] + scene.list[3]); y++) {
    const row = step.rows.findIndex(([, top, , height]) => snap(top) <= y && y < snap(top + height))
    colors.push(row < 0 ? "white" : row % 2 === 0 ? "red" : "blue")
  }
  return colors
}

describeNative("the rows of a scroller paint where a fractional offset reads back", () => {
  for (const scale of SCALES) {
    for (const [name, scene] of Object.entries(ORACLE[scale])) {
      // The scenes with a border or a padding clip inside the border box.
      if (/border|padding/.test(scene.spec.list)) continue
      for (const kind of KINDS) {
        if (kind === "virtual-list" && scene.spec.axes !== "y") continue
        it(`${name} as a ${kind} at scale ${scale}`, () => {
          const mounted = mount(scene, scale, kind, true)
          try {
            const list = mounted.list()
            for (const [at, step] of scene.steps.entries()) {
              run(list, step.op)
              mounted.root.renderer.dispatchNativeEvents()
              const file = path.join(SHOTS_DIR, `scroll-fraction-${name.replace(/\W+/g, "-")}-${kind}-x${scale}-${at}.png`)
              mounted.root.renderer.captureScreenshot(file)
              const pixels = readPixels(file)
              const column = Math.floor((scene.list[0] + scene.list[2] / 2) * scale)
              const clipTop = Math.floor(scene.list[1] * scale + 0.5)
              const painted = expectedRuns(scene, step, scale).map((_, row) => {
                const [r, g, b] = pixels.at(column, clipTop + row)
                return r > 200 && g < 60 && b < 60 ? "red" : b > 200 && r < 60 && g < 60 ? "blue" : r > 240 && g > 240 && b > 240 ? "white" : `#${[r, g, b].join(",")}`
              })
              expect(painted, `device pixel rows after ${JSON.stringify(step.op)}`).toEqual(expectedRuns(scene, step, scale))
            }
          } finally {
            mounted.root.unmount()
          }
        })
      }
    }
  }
})
