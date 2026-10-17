/// `@container` blocks that switch the `display` of an element to or from a table value: the table
/// role and the anonymous wrapping of the table (cells for what is loose in a table or a row, a table
/// for the loose parts in a block) follow the display the block gives, in the frame that lays the
/// container out, in both directions.
///
/// `fixtures/table-container-display-oracle.html` is the page and
/// `fixtures/table-container-display-oracle.json` the boxes headless Chrome reports for it
/// (`getBoundingClientRect` relative to the box of the case, `[x, y, width, height]`; regenerate with
/// the command in the HTML). The boxes are fixed-size divs. The query container is the line itself, at
/// 300px where `(min-width: 250px)` matches and at 200px where it does not.

import fs from "fs"
import path from "path"
import type { ReactElement } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

type Rect = [number, number, number, number]
type Style = Record<string, unknown>

const ORACLE: Record<string, Rect> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/table-container-display-oracle.json"),
    "utf8"
  )
)

function chrome(id: string): Rect {
  const box = ORACLE[id]
  expect(box, `no oracle box ${id}`).toBeDefined()
  return box ?? [0, 0, 0, 0]
}

/** The names the elements of a render are found by; boxes are relative to the one called `root`. */
function setup() {
  const root = createTestRoot()
  root.renderer.registerFont(SQUARE_FONT)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string): Rect => {
    root.renderer.dispatchNativeEvents()
    const element = refs.get(name)
    expect(element, `no element ${name}`).toBeDefined()
    const box = element?.getBoundingClientRect() ?? { x: 0, y: 0, width: 0, height: 0 }
    const origin = refs.get("root")?.getBoundingClientRect() ?? { x: 0, y: 0 }
    return [box.x - origin.x, box.y - origin.y, box.width, box.height]
  }
  /** Each `name` laid out as the oracle's `${scope}.${name}`; `root` is the oracle's `scope`. */
  const expectChrome = (scope: string, ...names: string[]) => {
    for (const name of names) {
      const expected = chrome(name === "root" ? scope : `${scope}.${name}`)
      const actual = rect(name)
      for (const [ix, key] of ["x", "y", "w", "h"].entries()) {
        expect(
          Math.abs((actual[ix] ?? 0) - (expected[ix] ?? 0)),
          `${scope}.${name}: ${key} ${actual[ix]} != ${expected[ix]}`
        ).toBeLessThanOrEqual(0.51)
      }
    }
  }
  /** Frames the window painted while `change` ran. */
  const paintedFrames = (change: () => void): number => {
    const before = root.renderer.getPaintedFrameCount()
    change()
    return root.renderer.getPaintedFrameCount() - before
  }
  return { ...root, ref, rect, expectChrome, paintedFrames }
}

type Names = ReturnType<typeof setup>["ref"]

type Case = {
  /** The oracle's name of the case: `${scope}-300` and `${scope}-200`. */
  scope: string
  app: (ref: Names, width: number) => ReactElement
  names: string[]
}

const line = (ref: Names, width: number, children: ReactElement) => (
  <div ref={ref("root")} style={{ width, containerType: "inline-size", ...TEXT }}>
    {children}
  </div>
)

const WIDE = "(min-width: 250px)"
const NARROW = "(max-width: 250px)"

/** A fixed-size box with the `display` it has and, in a container that matches `query`, the one `to` gives it. */
const part = (
  ref: Names,
  name: string,
  display: string,
  query: string | undefined,
  to: string | undefined,
  size?: [number, number],
  children?: ReactElement | ReactElement[]
) => {
  const style: Style = { display }
  if (size) {
    style.width = size[0]
    style.height = size[1]
  }
  if (query && to) style["@container"] = { [query]: { display: to } }
  return (
    <div ref={ref(name)} style={style}>
      {children}
    </div>
  )
}

