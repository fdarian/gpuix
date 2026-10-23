/// The rows and the box of a `<virtual-list>` against a scroller of divs in Chrome (round 15c): a row is measured in
/// 1/64 of a device pixel and not on the device pixel (a row of `20.3px` is `20.296875` tall at scale 1, `20.3` is
/// `20.296875` at scale 2 too, and a line of 13px text at 1.15 is `14.9375`), the rows stack from the list's own
/// unsnapped origin, and the list records its box like any element, so its `getBoundingClientRect` is the box layout
/// gave it, not zeros.
///
/// `fixtures/virtual-list-oracle.html` is the page, `fixtures/virtual-list-oracle.json` (device scale 1) and
/// `fixtures/virtual-list-oracle-x2.json` (scale 2) what headless Chrome reports for it (regenerate with
/// `fixtures/dsf-oracle.sh virtual-list`). Each scene is a scroller with rows of fractional heights, set to a series of
/// scroll offsets; the page records its box, its `scrollHeight` and `clientHeight`, and for each offset the `scrollTop`
/// Chrome scrolled to and the box of every row. The same scene is rendered here with a `<virtual-list>` and compared
/// exactly. Chrome scrolls to a device pixel (a `scrollTop` of 10.5 is 11 at scale 1, 10.3 is 10.5 at scale 2), and so does the list
/// when it places its rows (the `scrollTop` it reads back keeps the fraction that was written, as a div's does).
///
/// A row is compared when at least a device pixel of it is in view, the only rows a list builds. A list ignores its own
/// border when it places and sizes its rows, and its `clientHeight` and `scrollHeight` are those of the box with its
/// border, so the scenes with a border compare the box of the list only (a deviation, FORK.md).

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
type Row = { style: string; text?: string }
interface Scene {
  spec: { parent: string | null; list: string; rows: Row[]; compareRows: boolean; scrolls: number[] }
  list: Rect
  scrollHeight: number
  clientHeight: number
  steps: { scroll: number; scrollTop: number; rows: Rect[] }[]
}

const SCALES = [1, 2] as const
type Scale = (typeof SCALES)[number]

const oracleOf = (file: string): Record<string, Scene> =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLE: Record<Scale, Record<string, Scene>> = {
  1: oracleOf("virtual-list-oracle.json"),
  2: oracleOf("virtual-list-oracle-x2.json"),
}

/** The style of a scroller of the page as the style of a list: a list scrolls along y only and `overflow` is not its to say. */
function listStyle(css: string) {
  const style: Record<string, unknown> = styleOf(css)
  delete style.overflow
  return style
}

const rectOf = (instance: PublicInstance): Rect => {
  const box = instance.getBoundingClientRect()
  return [box.x, box.y, box.width, box.height]
}

const RED = "#ff0000"
const BLUE = "#0000ff"

function mount(scene: Scene, scale: Scale, colored = false) {
  const root = createTestRoot({ width: 600, height: 400, scaleFactor: scale })
  const rows: (PublicInstance | null)[] = []
  let list: PublicInstance | null = null
  const body = (
    <virtual-list
      ref={(instance: PublicInstance | null) => {
        list = instance
      }}
      style={{ ...listStyle(scene.spec.list), ...(colored ? { backgroundColor: "#ffffff" } : {}) } as never}
    >
      {scene.spec.rows.map((row, index) => (
        <div
          key={index}
          ref={(instance: PublicInstance | null) => {
            rows[index] = instance
          }}
          style={{ ...styleOf(row.style), ...(colored ? { backgroundColor: index % 2 === 0 ? RED : BLUE } : {}) } as never}
        >
          {row.text}
        </div>
      ))}
    </virtual-list>
  )
  root.render(
    <div style={{ position: "relative", width: 600, height: 400, ...(colored ? { backgroundColor: "#ffffff" } : {}) }}>
      {scene.spec.parent === null ? body : <div style={styleOf(scene.spec.parent) as never}>{body}</div>}
    </div>
  )
  root.renderer.dispatchNativeEvents()
  return { root, rows, list: () => list! }
}

/** The rows of the step with a device pixel in view in the client box, the ones the list builds: a row whose part in
 *  view is thinner paints nothing, and a list that scrolls to a device pixel does not build it. */
