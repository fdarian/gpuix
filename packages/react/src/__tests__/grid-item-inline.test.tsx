/// Inline content in a grid or flex item. CSS Grid 6.1 blockifies the in-flow children of the container
/// itself, and wraps each contiguous run of text in an anonymous item; whatever is inside an item (a
/// `<div>`, a `<text>`, a `<span>`, which blockify as items) is an ordinary inline formatting context, so
/// a `<span>` there stays on the line with its text and its weight moves only what follows it.
/// Numbers are from headless Chrome (chrome-headless-shell). The square font is GpuixTestSquare at 20px/20px:
/// 20px a letter (no spaces: the fallback glyph of a space differs). Helvetica Neue is the system face, so only x and width are compared.
/// HTML, root = `<div id=root style="width:400px;font:20px/20px GpuixTestSquare;color:#000;display:grid|flex">`:
///   item   `<div id=item>AA<span id=s style="font-weight:W">AAA</span>AA</div>`
///   direct `AA<span id=s style="font-weight:W">AAA</span><span id=t>AA</span>A`
///   nested `<span id=item>AA<span id=s ...>AAA</span>AA</span>`
/// Chrome, W = 400 and 700 alike: item -> grid item 400x20 at 0,0, s at (40,0) 60x20; in flex the item is 140x20;
/// direct -> root 400x80, s at (0,20) 400x20, t at (0,40) 400x20; nested -> item 400x20, s at (40,0) 60x20.
///
/// The stable label of the design system: an `inline-grid` span holding two spans in the same cell, a hidden one
/// that reserves the bold width and the visible one whose weight changes.
///   lab    `<span id=g style="display:inline-grid;white-space:nowrap"><span id=h style="grid-column:1;grid-row:1;
///           visibility:hidden">AAAA</span><span id=v style="grid-column:1;grid-row:1">AAA</span></span>`
///   text   `<div id=item><span id=pre>AA</span>${lab}<span id=post>AA</span></div>`, flexrow `<div id=pre>AA</div>${lab}
///           <div id=post>AA</div>` (flex, align-items:center), flexgrow = lab with flex-grow:1 then `<div id=post>AA</div>`.
/// Chrome: text -> pre (0,0) 40x20, g/h/v (40,0) 80x20, post (120,0) 40x20; flexrow the same; flexgrow -> g/h/v (0,0)
/// 360x20, post (360,0); in a grid item the item is 400x20 and the rest as text.
/// With `font:20px/24px "Helvetica Neue"`, root 400 wide, `<div id=item>before <lab 400|600/> after</div>` (hidden copy
/// weight 600, "Label"): g (62.6,0) 52.2x24 at both weights; as a flex row `<div>before</div><lab/><div>after</div>`:
/// g x 57, 52.2 wide, post x 109.3. Gpuix rounds a shrink-to-fit text width up to a pixel, so those compare at 1px.
/// With `font:20px/24px "Helvetica Neue"` and the first shape: s is x 51.1, 51.8 wide at W=400 and 55.1 wide at
/// W=700; in flex the item is 134.4 wide at 400 and 137.7 at 700.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const SQUARE = { width: 400, fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const
const HELVETICA = { width: 400, fontFamily: "Helvetica Neue", fontSize: 20, lineHeight: "24px", color: "#000000" } as const

type Rect = { x: number; y: number; w: number; h: number }

function setup() {
  const root = createTestRoot()
  root.renderer.registerFont(SQUARE_FONT)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string): Rect => {
    const element = refs.get(name)
    expect(element, `no element ${name}`).toBeDefined()
    const box = element!.getBoundingClientRect()
    const origin = refs.get("root")!.getBoundingClientRect()
    return { x: box.x - origin.x, y: box.y - origin.y, w: box.width, h: box.height }
  }
  return { ...root, ref, rect }
}

function expectRect(actual: Rect, expected: Rect, context: string, tolerance = 0.51) {
  for (const key of ["x", "y", "w", "h"] as const) {
    expect(Math.abs(actual[key] - expected[key]), `${context}: ${key} ${actual[key]} != ${expected[key]}`).toBeLessThanOrEqual(
      tolerance
    )
  }
}

