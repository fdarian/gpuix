/// `text-box-trim`, `text-box-edge` and `text-box`: a block container is shorter by the
/// space above its first line and below its last line that its edge leaves out.

import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FONT_SIZE = 16
const LINE = 24
const TEXT = { fontSize: FONT_SIZE, lineHeight: 1.5, fontFamily: "Menlo", color: "#000000" } as const
/** Menlo's metrics in em, units of 2048. */
const ASCENT = (1901 / 2048) * FONT_SIZE
const DESCENT = (483 / 2048) * FONT_SIZE
const CAP = (1493 / 2048) * FONT_SIZE
const EX = (1120 / 2048) * FONT_SIZE
const HALF_LEADING = (LINE - ASCENT - DESCENT) / 2
const BASELINE = HALF_LEADING + ASCENT

/** A box is laid out up to the next whole pixel of a 1x window, so a height is the
 *  ceiling of what the metrics say; the geometry of the lines themselves is exact. */
function near(actual: number, expected: number) {
  expect(actual, `${expected} laid out`).toBe(Math.ceil(expected - 1e-4))
}

let measuredColumn: number | undefined

/** Menlo has one advance for every character. Measured once, in a window of its own: a
 *  second test renderer stops the first. */
function columnWidth() {
  measuredColumn ??= measureColumn()
  return measuredColumn
}

function measureColumn() {
  const testRoot = createTestRoot()
  testRoot.render(
    <div highlight={{ query: "aaaaaaaaaa" }}>
      <text style={TEXT}>{"aaaaaaaaaa"}</text>
    </div>
  )
  const width = testRoot.renderer.getPaintedHighlights()[0]!.rects[0]!.width / 10
  testRoot.unmount()
  return width
}

function setup() {
  columnWidth()
  const root = createTestRoot({ width: 600, height: 400 })
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string) => refs.get(name)!.getBoundingClientRect()
  return { ...root, ref, rect }
}

/** The height of `<text style={{ ...TEXT, ...style }}>Hello</text>`. */
function heightOf(style: object, children: React.ReactNode = "Hello") {
  const { render, ref, rect, unmount } = setup()
  try {
    render(
      <div style={{ width: 300 }}>
        <text ref={ref("box")} style={{ ...TEXT, ...style }}>
          {children}
        </text>
      </div>
    )
    return rect("box").height
  } finally {
    unmount()
  }
}