function inView(scene: Scene, step: Scene["steps"][number], scale: Scale): number[] {
  const [, top] = scene.list
  const bottom = top + scene.clientHeight
  return step.rows.flatMap(([, y, , height], index) => (Math.min(y + height, bottom) - Math.max(y, top) >= 1 / scale ? [index] : []))
}

describeNative("a <virtual-list> against Chrome", () => {
  for (const scale of SCALES) {
    for (const [name, scene] of Object.entries(ORACLE[scale])) {
      it(`${name} at scale ${scale}`, () => {
        const mounted = mount(scene, scale)
        try {
          const list = mounted.list()
          expect(rectOf(list), "the box of the list").toEqual(scene.list)
          if (!scene.spec.compareRows) return
          expect(list.scrollHeight, "scrollHeight").toBe(scene.scrollHeight)
          expect(list.clientHeight, "clientHeight").toBe(scene.clientHeight)
          for (const step of scene.steps) {
            list.scrollTop = step.scroll
            mounted.root.renderer.dispatchNativeEvents()
            expect(rectOf(list), `the box of the list scrolled to ${step.scroll}`).toEqual(scene.list)
            const rows = inView(scene, step, scale)
            expect(rows.length, `rows in view at ${step.scroll}`).toBeGreaterThan(0)
            for (const index of rows) {
              expect(rectOf(mounted.rows[index]!), `row ${index} at scrollTop ${step.scroll} (Chrome ${step.scrollTop})`).toEqual(
                step.rows[index]
              )
            }
          }
        } finally {
          mounted.root.unmount()
        }
      })
    }
  }
})

/** Where the rows of a list paint: a row covers the device pixel rows from its top to its bottom, each rounded half up as
 *  Chrome snaps the edges of a box, inside the list. Rows with backgrounds touch when the layout ones do, so there is no
 *  gap and no overlap between two. */
function expectedRuns(scene: Scene, step: Scene["steps"][number], scale: Scale): string[] {
  const snap = (value: number) => Math.floor(value * scale + 0.5)
  const [, listTop, , listHeight] = [scene.list[0], scene.list[1], scene.list[2], scene.list[3]]
  const clip = [snap(listTop), snap(listTop + listHeight)] as const
  const colors: string[] = []
  for (let y = clip[0]; y < clip[1]; y++) {
    const row = step.rows.findIndex(([, top, , height]) => snap(top) <= y && y < snap(top + height))
    colors.push(row < 0 ? "white" : row % 2 === 0 ? "red" : "blue")
  }
  return colors
}

describeNative("the rows of a <virtual-list> paint as Chrome paints a scroller of divs", () => {
  for (const scale of SCALES) {
    for (const [name, scene] of Object.entries(ORACLE[scale])) {
      if (!scene.spec.compareRows) continue
      it(`${name} at scale ${scale}`, () => {
        const mounted = mount(scene, scale, true)
        try {
          const list = mounted.list()
          for (const step of scene.steps) {
            list.scrollTop = step.scroll
            mounted.root.renderer.dispatchNativeEvents()
            const file = path.join(SHOTS_DIR, `virtual-list-rows-${name.replace(/\W+/g, "-")}-x${scale}-${step.scroll}.png`)
            mounted.root.renderer.captureScreenshot(file)
            const pixels = readPixels(file)
            const [x, listTop] = [scene.list[0] + scene.list[2] / 2, scene.list[1]]
            const column = Math.floor(x * scale)
            const clipTop = Math.floor(listTop * scale + 0.5)
            const painted = expectedRuns(scene, step, scale).map((_, at) => {
              const [r, g, b] = pixels.at(column, clipTop + at)
              return r > 200 && g < 60 && b < 60 ? "red" : b > 200 && r < 60 && g < 60 ? "blue" : r > 240 && g > 240 && b > 240 ? "white" : `#${[r, g, b].join(",")}`
            })
            expect(painted, `device pixel rows at scrollTop ${step.scroll}`).toEqual(expectedRuns(scene, step, scale))
          }
        } finally {
          mounted.root.unmount()
        }
      })
    }
  }
})
