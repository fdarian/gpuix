/// `@container` blocks that switch the `display` of an element between `block`, `inline` and the
/// atomic inline values: the paragraph planner reads the display the block gives, in the frame that
/// lays the container out, in both directions.
///
/// `fixtures/inline-container-display-oracle.html` is the page and
/// `fixtures/inline-container-display-oracle.json` the boxes headless Chrome reports for it
/// (`getBoundingClientRect` relative to the box of the case, `[x, y, width, height]`; regenerate
/// with the command in the HTML). The text is GpuixTestSquare at 20px with a line height of 20px, so
/// every character is 20px wide, and only "A" is in the font. The query container is the line
/// itself, at 300px where `(min-width: 250px)` matches and at 200px where it does not.

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

const ORACLE: Record<string, Rect> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/inline-container-display-oracle.json"),
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
  /** The boxes to compare at a width. */
  names: (width: number) => string[]
}

const line = (ref: Names, width: number, children: ReactElement) => (
  <div ref={ref("root")} style={{ width, containerType: "inline-size", ...TEXT }}>
    {children}
  </div>
)

const CASES: Case[] = [
  {
    scope: "a",
    names: () => ["t1", "box", "t2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        <>
          <span ref={ref("t1")}>A</span>
          <div
            ref={ref("box")}
            style={{ display: "block", width: 30, height: 10, "@container": { "(min-width: 250px)": { display: "inline-block" } } }}
          />
          <span ref={ref("t2")}>A</span>
        </>
      ),
  },
  {
    scope: "b",
    names: () => ["t1", "box", "t2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        <>
          <span ref={ref("t1")}>A</span>
          <div ref={ref("box")} style={{ display: "block", "@container": { "(min-width: 250px)": { display: "inline" } } }}>
            AA
          </div>
          <span ref={ref("t2")}>A</span>
        </>
      ),
  },
  {
    scope: "c",
    names: () => ["t1", "box", "t2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        <>
          <span ref={ref("t1")}>A</span>
          <div
            ref={ref("box")}
            style={{ display: "inline-block", width: 30, height: 10, "@container": { "(max-width: 250px)": { display: "block" } } }}
          />
          <span ref={ref("t2")}>A</span>
        </>
      ),
  },
  {
    scope: "d",
    names: () => ["t1", "box", "item1", "item2", "t2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        <>
          <span ref={ref("t1")}>A</span>
          <div ref={ref("box")} style={{ display: "block", "@container": { "(min-width: 250px)": { display: "inline-flex" } } }}>
            <div ref={ref("item1")} style={{ width: 20, height: 10 }} />
            <div ref={ref("item2")} style={{ width: 20, height: 15 }} />
          </div>
          <span ref={ref("t2")}>A</span>
        </>
      ),
  },
  {
    scope: "e",
    names: () => ["t1", "box", "t2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        <>
          <span ref={ref("t1")}>A</span>
          <span ref={ref("box")} style={{ "@container": { "(min-width: 250px)": { display: "block" } } }}>
            AA
          </span>
          <span ref={ref("t2")}>A</span>
        </>
      ),
  },
  {
    scope: "f",
    // Chrome reports no useful box for the one that is hidden.
    names: (width) => (width === 300 ? ["span", "box"] : ["span"]),
    app: (ref, width) =>
      line(
        ref,
        width,
        <span ref={ref("span")}>
          A
          <div
            ref={ref("box")}
            style={{ display: "none", width: 30, height: 10, "@container": { "(min-width: 250px)": { display: "inline-block" } } }}
          />
          A
        </span>
      ),
  },
  {
    scope: "g",
    names: () => ["t1", "box", "t2"],
    app: (ref, width) =>
      line(
        ref,
        width,
        <>
          <span ref={ref("t1")}>A</span>
          <div
            ref={ref("box")}
            style={{ display: "inline", "@container": { "(min-width: 250px)": { display: "inline-block", width: 50 } } }}
          >
            A
          </div>
          <span ref={ref("t2")}>A</span>
        </>
      ),
  },
]

describeNative("an @container block that switches display", () => {
  for (const { scope, app, names } of CASES) {
    it(`${scope}: is laid out in the first frame, wide and narrow`, () => {
      for (const width of [300, 200]) {
        const { render, ref, expectChrome, paintedFrames, unmount } = setup()
        try {
          expect(paintedFrames(() => render(app(ref, width)))).toBe(1)
          expectChrome(`${scope}-${width}`, "root", ...names(width))
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
        expectChrome(`${scope}-200`, "root", ...names(200))
        expect(paintedFrames(() => render(app(ref, 300)))).toBe(1)
        expectChrome(`${scope}-300`, "root", ...names(300))
      } finally {
        unmount()
      }
    })
  }
})