describeNative("text-box-trim", () => {
  it("leaves a line its line height when nothing is trimmed", () => {
    near(heightOf({}), LINE)
    near(heightOf({ textBoxTrim: "none", textBoxEdge: "cap alphabetic" }), LINE)
    near(heightOf({ textBox: "normal" }), LINE)
  })

  it("trim-both cap alphabetic makes a line as tall as the cap height", () => {
    near(heightOf({ textBoxTrim: "trim-both", textBoxEdge: "cap alphabetic" }), CAP)
  })

  it("trim-start cuts down to the cap height above the baseline and keeps what is below the line", () => {
    const height = heightOf({ textBoxTrim: "trim-start", textBoxEdge: "cap alphabetic" })
    near(height, LINE - (BASELINE - CAP))
  })

  it("trim-end cuts everything below the baseline for an alphabetic under edge", () => {
    const height = heightOf({ textBoxTrim: "trim-end", textBoxEdge: "cap alphabetic" })
    near(height, BASELINE)
  })

  it("the ex edge cuts down to the x-height", () => {
    near(heightOf({ textBoxTrim: "trim-both", textBoxEdge: "ex alphabetic" }), EX)
  })

  it("the text edge cuts only the half-leading", () => {
    near(heightOf({ textBoxTrim: "trim-both", textBoxEdge: "text" }), ASCENT + DESCENT)
    near(heightOf({ textBoxTrim: "trim-both", textBoxEdge: "text text" }), ASCENT + DESCENT)
    near(heightOf({ textBoxTrim: "trim-start", textBoxEdge: "text" }), LINE - HALF_LEADING)
  })

  it("auto is the text edge, and so is no edge at all", () => {
    const text = ASCENT + DESCENT
    near(heightOf({ textBoxTrim: "trim-both" }), text)
    near(heightOf({ textBoxTrim: "trim-both", textBoxEdge: "auto" }), text)
  })

  it("a single cap or ex edge keeps the text under edge", () => {
    near(heightOf({ textBoxTrim: "trim-both", textBoxEdge: "cap" }), CAP + DESCENT)
    near(heightOf({ textBoxTrim: "trim-both", textBoxEdge: "ex" }), EX + DESCENT)
  })

  it("a text-edge trim of a line shorter than its font grows the box by the negative half-leading", () => {
    const tight = { lineHeight: "12px" }
    near(heightOf({ ...tight, textBoxTrim: "trim-both", textBoxEdge: "text" }), ASCENT + DESCENT)
  })

  it("ignores an invalid shorthand", () => {
    near(heightOf({ textBox: "trim-both sideways" }), LINE)
  })

  it("keeps the padding and the border outside the trim", () => {
    const height = heightOf({ textBoxTrim: "trim-both", textBoxEdge: "cap alphabetic", padding: 10, border: "2px solid red" })
    near(height, CAP + 24)
  })

  it("does not change a box with a height of its own", () => {
    const height = heightOf({ textBoxTrim: "trim-both", textBoxEdge: "cap alphabetic", height: 50 })
    near(height, 50)
  })

  it("trims only the first top and the last bottom of a paragraph that wraps", () => {
    const column = columnWidth()
    const style = { width: 11 * column }
    // "aaaaa bbbbb ccccc ddddd": two words to a line.
    const words = "aaaaa bbbbb ccccc ddddd"
    near(heightOf({ ...style }, words), 2 * LINE)
    near(
      heightOf({ ...style, textBoxTrim: "trim-both", textBoxEdge: "cap alphabetic" }, words),
      2 * LINE - (BASELINE - CAP) - (LINE - BASELINE)
    )
    near(
      heightOf({ ...style, textBoxTrim: "trim-start", textBoxEdge: "cap alphabetic" }, words),
      2 * LINE - (BASELINE - CAP)
    )
  })

  it("moves the next sibling up by what was cut, and the text with it", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300 }}>
          <text ref={ref("box")} style={{ ...TEXT, textBoxTrim: "trim-both", textBoxEdge: "cap alphabetic" }}>
            Hello
          </text>
          <div ref={ref("next")} style={{ height: 10 }} />
        </div>
      )
      expect(rect("box").top).toBeCloseTo(0, 3)
      near(rect("next").top, CAP)
    } finally {
      unmount()
    }
  })

  it("paints, selects and finds the glyphs where the line would have been, above the box", () => {
    const { render, renderer, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300, paddingTop: 30 }} highlight={{ query: "Hello" }}>
          <text ref={ref("box")} style={{ ...TEXT, textBoxTrim: "trim-both", textBoxEdge: "cap alphabetic" }}>
            Hello
          </text>
        </div>
      )
      const box = rect("box")
      const wash = renderer.getPaintedHighlights()[0]!.rects[0]!
      // The row of the line starts above the box by what was cut, and is as tall as it was.
      expect(wash.y).toBeCloseTo(box.top - (BASELINE - CAP), 3)
      expect(wash.height).toBeCloseTo(LINE, 3)
      // The baseline is a cap height below the top of the box.
      expect(wash.y + BASELINE).toBeCloseTo(box.top + CAP, 3)
    } finally {
      unmount()
    }
  })

  it("is read back by getBoundingClientRect as the element's bounds", () => {
    const { render, renderer, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300 }}>
          <text ref={ref("box")} testId="box" style={{ ...TEXT, textBoxTrim: "trim-both", textBoxEdge: "cap alphabetic" }}>
            Hello
          </text>
        </div>
      )
      const bounds = renderer.getElementBounds(renderer.findByTestId("box")!.id)!
      near(rect("box").height, bounds.height)
      expect(rect("box").top).toBeCloseTo(bounds.y, 3)
    } finally {
      unmount()
    }
  })
})

describeNative("text-box-edge", () => {
  it("is inherited by the block that holds the line", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300, textBoxEdge: "cap alphabetic" }}>
          <text ref={ref("trimmed")} style={{ ...TEXT, textBoxTrim: "trim-both" }}>
            Hello
          </text>
          <text ref={ref("own")} style={{ ...TEXT, textBoxTrim: "trim-both", textBoxEdge: "ex alphabetic" }}>
            Hello
          </text>
        </div>
      )
      near(rect("trimmed").height, CAP)
      near(rect("own").height, EX)
    } finally {
      unmount()
    }
  })

  it("passes through a display: contents element", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300 }}>
          <div style={{ display: "contents", textBoxEdge: "cap alphabetic" }}>
            <text ref={ref("box")} style={{ ...TEXT, textBoxTrim: "trim-both" }}>
              Hello
            </text>
          </div>
        </div>
      )
      near(rect("box").height, CAP)
    } finally {
      unmount()
    }
  })
})

describeNative("text-box", () => {
  it("is a trim and an edge in either order, a trim left out being trim-both", () => {
    near(heightOf({ textBox: "trim-both cap alphabetic" }), CAP)
    near(heightOf({ textBox: "cap alphabetic" }), CAP)
    near(heightOf({ textBox: "ex alphabetic trim-both" }), EX)
    near(heightOf({ textBox: "trim-end cap alphabetic" }), BASELINE)
    near(heightOf({ textBox: "trim-start" }), LINE - HALF_LEADING)
    near(heightOf({ textBox: "normal" }), LINE)
  })

  it("is overridden by a longhand", () => {
    near(heightOf({ textBox: "trim-both cap alphabetic", textBoxTrim: "trim-end" }), BASELINE)
    near(heightOf({ textBox: "trim-both cap alphabetic", textBoxEdge: "ex alphabetic" }), EX)
  })
})

