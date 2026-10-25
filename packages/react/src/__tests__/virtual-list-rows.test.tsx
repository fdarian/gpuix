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
/// (the `scrollTop` it reads back is the device pixel it scrolled to, as a div's is: `scroll-fraction.test.tsx`).
///
/// A row is compared when at least a device pixel of it is in view in the padding box of the list, the only rows a list
/// builds: a row that shows only in the top padding of a scrolled list is one of them. A list is a scroller whose client
/// box is its padding box, so its border insets its rows and its `clientWidth`, `clientHeight` and `scrollHeight`.

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
  spec: { parent: string | null; list: string; rows: Row[]; scrolls: number[] }
  list: Rect
  scrollHeight: number
  clientWidth: number
  clientHeight: number
  /** The width of the border on each side, top first. */
  border: [number, number, number, number]
  steps: { scroll: number; scrollTop: number; rows: Rect[]; probes?: Probe[] }[]
}
/** A point near a corner of the padding box and what `elementFromPoint` finds there: a row's index, the list or nothing of it. */
interface Probe {
  x: number
  y: number
  hit: number | "list" | "outside"
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

/** What the last pointer move was aimed at: a row's index, the list, or nothing of it. */
interface Aim {
  target: number | "list" | null
}

function mount(scene: Scene, scale: Scale, colored = false, aim?: Aim) {
  const aimAt = (target: number | "list") =>
    aim ? { onPointerMove: (event: { target: unknown; currentTarget: unknown }) => { if (event.target === event.currentTarget) aim.target = target } } : {}
  const root = createTestRoot({ width: 600, height: 400, scaleFactor: scale })
  const rows: (PublicInstance | null)[] = []
  let list: PublicInstance | null = null
  const body = (
    <virtual-list
      ref={(instance: PublicInstance | null) => {
        list = instance
      }}
      style={{ ...listStyle(scene.spec.list), ...(colored ? { backgroundColor: "#ffffff" } : {}) } as never}
      {...aimAt("list")}
    >
      {scene.spec.rows.map((row, index) => (
        <div
          key={index}
          ref={(instance: PublicInstance | null) => {
            rows[index] = instance
          }}
          style={{ ...styleOf(row.style), ...(colored ? { backgroundColor: index % 2 === 0 ? RED : BLUE } : {}) } as never}
          {...aimAt(index)}
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

/** The rows of the step with a device pixel in view in the padding box, the ones the list builds: a row whose part in
 *  view is thinner paints nothing, and a list that scrolls to a device pixel does not build it. */
function inView(scene: Scene, step: Scene["steps"][number], scale: Scale): number[] {
  const top = scene.list[1] + scene.border[0]
  const bottom = scene.list[1] + scene.list[3] - scene.border[2]
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
          expect(list.scrollHeight, "scrollHeight").toBe(scene.scrollHeight)
          expect(list.clientWidth, "clientWidth").toBe(scene.clientWidth)
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
 *  Chrome snaps the edges of a box, inside the padding box of the list. Rows with backgrounds touch when the layout ones do, so there is no
 *  gap and no overlap between two. */
function expectedRuns(scene: Scene, step: Scene["steps"][number], scale: Scale): string[] {
  const snap = (value: number) => Math.floor(value * scale + 0.5)
  const clip = [snap(scene.list[1] + scene.border[0]), snap(scene.list[1] + scene.list[3] - scene.border[2])] as const
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
            const column = Math.floor((scene.list[0] + scene.list[2] / 2) * scale)
            const clipTop = Math.floor((scene.list[1] + scene.border[0]) * scale + 0.5)
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

/** A list with a `border-radius` clips its rows, painted and hit, by its padding box with the corners rounded by the
 *  radius less the border width on each axis, as a scroller of divs does. The oracle records `elementFromPoint` at points
 *  just inside (0.85 of the corner ellipse) and just outside (1.1) of each corner ellipse of the padding box: the row is
 *  hit inside and what is under it outside. The rows are painted where they are hit. */
describeNative("the rows of a rounded <virtual-list> are clipped at its corners as Chrome clips a scroller of divs", () => {
  for (const scale of SCALES) {
    for (const [name, scene] of Object.entries(ORACLE[scale])) {
      if (!scene.steps[0]!.probes) continue
      it(`${name} at scale ${scale}`, () => {
        const aim: Aim = { target: null }
        const mounted = mount(scene, scale, true, aim)
        try {
          const list = mounted.list()
          for (const step of scene.steps) {
            list.scrollTop = step.scroll
            mounted.root.renderer.dispatchNativeEvents()
            const file = path.join(SHOTS_DIR, `virtual-list-rounded-${name.replace(/\W+/g, "-")}-x${scale}-${step.scroll}.png`)
            mounted.root.renderer.captureScreenshot(file)
            const pixels = readPixels(file)
            const wrong: string[] = []
            for (const probe of step.probes!) {
              // A probe within a pixel of the edge between two rows is Chrome's call on which row, by its own snapping.
              if (step.rows.some(([, y, , height]) => [y, y + height].some((edge) => Math.abs(edge - probe.y) < 1))) continue
              const at = `(${probe.x.toFixed(1)}, ${probe.y.toFixed(1)}) at scrollTop ${step.scroll}`
              aim.target = null
              mounted.root.renderer.nativeSimulateMouseMove(probe.x, probe.y)
              // The border ring is the list's own hit area and its edge is the border box's arc, which a probe 1.1 times
              // out in a thick border lies close to: what is asked here is whether a row is hit.
              const hit = typeof aim.target === "number" ? aim.target : "not a row"
              const want = typeof probe.hit === "number" ? probe.hit : "not a row"
              if (hit !== want) wrong.push(`hit ${at}: got ${hit} Chrome ${want}`)

              const [r, g, b] = pixels.at(Math.floor(probe.x * scale), Math.floor(probe.y * scale))
              const painted = r > 200 && g < 60 && b < 60 ? "red" : b > 200 && r < 60 && g < 60 ? "blue" : "other"
              const paintedWant = typeof probe.hit === "number" ? (probe.hit % 2 === 0 ? "red" : "blue") : "other"
              if (painted !== paintedWant) wrong.push(`paint ${at}: got ${painted} Chrome ${paintedWant}`)
            }
            expect(wrong).toEqual([])
          }
        } finally {
          mounted.root.unmount()
        }
      })
    }
  }
})