describeNative("inline content in a grid or flex item", () => {
  for (const weight of [400, 700]) {
    it(`a span in a div item stays on the line, weight ${weight}`, () => {
      const { render, ref, rect, unmount } = setup()
      try {
        render(
          <div ref={ref("root")} style={{ display: "grid", ...SQUARE }}>
            <div ref={ref("item")}>
              AA<span ref={ref("s")} style={{ fontWeight: weight }}>AAA</span>AA
            </div>
          </div>
        )
        expectRect(rect("root"), { x: 0, y: 0, w: 400, h: 20 }, "root")
        expectRect(rect("item"), { x: 0, y: 0, w: 400, h: 20 }, "item")
        expectRect(rect("s"), { x: 40, y: 0, w: 60, h: 20 }, "s")
      } finally {
        unmount()
      }
    })

    it(`a span in a flex item stays on the line, weight ${weight}`, () => {
      const { render, ref, rect, unmount } = setup()
      try {
        render(
          <div ref={ref("root")} style={{ display: "flex", ...SQUARE }}>
            <div ref={ref("item")}>
              AA<span ref={ref("s")} style={{ fontWeight: weight }}>AAA</span>AA
            </div>
          </div>
        )
        // A shrink-to-fit text width is rounded up to a whole pixel and a bit, so it is 141 here.
        expectRect(rect("item"), { x: 0, y: 0, w: 140, h: 20 }, "item", 1.01)
        expectRect(rect("s"), { x: 40, y: 0, w: 60, h: 20 }, "s")
      } finally {
        unmount()
      }
    })

    it(`a span in a span item stays on the line, weight ${weight}`, () => {
      const { render, ref, rect, unmount } = setup()
      try {
        render(
          <div ref={ref("root")} style={{ display: "grid", ...SQUARE }}>
            <span ref={ref("item")}>
              AA<span ref={ref("s")} style={{ fontWeight: weight }}>AAA</span>AA
            </span>
          </div>
        )
        expectRect(rect("item"), { x: 0, y: 0, w: 400, h: 20 }, "item")
        expectRect(rect("s"), { x: 40, y: 0, w: 60, h: 20 }, "s")
      } finally {
        unmount()
      }
    })

    it(`text and spans that are children of the grid itself are one item each, weight ${weight}`, () => {
      const { render, ref, rect, unmount } = setup()
      try {
        render(
          <div ref={ref("root")} style={{ display: "grid", ...SQUARE }}>
            AA<span ref={ref("s")} style={{ fontWeight: weight }}>AAA</span>
            <span ref={ref("t")}>AA</span>A
          </div>
        )
        expectRect(rect("root"), { x: 0, y: 0, w: 400, h: 80 }, "root")
        expectRect(rect("s"), { x: 0, y: 20, w: 400, h: 20 }, "s")
        expectRect(rect("t"), { x: 0, y: 40, w: 400, h: 20 }, "t")
      } finally {
        unmount()
      }
    })
  }

  it("a weight change moves the text after the span as it does in Chrome", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      const tree = (weight: number) => (
        <div ref={ref("root")} style={{ display: "flex", ...HELVETICA }}>
          <div ref={ref("item")}>
            Hello <span ref={ref("s")} style={{ fontWeight: weight }}>World</span> tail
          </div>
        </div>
      )
      render(tree(400))
      expect(Math.abs(rect("s").x - 51.1)).toBeLessThanOrEqual(0.51)
      expect(Math.abs(rect("s").w - 51.8)).toBeLessThanOrEqual(0.51)
      expect(Math.abs(rect("item").w - 134.4)).toBeLessThanOrEqual(1)
      render(tree(700))
      expect(Math.abs(rect("s").x - 51.1)).toBeLessThanOrEqual(0.51)
      expect(Math.abs(rect("s").w - 55.1)).toBeLessThanOrEqual(0.51)
      expect(Math.abs(rect("item").w - 137.7)).toBeLessThanOrEqual(1)
    } finally {
      unmount()
    }
  })

  it("a hover weight on the span does the same in a grid item", () => {
    const { render, ref, rect, renderer, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ display: "grid", ...HELVETICA }}>
          <div ref={ref("item")}>
            Hello <span ref={ref("s")} style={{ hover: { fontWeight: 700 } }}>World</span> tail
          </div>
        </div>
      )
      expect(Math.abs(rect("s").w - 51.8)).toBeLessThanOrEqual(0.51)
      renderer.nativeSimulateMouseMove(70, 10)
      renderer.flush()
      expect(Math.abs(rect("s").x - 51.1)).toBeLessThanOrEqual(0.51)
      expect(Math.abs(rect("s").w - 55.1)).toBeLessThanOrEqual(0.51)
    } finally {
      unmount()
    }
  })
})