describeNative("text-box-trim through blocks", () => {
  it("trims the first line of the first child and the last line of the last child, not the ones between", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("box")} style={{ width: 300, textBox: "trim-both cap alphabetic" }}>
          <text ref={ref("first")} style={TEXT}>
            one
          </text>
          <text ref={ref("middle")} style={TEXT}>
            two
          </text>
          <text ref={ref("last")} style={TEXT}>
            three
          </text>
        </div>
      )
      near(rect("first").height, LINE - (BASELINE - CAP))
      near(rect("middle").height, LINE)
      near(rect("last").height, BASELINE)
      // The boxes are laid out to whole pixels one by one.
      expect(rect("box").height).toBe(Math.ceil(LINE - (BASELINE - CAP)) + LINE + Math.ceil(BASELINE))
    } finally {
      unmount()
    }
  })

  it("goes down through nested blocks", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("box")} style={{ width: 300, textBox: "trim-both cap alphabetic" }}>
          <div>
            <div>
              <text ref={ref("text")} style={TEXT}>
                one
              </text>
            </div>
          </div>
        </div>
      )
      near(rect("text").height, CAP)
      near(rect("box").height, CAP)
    } finally {
      unmount()
    }
  })

  it("stops at padding or a border on the side of a block between", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("box")} style={{ width: 300, textBox: "trim-both cap alphabetic" }}>
          <div style={{ paddingTop: 4 }}>
            <text ref={ref("padded")} style={TEXT}>
              one
            </text>
          </div>
          <div style={{ borderBottom: "1px solid red" }}>
            <text ref={ref("bordered")} style={TEXT}>
              two
            </text>
          </div>
        </div>
      )
      // The first block's start and the last block's end are stopped; the other ends go through.
      near(rect("padded").height, LINE)
      near(rect("bordered").height, LINE)
      near(rect("box").height, 2 * LINE + 4 + 1)
    } finally {
      unmount()
    }
  })

  it("uses the metric of the block that holds the line", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300, textBox: "trim-both cap alphabetic" }}>
          <div style={{ textBoxEdge: "ex alphabetic" }}>
            <text ref={ref("text")} style={TEXT}>
              one
            </text>
          </div>
        </div>
      )
      near(rect("text").height, EX)
    } finally {
      unmount()
    }
  })

  it("skips an absolutely positioned child when looking for the first and the last", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300, textBox: "trim-both cap alphabetic", position: "relative" }}>
          <text style={{ ...TEXT, position: "absolute", top: 0, left: 0 }}>overlay</text>
          <text ref={ref("text")} style={TEXT}>
            one
          </text>
          <text style={{ ...TEXT, position: "absolute", top: 0, left: 0 }}>overlay</text>
        </div>
      )
      near(rect("text").height, CAP)
    } finally {
      unmount()
    }
  })

  it("is not passed through a flex container, and a flex container does not trim", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300 }}>
          <div style={{ display: "flex", flexDirection: "column", textBox: "trim-both cap alphabetic" }}>
            <text ref={ref("item")} style={TEXT}>
              one
            </text>
          </div>
          <div style={{ display: "flex", flexDirection: "column" }}>
            <text ref={ref("own")} style={{ ...TEXT, textBox: "trim-both cap alphabetic" }}>
              one
            </text>
          </div>
        </div>
      )
      near(rect("item").height, LINE)
      // A flex item that is a block container trims its own lines.
      near(rect("own").height, CAP)
    } finally {
      unmount()
    }
  })

  it("trim-none on a descendant does not take off the trim of an ancestor", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300, textBox: "trim-both cap alphabetic" }}>
          <text ref={ref("text")} style={{ ...TEXT, textBoxTrim: "none" }}>
            one
          </text>
        </div>
      )
      near(rect("text").height, CAP)
    } finally {
      unmount()
    }
  })

  it("trims a paragraph of inline children as one text", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("box")} style={{ width: 300, ...TEXT, textBox: "trim-both cap alphabetic" }}>
          one <span>two</span> three
        </div>
      )
      near(rect("box").height, CAP)
    } finally {
      unmount()
    }
  })

  it("is not applied to a block with no text", () => {
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 300 }}>
          <div ref={ref("box")} style={{ height: 40, textBox: "trim-both cap alphabetic" }} />
        </div>
      )
      near(rect("box").height, 40)
    } finally {
      unmount()
    }
  })
})
