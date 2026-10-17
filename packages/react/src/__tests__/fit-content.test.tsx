/// `width: fit-content` is the preferred width `min(max-content, max(min-content, stretch-fit))`, which the
/// author's `min-width` and `max-width` (a percentage too) then clamp, `min-width` winning. The content sizes
/// do not go into the author's limits: a `max-width` under the min-content width is the width, and a
/// percentage `max-width` over the max-content width does not widen the box. A child that asks for `100%` of
/// the box counts as `auto` in the box's content sizes and resolves against the final width afterwards.
///
/// The expected numbers are from headless Chrome (chrome-headless-shell, never the Chrome app):
/// fixtures/fit-content-oracle.html has every case under the name it has here, with the command that prints
/// the numbers. Each is `[box width, ...child widths]`; the heights are checked where a case wraps text.
/// Chrome does not take `fit-content(<limit>)` for `width`, so those cases use the definition,
/// `min(max-content, max(min-content, limit))`, on the numbers of the cases next to them.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))
const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

type Style = NonNullable<React.ComponentProps<"div">["style"]>

/** A child of the box: a fixed-width div, `100%` of the box, or text (a `flex` child of the box). */
type Kid = number | { width: number | string; flex?: boolean; text?: string; pre?: boolean }

const FIT = { width: "fit-content" } as const
const BLOCK = { width: 400 } as const
const COLUMN = { display: "flex", flexDirection: "column", width: 600 } as const
const ROW = { display: "flex", width: 400 } as const
const GRID = { display: "grid", width: 400, gridTemplateColumns: "400px" } as const
const RELATIVE = { position: "relative", height: 60, width: 400 } as const
const PERCENT: Kid = { width: "100%" }
const TEXT_BLOCK = { ...BLOCK, ...TEXT } as const
const TEXT_COLUMN = { ...COLUMN, ...TEXT } as const
const SHORT: Kid = { width: "100%", flex: true, text: "AAAAA" }
const LONG = "AAAA AAAA AAAA AAAA AAAA AAAA"
/** Text that does not wrap, 2000px wide: the code of a popup. */
const PRE: Kid = { width: "auto", text: "A".repeat(100), pre: true }

/** Lays out `parent > box > kids` in a window and returns the width of the box and of each kid, then the box height
 *  (and the width of the parent first, with `withParent`). */
function layout(parent: Style, box: Style, kids: Kid[], withParent = false) {
  const root = createTestRoot({ width: 1280, height: 800 })
  root.renderer.registerFont(SQUARE_FONT)
  try {
    root.render(
      <div testId="parent" style={parent}>
        <div testId="box" style={box}>
          {kids.map((kid, index) =>
            typeof kid === "number" ? (
              <div key={index} testId={`k${index}`} style={{ width: kid, height: 10 }} />
            ) : (
              <div
                key={index}
                testId={`k${index}`}
                style={{
                  width: kid.width,
                  ...(kid.text === undefined ? { height: 10 } : {}),
                  ...(kid.flex ? { display: "flex" } : {}),
                  ...(kid.pre ? { whiteSpace: "pre" } : {}),
                }}
              >
                {kid.text !== undefined && <text style={TEXT}>{kid.text}</text>}
              </div>
            )
          )}
        </div>
      </div>
    )
    const bounds = (id: string) => root.renderer.getElementBounds(root.renderer.findByTestId(id)!.id)!
    return [
      ...(withParent ? [bounds("parent").width] : []),
      bounds("box").width,
      ...kids.map((_, index) => bounds(`k${index}`).width),
      bounds("box").height,
    ]
  } finally {
    root.unmount()
  }
}

/** The widths of the parent, the box and its kids, for a parent that is shrink-to-fit. */
function widthsWithParent(parent: Style, box: Style, kids: Kid[]) {
  return layout(parent, box, kids, true).slice(0, -1)
}

/** The widths of a case, without the height. */
function widths(parent: Style, box: Style, kids: Kid[]) {
  return layout(parent, box, kids).slice(0, -1)
}

