/// The `appRegion` of an inline box. A span has no host box, so the paragraph registers one
/// region per line fragment of its border box, and a span that wraps drags (or does not) on
/// both lines and nowhere else.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

const describeNative =
  hasNativeTestRenderer && process.platform !== "win32" ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const

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

describeNative("appRegion on inline boxes", () => {
  /** A 12-column paragraph "aaaa bbbb cccc dddd" that wraps as "aaaa bbbb " and "cccc dddd",
   *  with the span around "bbbb cccc": columns 5-8 of line 0 and columns 0-3 of line 1. */
  function setup(
    paragraphRegion: "drag" | "no-drag",
    spanStyle: React.CSSProperties,
    spanChild: React.ReactNode = "bbbb cccc"
  ) {
    const width = columnWidth()
    const testRoot = createTestRoot({ titlebarTransparent: true })
    testRoot.render(
      <div style={{ width: 12 * width, paddingTop: 40 }}>
        <text testId="p" style={{ ...TEXT, appRegion: paragraphRegion }}>
          aaaa <span style={spanStyle}>{spanChild}</span> dddd
        </text>
      </div>
    )
    return testRoot
  }

  /** Presses the middle of a character cell of the paragraph and returns the window
   *  actions that asked for. */
  function pressAt(testRoot: TestRoot, column: number, line: number) {
    const element = testRoot.renderer.findByTestId("p")
    expect(element, "missing testId p").toBeDefined()
    const box = testRoot.renderer.getElementBounds(element!.id)!
    const x = box.x + (column + 0.5) * columnWidth()
    const y = box.y + (line + 0.5) * LINE
    testRoot.renderer.nativeSimulateMouseDown(x, y)
    testRoot.renderer.nativeSimulateMouseUp(x, y)
    return testRoot.renderer.takeSimulatedWindowActions()
  }

  it("a drag span in a no-drag paragraph drags on both of its lines and nowhere else", () => {
    const testRoot = setup("no-drag", { appRegion: "drag" })
    expect(pressAt(testRoot, 6, 0)).toEqual(["move"])
    expect(pressAt(testRoot, 1, 1)).toEqual(["move"])
    expect(pressAt(testRoot, 1, 0)).toEqual([])
    expect(pressAt(testRoot, 7, 1)).toEqual([])
    expect(pressAt(testRoot, 11, 0)).toEqual([])
    testRoot.unmount()
  })

  it("a no-drag span in a drag paragraph cuts a hole on both of its lines", () => {
    const testRoot = setup("drag", { appRegion: "no-drag" })
    expect(pressAt(testRoot, 6, 0)).toEqual([])
    expect(pressAt(testRoot, 1, 1)).toEqual([])
    expect(pressAt(testRoot, 1, 0)).toEqual(["move"])
    expect(pressAt(testRoot, 7, 1)).toEqual(["move"])
    testRoot.unmount()
  })

  it("a nested span inherits, and a span inside it can set the value again", () => {
    const testRoot = setup(
      "no-drag",
      { appRegion: "drag" },
      <>
        bb<span style={{ appRegion: "no-drag" }}>bb</span> cccc
      </>
    )
    expect(pressAt(testRoot, 5, 0)).toEqual(["move"])
    expect(pressAt(testRoot, 7, 0)).toEqual([])
    expect(pressAt(testRoot, 1, 1)).toEqual(["move"])
    testRoot.unmount()
  })

  it("a span with no value inherits the paragraph's", () => {
    const testRoot = setup("drag", {})
    expect(pressAt(testRoot, 6, 0)).toEqual(["move"])
    expect(pressAt(testRoot, 1, 1)).toEqual(["move"])
    testRoot.unmount()
  })

  it("is not a region in a window with a titlebar", () => {
    const width = columnWidth()
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 12 * width, paddingTop: 40 }}>
        <text testId="p" style={TEXT}>
          aaaa <span style={{ appRegion: "drag" }}>bbbb cccc</span> dddd
        </text>
      </div>
    )
    expect(pressAt(testRoot, 6, 0)).toEqual([])
    testRoot.unmount()
  })
})