describeNative("the stable label: a weight change does not move what is around an inline-grid", () => {
  const label = (ref: ReturnType<typeof setup>["ref"], weight: number, grid: object = {}) => (
    <span ref={ref("g")} style={{ display: "inline-grid", whiteSpace: "nowrap", ...grid }}>
      <span ref={ref("h")} style={{ gridColumnStart: 1, gridRowStart: 1, visibility: "hidden", fontWeight: 600 }}>
        {WORD}
      </span>
      <span ref={ref("v")} style={{ gridColumnStart: 1, gridRowStart: 1, fontWeight: weight }}>
        {WORD}
      </span>
    </span>
  )
  const WORD = "Label"
  // A shrink-to-fit text width is rounded up to a whole pixel (and 40.000004 from the font's f32 advances to 41),
  // so each item of a row is up to a pixel wider than Chrome's and a row of three drifts by up to 2.
  const near = (actual: number, expected: number, context: string) =>
    expect(Math.abs(actual - expected), `${context}: ${actual} != ${expected}`).toBeLessThanOrEqual(2)

  it("sits on the line between the text before and after it, whatever the weight", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      const tree = (weight: number) => (
        <div ref={ref("root")} style={HELVETICA}>
          <div ref={ref("item")}>
            <span ref={ref("pre")}>before </span>
            {label(ref, weight)}
            <span ref={ref("post")}> after</span>
          </div>
        </div>
      )
      for (const weight of [400, 600, 400]) {
        render(tree(weight))
        near(rect("g").x, 62.6, `g.x at ${weight}`)
        near(rect("g").w, 52.2, `g.w at ${weight}`)
        near(rect("post").x, 114.8, `post.x at ${weight}`)
        expect(rect("g").h).toBe(24)
        expect(rect("h").w).toBe(rect("g").w)
        expect(rect("v").w).toBe(rect("g").w)
      }
    } finally {
      unmount()
    }
  })

  it("is a grid of its two spans in a flex row, and the text around it stays put", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      const tree = (weight: number) => (
        <div ref={ref("root")} style={{ display: "flex", alignItems: "center", ...HELVETICA }}>
          <div ref={ref("pre")}>before</div>
          {label(ref, weight)}
          <div ref={ref("post")}>after</div>
        </div>
      )
      for (const weight of [400, 600]) {
        render(tree(weight))
        near(rect("g").x, 57, `g.x at ${weight}`)
        near(rect("g").w, 52.2, `g.w at ${weight}`)
        near(rect("post").x, 109.3, `post.x at ${weight}`)
      }
    } finally {
      unmount()
    }
  })

  it("in a square font: the cell is as wide as the hidden copy, in a line, a flex row and a grid item", () => {
    for (const container of ["block", "grid"]) {
      const { render, ref, rect, unmount } = setup()
      try {
        render(
          <div ref={ref("root")} style={{ display: container, ...SQUARE }}>
            <div ref={ref("item")}>
              <span ref={ref("pre")}>AA</span>
              <span ref={ref("g")} style={{ display: "inline-grid", whiteSpace: "nowrap" }}>
                <span ref={ref("h")} style={{ gridColumnStart: 1, gridRowStart: 1, visibility: "hidden" }}>AAAA</span>
                <span ref={ref("v")} style={{ gridColumnStart: 1, gridRowStart: 1 }}>AAA</span>
              </span>
              <span ref={ref("post")}>AA</span>
            </div>
          </div>
        )
        expectRect(rect("pre"), { x: 0, y: 0, w: 40, h: 20 }, `${container} pre`, 1.01)
        expectRect(rect("g"), { x: 40, y: 0, w: 80, h: 20 }, `${container} g`, 1.01)
        expectRect(rect("h"), { x: 40, y: 0, w: 80, h: 20 }, `${container} h`, 1.6)
        expectRect(rect("v"), { x: 40, y: 0, w: 80, h: 20 }, `${container} v`, 1.6)
        expectRect(rect("post"), { x: 120, y: 0, w: 40, h: 20 }, `${container} post`, 1.6)
      } finally {
        unmount()
      }
    }
  })

  it("in a square font, as a flex item that grows it takes the free space and its spans stretch", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ display: "flex", ...SQUARE }}>
          <span ref={ref("g")} style={{ display: "inline-grid", flexGrow: 1, textAlign: "left" }}>
            <span ref={ref("h")} style={{ gridColumnStart: 1, gridRowStart: 1, visibility: "hidden" }}>AAAA</span>
            <span ref={ref("v")} style={{ gridColumnStart: 1, gridRowStart: 1 }}>AAA</span>
          </span>
          <div ref={ref("post")}>AA</div>
        </div>
      )
      expectRect(rect("g"), { x: 0, y: 0, w: 360, h: 20 }, "g", 1.6)
      expectRect(rect("v"), { x: 0, y: 0, w: 360, h: 20 }, "v", 1.6)
      expectRect(rect("post"), { x: 360, y: 0, w: 40, h: 20 }, "post", 1.6)
    } finally {
      unmount()
    }
  })

  it("in a square font, a flex row of text, the label and text", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ display: "flex", alignItems: "center", ...SQUARE }}>
          <div ref={ref("pre")}>AA</div>
          <span ref={ref("g")} style={{ display: "inline-grid", whiteSpace: "nowrap" }}>
            <span ref={ref("h")} style={{ gridColumnStart: 1, gridRowStart: 1, visibility: "hidden" }}>AAAA</span>
            <span ref={ref("v")} style={{ gridColumnStart: 1, gridRowStart: 1 }}>AAA</span>
          </span>
          <div ref={ref("post")}>AA</div>
        </div>
      )
      expectRect(rect("pre"), { x: 0, y: 0, w: 40, h: 20 }, "pre", 2.01)
      expectRect(rect("g"), { x: 40, y: 0, w: 80, h: 20 }, "g", 2.01)
      expectRect(rect("post"), { x: 120, y: 0, w: 40, h: 20 }, "post", 2.01)
    } finally {
      unmount()
    }
  })

  it("a hover weight on the visible span, with a transition, leaves the text around it where it was", () => {
    const { render, ref, rect, renderer, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={HELVETICA}>
          <div ref={ref("item")}>
            <span ref={ref("pre")}>before </span>
            <span ref={ref("g")} style={{ display: "inline-grid", whiteSpace: "nowrap" }}>
              <span ref={ref("h")} style={{ gridColumnStart: 1, gridRowStart: 1, visibility: "hidden", fontWeight: 600 }}>
                Label
              </span>
              <span
                ref={ref("v")}
                style={{ gridColumnStart: 1, gridRowStart: 1, transition: "font-weight 80ms", hover: { fontWeight: 600 } }}
              >
                Label
              </span>
            </span>
            <span ref={ref("post")}> after</span>
          </div>
        </div>
      )
      const before = rect("post").x
      renderer.nativeSimulateMouseMove(80, 10)
      renderer.flush()
      expect(rect("post").x).toBe(before)
      expect(rect("g").w).toBe(rect("h").w)
    } finally {
      unmount()
    }
  })
})
