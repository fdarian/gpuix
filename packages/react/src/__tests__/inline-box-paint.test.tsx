/// The background, border and radius of an inline box. A span has no element to
/// paint them with, so its paragraph paints a quad for each line fragment of the
/// span, behind the glyphs: as tall as the content area of the font plus the
/// vertical padding and border, cut at the line breaks as `box-decoration-break:
/// slice` does.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: LINE, fontFamily: "Menlo", color: "#000000" } as const
/** The content area of Menlo at 16px: its ascent plus its descent. */
const CONTENT = 18.625
const PAGE = { width: 300, height: 120, padding: 20, backgroundColor: "#ffffff" } as const

type Rgba = [number, number, number, number]
const WHITE: Rgba = [255, 255, 255, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const YELLOW: Rgba = [255, 255, 0, 255]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]

let measuredColumn: number | undefined

/** Menlo has one advance for every character, so a column is a fixed width. Measured
 *  once, in a window of its own: a second test renderer stops the first. */
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

describeNative("background and border on inline boxes", () => {
  function setup(node: React.ReactNode) {
    columnWidth()
    const testRoot = createTestRoot({ width: 300, height: 120 })
    testRoot.render(node)
    const renderer = testRoot.renderer
    const paragraph = () => renderer.getElementBounds(renderer.findByTestId("p")!.id)!
    const shoot = (name: string) => {
      const file = `${SHOTS_DIR}/inline-box-paint-${name}.png`
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      const box = paragraph()
      /** The pixel `dx` and `dy` logical pixels from the paragraph's top left corner. */
      const px = (dx: number, dy: number) => pixels.at(Math.floor(box.x + dx), Math.floor(box.y + dy))
      return {
        px,
        /** The pixel at `column` columns from the left and `dy` from the top. */
        at: (column: number, dy: number, dx = 0) => px(column * columnWidth() + dx, dy),
      }
    }
    const bounds = (id: string) => renderer.getElementBounds(renderer.findByTestId(id)!.id)!
    return { testRoot, renderer, shoot, paragraph, bounds }
  }

  /** The line `line`'s content area starts this far below the top of the line. */
  const inset = (lineHeight: number) => (lineHeight - CONTENT) / 2

  it("paints a span's background and padding as the content area plus the padding", () => {
    const { testRoot, shoot, paragraph } = setup(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          aaaa{" "}
          <span
            style={{ backgroundColor: "#00ff00", paddingLeft: 6, paddingRight: 6, paddingTop: 4, paddingBottom: 4 }}
          >
            bbbb
          </span>{" "}
          cccc
        </text>
      </div>
    )
    const before = paragraph()
    const shot = shoot("padded")
    // Columns 5-8 hold "bbbb", with 6px of padding on each side.
    const top = inset(LINE) - 4
    const bottom = inset(LINE) + CONTENT + 4
    expect(shot.at(5, 10, 3), "in the left padding").toEqual(GREEN)
    expect(shot.at(5, 10, -3), "left of the box").toEqual(WHITE)
    expect(shot.at(9, 10, 9), "in the right padding").toEqual(GREEN)
    expect(shot.at(9, 10, 14), "right of the box").toEqual(WHITE)
    expect(shot.at(6, top + 1.5), "just inside the vertical padding above").toEqual(GREEN)
    expect(shot.at(6, top - 1.5), "just above it").toEqual(WHITE)
    expect(shot.at(6, bottom - 1.5), "just inside the vertical padding below").toEqual(GREEN)
    expect(shot.at(6, bottom + 1.5), "just below it").toEqual(WHITE)
    expect(shot.at(1, 10), "text before the box").toEqual(WHITE)
    expect(paragraph(), "the vertical padding moves no line").toEqual(before)
    expect(paragraph().height).toBeCloseTo(LINE, 1)
    testRoot.unmount()
  })

  it("fills only the content area when the line is taller than the font", () => {
    const { testRoot, shoot } = setup(
      <div style={PAGE}>
        <text testId="p" style={{ ...TEXT, lineHeight: 40 }}>
          aa <span style={{ backgroundColor: "#00ff00" }}>bbbb</span> cc
        </text>
      </div>
    )
    const shot = shoot("tall-line")
    const top = inset(40)
    // Between the glyphs of "bbbb" the background shows; a column of a glyph like `b`
    // has a vertical stem, so sample the right edge of the last cell instead.
    expect(shot.at(3, top - 2.5, 1), "the line above the content area").toEqual(WHITE)
    expect(shot.at(3, top + 1.5, 1), "the top of the content area").toEqual(GREEN)
    expect(shot.at(3, top + CONTENT - 1.5, 1), "the bottom of the content area").toEqual(GREEN)
    expect(shot.at(3, top + CONTENT + 2.5, 1), "the line below it").toEqual(WHITE)
    testRoot.unmount()
  })

  it("is as tall as the font plus the vertical padding and border, whatever the line height", () => {
    const { testRoot, bounds } = setup(
      <div style={PAGE}>
        <text testId="p" style={{ ...TEXT, lineHeight: 40 }}>
          aa{" "}
          <span testId="s" style={{ paddingTop: 4, paddingBottom: 3, borderTopWidth: 2, borderBottomWidth: 1 }}>
            bbbb
          </span>{" "}
          cc
        </text>
      </div>
    )
    const paragraph = bounds("p")
    const span = bounds("s")
    expect(span.height).toBeCloseTo(CONTENT + 4 + 3 + 2 + 1, 1)
    expect(span.y).toBeCloseTo(paragraph.y + inset(40) - 6, 1)
    expect(paragraph.height, "and the padding moves no line").toBeCloseTo(40, 1)
    testRoot.unmount()
  })

  /** At 10.5 columns "aaaa bbbb cccc" wraps after "bbbb ": the span `bbbb cccc` is
   *  columns 5-9 of line 0, the hanging space included, and columns 0-3 of line 1. */
  const wrapped = (span: object, before = "aaaa") => (
    <div style={PAGE}>
      <text testId="p" style={{ ...TEXT, width: Math.ceil(10.5 * columnWidth()) }}>
        {before}{" "}
        <span style={span}>bbbb cccc</span>
      </text>
    </div>
  )
  const BOX = {
    backgroundColor: "#ffff00",
    borderWidth: 2,
    borderColor: "#0000ff",
    borderRadius: 8,
    paddingLeft: 4,
    paddingRight: 4,
    paddingTop: 3,
    paddingBottom: 3,
  } as const

  it("cuts a span that wraps: the left border on the first line, the right on the last", () => {
    const { testRoot, shoot, paragraph } = setup(wrapped(BOX))
    const shot = shoot("wrapped")
    const mid0 = LINE / 2
    const mid1 = LINE + LINE / 2
    const column = columnWidth()

    // The first fragment: left border and padding, then the text.
    expect(shot.at(5, mid0, 1), "its left border").toEqual(BLUE)
    expect(shot.at(5, mid0, 3.5), "its background, in the padding").toEqual(YELLOW)
    // It is open at the end of the line: background to the edge of the wrapped
    // line, and no border there.
    const end = Math.ceil(10.5 * column) - 1
    expect(shot.px(end, mid0), "no right border where it wraps").toEqual(YELLOW)
    expect(shot.px(Math.ceil(10.5 * column) + 2, mid0), "and nothing past the line").toEqual(WHITE)

    // The last fragment: no left border, the right one after "cccc".
    expect(shot.px(0.5, mid1), "no left border where it continues").toEqual(YELLOW)
    expect(shot.at(4, mid1, 4 + 2 - 1), "its right border").toEqual(BLUE)
    expect(shot.at(4, mid1, 6 + 2), "and nothing after it").toEqual(WHITE)

    // The top border runs along both fragments.
    const top0 = inset(LINE) - 3 - 2
    const top1 = LINE + inset(LINE) - 3 - 2
    expect(shot.at(7, top0 + 0.5), "top border of the first").toEqual(BLUE)
    expect(shot.at(2, top1 + 0.5), "top border of the second").toEqual(BLUE)
    // Both lines stay where they were: the paragraph is two lines tall.
    expect(paragraph().height).toBeCloseTo(2 * LINE, 1)
    testRoot.unmount()
  })

  it("rounds the left corners of the first fragment and the right corners of the last", () => {
    const { testRoot, shoot } = setup(wrapped(BOX))
    const shot = shoot("corners")
    const top0 = inset(LINE) - 3 - 2
    const top1 = LINE + inset(LINE) - 3 - 2
    const bottom1 = LINE + inset(LINE) + CONTENT + 3 + 2
    const column = columnWidth()

    // A radius of 8 leaves the very corner bare.
    expect(shot.at(5, top0 + 0.5, 0.5), "first fragment, top left corner").toEqual(WHITE)
    expect(shot.at(5, top0 + 0.5, 12), "and the border along the top past it").toEqual(BLUE)
    // The open side is square: the end of the first line is filled to its corner.
    const end = Math.ceil(10.5 * column) - 1
    expect(shot.px(end, top0 + 0.5), "first fragment, top right corner").toEqual(BLUE)
    // The second fragment is open on the left and rounded on the right.
    expect(shot.px(1.5, top1 + 0.5), "last fragment, top left corner").toEqual(BLUE)
    expect(shot.at(4, top1 + 0.5, 4 + 2 + 3), "last fragment, top right corner").toEqual(WHITE)
    expect(shot.at(4, bottom1 - 0.5, 4 + 2 + 3), "last fragment, bottom right corner").toEqual(WHITE)
    expect(shot.px(1.5, bottom1 - 0.5), "last fragment, bottom left corner").toEqual(BLUE)
    testRoot.unmount()
  })

  it("lets the vertical padding of a wrapped span cover the line above without moving it", () => {
    const tall = { backgroundColor: "#00ff00", paddingTop: 9, paddingBottom: 9 } as const
    const { testRoot, shoot, paragraph } = setup(wrapped(tall, "\u00a0\u00a0\u00a0\u00a0"))
    const before = paragraph()
    const shot = shoot("overlap")
    // Fragment 2 is at line 1; its padding reaches 9px up, into line 0, where
    // columns 0-3 hold no-break spaces, so that nothing is painted over it.
    const top1 = LINE + inset(LINE) - 9
    expect(top1).toBeLessThan(LINE)
    expect(shot.at(2, top1 + 1.5), "inside the padding, above the line it belongs to").toEqual(GREEN)
    expect(shot.at(2, top1 - 1.5), "above the padding").toEqual(WHITE)
    expect(before.height).toBeCloseTo(2 * LINE, 1)
    testRoot.unmount()
  })

  it("paints a nested span's background over the one around it", () => {
    const { testRoot, shoot } = setup(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          a{" "}
          <span style={{ backgroundColor: "#ffff00", paddingLeft: 4, paddingRight: 4 }}>
            bb <span style={{ backgroundColor: "#ff0000", paddingLeft: 3, paddingRight: 3 }}>cc</span> dd
          </span>
        </text>
      </div>
    )
    const shot = shoot("nested")
    // "a bb cc dd": the outer span is columns 2-9, the inner one columns 5-6.
    // The outer box has 4px of padding on each side and the inner one 3px.
    expect(shot.at(2, 10, 2), "the outer padding").toEqual(YELLOW)
    expect(shot.at(5, 10, 2), "the outer box before the inner one").toEqual(YELLOW)
    expect(shot.at(5, 10, 5.5), "the inner box's padding is over the outer's").toEqual(RED)
    expect(shot.at(7, 10, 8.5), "the inner box's trailing padding").toEqual(RED)
    expect(shot.at(7, 10, 12), "after it, the outer box").toEqual(YELLOW)
    expect(shot.at(10, 10, 12), "the outer box's trailing padding").toEqual(YELLOW)
    expect(shot.at(10, 10, 16), "past the outer box").toEqual(WHITE)
    testRoot.unmount()
  })

  it("leaves no seam between spans that touch", () => {
    const green = { backgroundColor: "#00ff00", color: "#00ff00" } as const
    const { testRoot, shoot } = setup(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          <span style={green}>aaa</span>
          <span style={{ ...green, paddingLeft: 3.3, paddingRight: 2.7 }}>bbb</span>
          <span style={green}>ccc</span>
        </text>
      </div>
    )
    const shot = shoot("seam")
    const end = 9 * columnWidth() + 6
    for (const y of [inset(LINE) + 1.5, LINE / 2, inset(LINE) + CONTENT - 1.5]) {
      for (let x = 0; x < end - 1; x++) expect(shot.px(x, y), `x ${x}, y ${y}`).toEqual(GREEN)
    }
    expect(shot.px(end + 2, LINE / 2), "and the end of the last one").toEqual(WHITE)
    testRoot.unmount()
  })

  it("is under the pointer in its vertical padding and not in the line box above it", () => {
    const seen: string[] = []
    const hear = (name: string) => (event: { target: unknown; currentTarget: unknown }) => {
      if (name === "span" || event.target === event.currentTarget) seen.push(name)
    }
    const { testRoot, renderer, paragraph } = setup(
      <div style={PAGE}>
        <text testId="p" style={{ ...TEXT, lineHeight: 40 }} onClick={hear("text")}>
          aa{" "}
          <span style={{ paddingTop: 6, paddingBottom: 6, cursor: "pointer" }} onClick={hear("span")}>
            bbbb
          </span>
        </text>
      </div>
    )
    const box = paragraph()
    const x = box.x + 4 * columnWidth()
    // The content area is 10.7 to 29.3 of the line, and the padding reaches 6 beyond it.
    const press = (y: number) => renderer.nativeSimulateClick(x, box.y + y)
    const cursorAt = (y: number) => {
      renderer.nativeSimulateMouseMove(x, box.y + y)
      return renderer.getCursor()
    }
    press(2)
    press(7)
    press(20)
    press(33)
    press(38)
    expect(seen).toEqual(["text", "span", "span", "span", "text"])
    expect(cursorAt(2)).toBe("default")
    expect(cursorAt(7)).toBe("pointer")
    expect(cursorAt(38)).toBe("default")
    testRoot.unmount()
  })

  it("shows a hover background only while the pointer is on the span", () => {
    const { testRoot, shoot, paragraph, renderer } = setup(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          aa <span style={{ paddingLeft: 4, paddingRight: 4, hover: { backgroundColor: "#00ff00" } }}>bb</span>
        </text>
      </div>
    )
    expect(shoot("hover-off").at(3, 10, 2)).toEqual(WHITE)
    const box = paragraph()
    renderer.nativeSimulateMouseMove(box.x + 3.5 * columnWidth() + 4, box.y + LINE / 2)
    expect(shoot("hover-on").at(3, 10, 2)).toEqual(GREEN)
    testRoot.unmount()
  })

  it("paints nothing for a border without a visible colour and keeps its space", () => {
    const { testRoot, shoot } = setup(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          aa <span style={{ borderWidth: 3 }}>bb</span>
        </text>
      </div>
    )
    const shot = shoot("no-colour")
    expect(shot.at(3, 10, 1.5)).toEqual(WHITE)
    expect(shot.at(3, 10, -1.5)).toEqual(WHITE)
    testRoot.unmount()
  })
})