const CASES: Case[] = [
  {
    // The blocks in a row share one anonymous cell, until they are cells.
    scope: "a",
    names: ["row", "c1", "c2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        part(ref, "row", "table-row", undefined, undefined, undefined, [
          part(ref, "c1", "block", WIDE, "table-cell", [30, 10]),
          part(ref, "c2", "block", WIDE, "table-cell", [20, 20]),
        ])
      ),
  },
  {
    // The container itself becomes a table, and what it holds cells.
    scope: "b",
    names: ["table", "c1", "c2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        part(ref, "table", "block", WIDE, "table", undefined, [
          part(ref, "c1", "block", WIDE, "table-cell", [30, 10]),
          part(ref, "c2", "block", WIDE, "table-cell", [20, 20]),
        ])
      ),
  },
  {
    // A row that is a block: the cells in it are loose parts in an anonymous table.
    scope: "c",
    names: ["table", "row", "c1", "c2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        part(ref, "table", "table", undefined, undefined, undefined, [
          part(ref, "row", "block", WIDE, "table-row", undefined, [
            part(ref, "c1", "table-cell", undefined, undefined, [30, 10]),
            part(ref, "c2", "table-cell", undefined, undefined, [20, 20]),
          ]),
        ])
      ),
  },
  {
    // A block in a table, which is a caption above the cell in a wide container.
    scope: "d",
    names: ["table", "cell", "caption"],
    app: (ref, width) =>
      line(
        ref,
        width,
        part(ref, "table", "table", undefined, undefined, undefined, [
          part(ref, "cell", "table-cell", undefined, undefined, [30, 20]),
          <div key="caption" ref={ref("caption")} style={{ display: "block", height: 10, "@container": { [WIDE]: { display: "table-caption" } } }} />,
        ])
      ),
  },
  {
    // A table that is a block in a narrow container.
    scope: "e",
    names: ["table", "row", "c1", "c2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        part(ref, "table", "table", NARROW, "block", undefined, [
          part(ref, "row", "table-row", undefined, undefined, undefined, [
            part(ref, "c1", "table-cell", undefined, undefined, [30, 10]),
            part(ref, "c2", "table-cell", undefined, undefined, [20, 20]),
          ]),
        ])
      ),
  },
  {
    // A block that is an inline-table on a line of text in a wide container.
    scope: "f",
    names: ["t1", "table", "row", "c1", "t2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        <>
          <span ref={ref("t1")}>AA</span>
          {part(ref, "table", "block", WIDE, "inline-table", undefined, [
            part(ref, "row", "table-row", undefined, undefined, undefined, [
              part(ref, "c1", "table-cell", undefined, undefined, [30, 30]),
            ]),
          ])}
          <span ref={ref("t2")}>AA</span>
        </>
      ),
  },
  {
    // A block that is a row group in a table in a wide container.
    scope: "g",
    names: ["table", "group", "row", "c1", "c2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        part(ref, "table", "table", undefined, undefined, undefined, [
          part(ref, "group", "block", WIDE, "table-row-group", undefined, [
            part(ref, "row", "table-row", undefined, undefined, undefined, [
              part(ref, "c1", "table-cell", undefined, undefined, [30, 10]),
              part(ref, "c2", "table-cell", undefined, undefined, [20, 20]),
            ]),
          ]),
        ])
      ),
  },
  {
    // A cell that is a block in a narrow container.
    scope: "h",
    names: ["table", "row", "c1", "c2", "c3"],
    app: (ref, width) =>
      line(
        ref,
        width,
        part(ref, "table", "table", undefined, undefined, undefined, [
          part(ref, "row", "table-row", undefined, undefined, undefined, [
            part(ref, "c1", "table-cell", undefined, undefined, [30, 10]),
            part(ref, "c2", "table-cell", NARROW, "block", [20, 20]),
            part(ref, "c3", "table-cell", undefined, undefined, [10, 30]),
          ]),
        ])
      ),
  },
]

describeNative("an @container block that switches display to or from a table value", () => {
  for (const { scope, app, names } of CASES) {
    it(`${scope}: is laid out in the first frame, wide and narrow`, () => {
      for (const width of [300, 200]) {
        const { render, ref, expectChrome, paintedFrames, unmount } = setup()
        try {
          expect(paintedFrames(() => render(app(ref, width)))).toBe(1)
          expectChrome(`${scope}-${width}`, "root", ...names)
        } finally {
          unmount()
        }
      }
    })

    it(`${scope}: flips both ways in the frame that resizes the container`, () => {
      const { render, ref, expectChrome, paintedFrames, unmount } = setup()
      try {
        render(app(ref, 300))
        expect(paintedFrames(() => render(app(ref, 200)))).toBe(1)
        expectChrome(`${scope}-200`, "root", ...names)
        expect(paintedFrames(() => render(app(ref, 300)))).toBe(1)
        expectChrome(`${scope}-300`, "root", ...names)
      } finally {
        unmount()
      }
    })
  }
})
