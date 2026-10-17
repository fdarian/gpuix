/// `position: relative` percentage offsets on an `inline-block`: `left` and `right` are of the width
/// of the containing block, `top` and `bottom` of its height when that is definite and `auto`
/// otherwise.
///
/// `fixtures/inline-relative-oracle.html` is the page and `fixtures/inline-relative-oracle.json`
/// the boxes headless Chrome reports for it (`getBoundingClientRect` relative to the box of the case,
/// `[x, y, width, height]`; regenerate with the command in the HTML). The text is GpuixTestSquare at
/// 20px with a line height of 20px, so every character is 20px wide, and only "A" is in the font.

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
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/inline-relative-oracle.json"),
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
  /** Each `name` laid out as the oracle's `${scope}.${name}`. */
  const expectChrome = (scope: string, ...names: string[]) => {
    for (const name of names) {
      const expected = chrome(`${scope}.${name}`)
      const actual = rect(name)
      for (const [ix, key] of ["x", "y", "w", "h"].entries()) {
        expect(
          Math.abs((actual[ix] ?? 0) - (expected[ix] ?? 0)),
          `${scope}.${name}: ${key} ${actual[ix]} != ${expected[ix]}`
        ).toBeLessThanOrEqual(0.51)
      }
    }
  }
  return { ...root, ref, rect, expectChrome }
}

type Names = ReturnType<typeof setup>["ref"]

const PADDED = { width: 340, paddingTop: 10, paddingBottom: 10, paddingLeft: 20, paddingRight: 20, ...TEXT } as const

const box = (ref: Names, name: string, offsets: Record<string, number | string>): ReactElement => (
  <span
    ref={ref(name)}
    style={{ display: "inline-block", position: "relative", width: 20, height: 10, ...offsets }}
  />
)

/** `A`, a box with the offsets, `A`: the case `scope` of the oracle. */
const line = (ref: Names, offsets: Record<string, number | string>, style: Record<string, number | string> = {}) => (
  <div ref={ref("root")} style={{ ...PADDED, ...style }}>
    <span ref={ref("t1")}>A</span>
    {box(ref, "box", offsets)}
    <span ref={ref("t2")}>A</span>
  </div>
)

function check(scope: string, element: (ref: Names) => ReactElement) {
  const { render, ref, expectChrome, unmount } = setup()
  try {
    render(element(ref))
    expectChrome(scope, "t1", "box", "t2")
  } finally {
    unmount()
  }
}

describeNative("a percentage offset of a relative inline-block: left and right", () => {
  it("is of the width of the content box of the containing block", () => {
    check("left", (ref) => line(ref, { left: "10%" }))
  })

  it("is negated for right", () => {
    check("right", (ref) => line(ref, { right: "10%" }))
  })

  it("lets left win over right", () => {
    check("leftright", (ref) => line(ref, { left: "10%", right: "25%" }))
  })

  it("takes a percentage in a calc", () => {
    check("calc", (ref) => line(ref, { left: "calc(10% + 4px)" }))
  })

  it("is of the block, not of the span around the box", () => {
    const { render, ref, expectChrome, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={PADDED}>
          <span ref={ref("t1")}>
            A
            <span style={{ paddingLeft: 30, paddingRight: 30 }}>{box(ref, "box", { left: "10%" })}</span>
          </span>
        </div>
      )
      expectChrome("span", "t1", "box")
    } finally {
      unmount()
    }
  })
})

describeNative("a percentage offset of a relative inline-block: top and bottom", () => {
  it("is auto when the containing block has no definite height", () => {
    check("auto-top", (ref) => line(ref, { top: "10%" }))
    check("auto-bottom", (ref) => line(ref, { bottom: "10%" }))
    check("auto-top-calc", (ref) => line(ref, { top: "calc(10% + 5px)" }))
  })

  it("gives way to the opposite inset when it is auto", () => {
    check("auto-top-bottom", (ref) => line(ref, { top: "10%", bottom: 5 }))
  })

  it("is of the height of the content box of the containing block when it has a height", () => {
    check("top", (ref) => line(ref, { top: "10%" }, { height: 200 }))
    check("bottom", (ref) => line(ref, { bottom: "25%" }, { height: 200 }))
    check("top-calc", (ref) => line(ref, { top: "calc(10% + 5px)", left: "10%" }, { height: 200 }))
  })

  it("is of a block whose height is a percentage that resolves, and auto when its height is auto", () => {
    for (const [scope, hostHeight] of [
      ["percent-parent", "50%"],
      ["auto-in-definite", undefined],
    ] as const) {
      const { render, ref, expectChrome, unmount } = setup()
      try {
        render(
          <div ref={ref("root")} style={{ ...PADDED, height: 300 }}>
            <div style={hostHeight ? { height: hostHeight } : {}}>
              <span ref={ref("t1")}>A</span>
              {box(ref, "box", { top: "10%" })}
              <span ref={ref("t2")}>A</span>
            </div>
          </div>
        )
        expectChrome(scope, "t1", "box", "t2")
      } finally {
        unmount()
      }
    }
  })

  it("is of the height a column flex container gives its item", () => {
    const { render, ref, expectChrome, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ ...PADDED, height: 300, display: "flex", flexDirection: "column" }}>
          <div>
            <span ref={ref("t1")}>A</span>
            {box(ref, "box", { top: "10%" })}
            <span ref={ref("t2")}>A</span>
          </div>
        </div>
      )
      expectChrome("stretched", "t1", "box", "t2")
    } finally {
      unmount()
    }
  })

  it("is of the block that holds the line when a block sits before it", () => {
    const { render, ref, expectChrome, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ ...PADDED, height: 200 }}>
          <div style={{ height: 10 }} />
          <span ref={ref("t1")}>A</span>
          {box(ref, "box", { top: "10%" })}
          <span ref={ref("t2")}>A</span>
        </div>
      )
      expectChrome("mixed", "t1", "box", "t2")
    } finally {
      unmount()
    }
  })

  it("follows the height of the block when it changes", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      const app = (height: number) => line(ref, { top: "10%" }, { height })
      render(app(200))
      expect(rect("box")[1]).toBeCloseTo(chrome("top.box")[1], 0)
      render(app(100))
      // 10% of the 80px content box of a 100px block, on the line at 20.
      expect(rect("box")[1]).toBeCloseTo(28, 0)
    } finally {
      unmount()
    }
  })
})