/** Every width is checked, so one run lists all the cases that differ. `tolerance` is 1px for text, which is laid out in whole pixels. */
function expectWidths(actual: number[], expected: number[], context: string, tolerance = 0.5) {
  expect.soft(actual.length, context).toBe(expected.length)
  actual.forEach((width, index) =>
    expect.soft(Math.abs(width - expected[index]!), `${context} [${index}] ${width} != ${expected[index]}`).toBeLessThan(tolerance)
  )
}

describeNative("width: fit-content", () => {
  it("is clamped by a percentage max-width: it hugs under the limit and stops at it over", () => {
    // blk fit max50 c300 / c120 / max100% c500, in a 400px block
    expectWidths(widths(BLOCK, { ...FIT, maxWidth: "50%" }, [300]), [200, 300], "max50 c300")
    expectWidths(widths(BLOCK, { ...FIT, maxWidth: "50%" }, [120]), [120, 120], "max50 c120")
    expectWidths(widths(BLOCK, { ...FIT, maxWidth: "100%" }, [500]), [400, 500], "max100% c500")
    expectWidths(widths(BLOCK, FIT, [500]), [500, 500], "no max c500")
  })

  it("is clamped by a length max-width under its min-content width", () => {
    // blk fit max200px c300 / c120
    expectWidths(widths(BLOCK, { ...FIT, maxWidth: 200 }, [300]), [200, 300], "max200px c300")
    expectWidths(widths(BLOCK, { ...FIT, maxWidth: 200 }, [120]), [120, 120], "max200px c120")
  })

  it("is widened by min-width, which wins over max-width", () => {
    // blk fit min50 c120, blk fit min350 max200 c100
    expectWidths(widths(BLOCK, { ...FIT, minWidth: "50%" }, [120]), [200, 120], "min50 c120")
    expectWidths(widths(BLOCK, { ...FIT, minWidth: 350, maxWidth: 200 }, [100]), [350, 100], "min350 max200")
  })

  it("takes a length limit with the border box: padding is part of the width", () => {
    // blk fit max50 pad bbox c300
    expectWidths(widths(BLOCK, { ...FIT, paddingLeft: 10, paddingRight: 10, maxWidth: "50%" }, [300]), [200, 300], "pad")
    // blk fit + %child pad
    expectWidths(
      widths(BLOCK, { ...FIT, paddingLeft: 10, paddingRight: 10 }, [120, PERCENT]),
      [140, 120, 120],
      "pad %child"
    )
  })

  it("hugs the other children when a child asks for 100% of it", () => {
    // blk fit + %child c120 sib, blk fit max50 + %child c300 / c120
    expectWidths(widths(BLOCK, FIT, [120, PERCENT]), [120, 120, 120], "%child with a sibling")
    expectWidths(widths(BLOCK, { ...FIT, maxWidth: "50%" }, [300, PERCENT]), [200, 300, 200], "max50 %child c300")
    expectWidths(widths(BLOCK, { ...FIT, maxWidth: "50%" }, [120, PERCENT]), [120, 120, 120], "max50 %child c120")
    expectWidths(widths(BLOCK, FIT, [PERCENT]), [0, 0], "%child alone")
  })

  it("hugs a text child that is 100% wide, and wraps it at the limit", () => {
    // txt fit %flexchild short, txt fit max50 %flexchild short, txt fit max50 long, txt fit max10% long,
    // txt fit max%100 %flexchild long
    expectWidths(widths(TEXT_BLOCK, FIT, [SHORT]), [100, 100], "short", 1.01)
    expectWidths(widths(TEXT_BLOCK, { ...FIT, maxWidth: "50%" }, [SHORT]), [100, 100], "max50 short", 1.01)
    const wraps = layout(TEXT_BLOCK, { ...FIT, maxWidth: "50%" }, [{ width: "auto", text: LONG.slice(0, 19) }])
    expect.soft(wraps[0], "wraps width").toBeCloseTo(200, 0)
    expect.soft(wraps[2], "wraps height").toBeCloseTo(40, 0)
    const squeezed = layout(TEXT_BLOCK, { ...FIT, maxWidth: "10%" }, [{ width: "auto", text: LONG.slice(0, 19) }])
    expect.soft(squeezed[0], "squeezed width").toBeCloseTo(40, 0)
    expect.soft(squeezed[2], "squeezed height").toBeCloseTo(80, 0)
    const long = layout(TEXT_BLOCK, { ...FIT, maxWidth: "100%" }, [{ width: "100%", flex: true, text: LONG }])
    expect.soft(long[0], "long width").toBeCloseTo(400, 0)
    expect.soft(long[1], "long child width").toBeCloseTo(400, 0)
    expect.soft(long[2], "long height").toBeCloseTo(40, 0)
  })

  describe("in a parent that is shrink-to-fit", () => {
    // `nest` cases of the oracle: [parent, box, ...kids]. A percentage max-width of the box is not
    // counted in the width of the parent (it is cyclic), and is of the final width of the parent.
    const ABSOLUTE = { position: "absolute", left: 0, top: 0 } as const
    const limited = { ...FIT, maxWidth: "100%" } as const

    it("hugs content narrower than the percentage", () => {
      // nest abs parent fit max% c120 (and with a 100% child), nest abs col parent fit max% c120 (and with a 100% child)
      const block = { ...ABSOLUTE, display: "block" } as const
      const column = { ...ABSOLUTE, display: "flex", flexDirection: "column" } as const
      expectWidths(widthsWithParent(block, limited, [120]), [120, 120, 120], "abs block")
      expectWidths(widthsWithParent(block, limited, [120, PERCENT]), [120, 120, 120, 120], "abs block %child")
      expectWidths(widthsWithParent(column, limited, [120]), [120, 120, 120], "abs column")
      expectWidths(widthsWithParent(column, limited, [120, PERCENT]), [120, 120, 120, 120], "abs column %child")
    })

    it("is limited by the final width of the parent", () => {
      // nest abs parent fit max50 c300, nest abs col parent fit max% c900
      expectWidths(
        widthsWithParent({ ...ABSOLUTE, display: "block", maxWidth: 400 }, { ...FIT, maxWidth: "50%" }, [300]),
        [300, 150, 300],
        "abs block max50"
      )
      expectWidths(
        widthsWithParent({ ...ABSOLUTE, display: "flex", flexDirection: "column", maxWidth: 600 }, limited, [900]),
        [600, 600, 900],
        "abs column max600"
      )
    })
  })

  describe("in a flex column", () => {
    it("hugs under the limit and stops at it over", () => {
      // col fit c120, col fit max% c120 / c900, col fit max400 c900 / c120, col fit max50% c900 / c120
      expectWidths(widths(COLUMN, FIT, [120]), [120, 120], "fit c120")
      expectWidths(widths(COLUMN, { ...FIT, maxWidth: "100%" }, [120]), [120, 120], "max% c120")
      expectWidths(widths(COLUMN, { ...FIT, maxWidth: "100%" }, [900]), [600, 900], "max% c900")
      expectWidths(widths(COLUMN, { ...FIT, maxWidth: 400 }, [900]), [400, 900], "max400 c900")
      expectWidths(widths(COLUMN, { ...FIT, maxWidth: 400 }, [120]), [120, 120], "max400 c120")
      expectWidths(widths(COLUMN, { ...FIT, maxWidth: "50%" }, [900]), [300, 900], "max50% c900")
      expectWidths(widths(COLUMN, { ...FIT, maxWidth: "50%" }, [120]), [120, 120], "max50% c120")
      expectWidths(widths(COLUMN, { ...FIT, minWidth: "50%" }, [120]), [300, 120], "min50% c120")
    })

    it("hugs a child that asks for 100% of it", () => {
      // col fit max% + %child c120, txtcol fit %flexchild short, txtcol fit max% %flexchild short
      expectWidths(widths(COLUMN, { ...FIT, maxWidth: "100%" }, [120, PERCENT]), [120, 120, 120], "%child")
      expectWidths(widths(TEXT_COLUMN, FIT, [SHORT]), [100, 100], "text %child", 1.01)
      expectWidths(widths(TEXT_COLUMN, { ...FIT, maxWidth: "100%" }, [SHORT]), [100, 100], "max% text %child", 1.01)
    })

    it("is the same as alignSelf: flex-start with a percentage max-width", () => {
      // col start max% c120
      expectWidths(
        widths({ ...COLUMN, alignItems: "flex-start" }, { maxWidth: "100%" }, [120]),
        [120, 120],
        "flex-start"
      )
    })

    it("clamps a scroll container whose content is rigid", () => {
      // col fit scroll c900, col fit scroll max% c900, col fit scroll max400 c900, col start scroll max% c900
      const scroll = { ...FIT, overflowX: "scroll" } as const
      expectWidths(widths(COLUMN, scroll, [900]), [900, 900], "scroll")
      expectWidths(widths(COLUMN, { ...scroll, maxWidth: "100%" }, [900]), [600, 900], "scroll max%")
      expectWidths(widths(COLUMN, { ...scroll, maxWidth: 400 }, [900]), [400, 900], "scroll max400")
      expectWidths(
        widths({ ...COLUMN, alignItems: "flex-start" }, { overflowX: "scroll", maxWidth: "100%" }, [900]),
        [600, 900],
        "flex-start scroll max%"
      )
    })

    it("clamps a scroll container whose content is text that does not wrap", () => {
      // pre col fit scroll, pre col fit scroll max%, pre col fit scroll max400, pre col start scroll max%,
      // pre col fit max% short
      const scroll = { ...FIT, overflowX: "scroll" } as const
      expectWidths(widths(TEXT_COLUMN, scroll, [PRE]), [2000, 2000], "pre scroll", 1.01)
      expectWidths(widths(TEXT_COLUMN, { ...scroll, maxWidth: "100%" }, [PRE]), [600, 600], "pre scroll max%", 1.01)
      expectWidths(widths(TEXT_COLUMN, { ...scroll, maxWidth: 400 }, [PRE]), [400, 400], "pre scroll max400", 1.01)
      expectWidths(
        widths({ ...TEXT_COLUMN, alignItems: "flex-start" }, { overflowX: "scroll", maxWidth: "100%" }, [PRE]),
        [600, 600],
        "pre flex-start scroll max%",
        1.01
      )
      expectWidths(
        widths(TEXT_COLUMN, { ...FIT, maxWidth: "100%" }, [{ width: "auto", text: "AAAAA", pre: true }]),
        [100, 100],
        "pre short",
        1.01
      )
    })

    it("takes a limit: fit-content(200px) over 300px of content is 300px", () => {
      expectWidths(widths(COLUMN, { width: "fit-content(200px)" }, [300]), [300, 300], "limit 200 c300")
      expectWidths(widths(COLUMN, { width: "fit-content(500px)" }, [300]), [300, 300], "limit 500 c300")
      expectWidths(widths(COLUMN, { width: "fit-content(50%)" }, [900]), [900, 900], "limit 50% c900")
    })
  })

  describe("in a flex row", () => {
    it("is clamped by max-width and widened by min-width", () => {
      // row fit max50 c300 / c120, row fit c500, row fit max200px c300, row fit min50 c120
      expectWidths(widths(ROW, { ...FIT, maxWidth: "50%" }, [300]), [200, 300], "max50 c300")
      expectWidths(widths(ROW, { ...FIT, maxWidth: "50%" }, [120]), [120, 120], "max50 c120")
      expectWidths(widths(ROW, FIT, [500]), [500, 500], "c500")
      expectWidths(widths(ROW, { ...FIT, maxWidth: 200 }, [300]), [200, 300], "max200px c300")
      expectWidths(widths(ROW, { ...FIT, minWidth: "50%" }, [120]), [200, 120], "min50 c120")
    })

    it("hugs a child that asks for 100% of it", () => {
      // row fit max50 + %child c120
      expectWidths(widths(ROW, { ...FIT, maxWidth: "50%" }, [120, PERCENT]), [120, 120, 120], "%child")
    })
  })

  describe("as a grid item", () => {
    it("is clamped by max-width and widened by min-width", () => {
      // grid fit max50 c300 / c120, grid fit max200px c300, grid fit c120, grid fit min50 c120
      expectWidths(widths(GRID, { ...FIT, maxWidth: "50%" }, [300]), [200, 300], "max50 c300")
      expectWidths(widths(GRID, { ...FIT, maxWidth: "50%" }, [120]), [120, 120], "max50 c120")
      expectWidths(widths(GRID, { ...FIT, maxWidth: 200 }, [300]), [200, 300], "max200px c300")
      expectWidths(widths(GRID, FIT, [120]), [120, 120], "c120")
      expectWidths(widths(GRID, { ...FIT, minWidth: "50%" }, [120]), [200, 120], "min50 c120")
    })

    it("hugs a child that asks for 100% of it", () => {
      // grid fit %child c120
      expectWidths(widths(GRID, FIT, [120, PERCENT]), [120, 120, 120], "%child")
    })

    it("is clamped in a grid whose column is 1fr", () => {
      // grid 1fr fit max50 c120 / c300
      const fr = { display: "grid", width: 400 } as const
      expectWidths(widths(fr, { ...FIT, maxWidth: "50%" }, [120]), [120, 120], "1fr c120")
      expectWidths(widths(fr, { ...FIT, maxWidth: "50%" }, [300]), [200, 300], "1fr c300")
    })
  })

  describe("as an inline-block", () => {
    it("is clamped by max-width and widened by min-width", () => {
      // ib fit max50 c300 / c120, ib fit min50 c120, ib fit max50 %child c120
      const inline = { ...FIT, display: "inline-block" } as const
      expectWidths(widths(BLOCK, { ...inline, maxWidth: "50%" }, [300]), [200, 300], "max50 c300")
      expectWidths(widths(BLOCK, { ...inline, maxWidth: "50%" }, [120]), [120, 120], "max50 c120")
      expectWidths(widths(BLOCK, { ...inline, minWidth: "50%" }, [120]), [200, 120], "min50 c120")
      expectWidths(widths(BLOCK, { ...inline, maxWidth: "50%" }, [120, PERCENT]), [120, 120, 120], "%child")
    })
  })

  describe("when absolutely positioned", () => {
    it("is clamped by a percentage max-width of the containing block", () => {
      // abs fit max50 c300 / c120, abs fit max50 %child c120
      const absolute = { ...FIT, position: "absolute", left: 0, maxWidth: "50%" } as const
      expectWidths(widths(RELATIVE, absolute, [300]), [200, 300], "c300")
      expectWidths(widths(RELATIVE, absolute, [120]), [120, 120], "c120")
      expectWidths(widths(RELATIVE, absolute, [120, PERCENT]), [120, 120, 120], "%child")
    })
  })

  describe("as the other keyword properties", () => {
    it("max-width and min-width: fit-content, and a flex basis", () => {
      // blk max-width fit-content c300, blk min-width fit-content c120,
      // row basis fit-content c300 sib, row basis fit-content max50 c300
      expectWidths(widths(BLOCK, { maxWidth: "fit-content" }, [300]), [300, 300], "max-width")
      expectWidths(widths(BLOCK, { maxWidth: "fit-content" }, [120]), [120, 120], "max-width c120")
      expectWidths(widths(BLOCK, { minWidth: "fit-content" }, [120]), [400, 120], "min-width")
      expectWidths(widths(ROW, { flexBasis: "fit-content" }, [300]), [300, 300], "basis")
      expectWidths(widths(ROW, { flexBasis: "fit-content", maxWidth: "50%" }, [300]), [200, 300], "basis max50")
    })
  })
})
