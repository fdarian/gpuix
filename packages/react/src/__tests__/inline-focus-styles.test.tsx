/// `focusVisible` and `focusWithin` on an inline box. A span has no element to
/// apply them to, so the paragraph folds them into the span's run when it builds,
/// and it builds again when focus moves.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: LINE, fontFamily: "Menlo", color: "#000000" } as const
const PAGE = { width: 300, height: 100, padding: 10, backgroundColor: "#ffffff" } as const
const RED: [number, number, number, number] = [255, 0, 0, 255]
const BLUE: [number, number, number, number] = [0, 0, 255, 255]
const WHITE: [number, number, number, number] = [255, 255, 255, 255]

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

describeNative("focus styles on inline boxes", () => {
  function setup(node: React.ReactNode) {
    columnWidth()
    const testRoot = createTestRoot({ width: 300, height: 100 })
    testRoot.render(node)
    const shoot = (name: string) => {
      const file = `${SHOTS_DIR}/inline-focus-styles-${name}.png`
      testRoot.renderer.captureScreenshot(file)
      return readLogicalPixels(file, testRoot.renderer)
    }
    /** The pixel a little inside the top left corner of a character cell of `p`. */
    const cornerOf = (pixels: ReturnType<typeof shoot>, column: number) => {
      const box = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("p")!.id)!
      return pixels.at(Math.floor(box.x + column * columnWidth() + 1), Math.floor(box.y + 1))
    }
    const click = (column: number) => {
      const box = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("p")!.id)!
      testRoot.renderer.nativeSimulateClick(box.x + (column + 0.5) * columnWidth(), box.y + LINE / 2)
    }
    return { testRoot, shoot, cornerOf, click, tab: () => testRoot.renderer.simulateKeystrokes("Tab") }
  }

  /** `ab[cd]ef`: the span is columns 2-3. Something else to Tab to follows it. */
  const sentence = (span: object) => (
    <div style={PAGE}>
      <text testId="p" style={TEXT}>
        ab
        <span tabIndex={0} style={span}>
          cd
        </span>
        ef
      </text>
      <div tabIndex={0} style={{ width: 10, height: 10 }} />
    </div>
  )

  it("paints the focusVisible of a span while Tab holds focus on it, and the neighbours stay", () => {
    const { testRoot, shoot, cornerOf, tab } = setup(
      sentence({ focusVisible: { backgroundColor: "#ff0000" } })
    )
    const before = shoot("tab-before")
    expect(cornerOf(before, 2)).toEqual(WHITE)
    tab()
    const during = shoot("tab-during")
    expect(cornerOf(during, 2)).toEqual(RED)
    expect(cornerOf(during, 3)).toEqual(RED)
    expect(cornerOf(during, 1)).toEqual(WHITE)
    expect(cornerOf(during, 4)).toEqual(WHITE)
    tab()
    expect(cornerOf(shoot("tab-after"), 2)).toEqual(WHITE)
    testRoot.unmount()
  })

  it("paints focusVisible after Tab and not after a press, which is only focusWithin", () => {
    const { testRoot, shoot, cornerOf, click, tab } = setup(
      sentence({
        focusVisible: { backgroundColor: "#ff0000" },
        focusWithin: { backgroundColor: "#0000ff" },
      })
    )
    click(2)
    expect(cornerOf(shoot("press"), 2)).toEqual(BLUE)
    click(0)
    expect(cornerOf(shoot("press-away"), 2)).toEqual(WHITE)
    tab()
    expect(cornerOf(shoot("tab"), 2)).toEqual(RED)
    testRoot.unmount()
  })

  it("restyles a span for focusWithin while a span nested in it has focus", () => {
    const { testRoot, shoot, cornerOf, tab } = setup(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          ab
          <span style={{ focusWithin: { backgroundColor: "#0000ff" } }}>
            cd
            <span tabIndex={0} style={{ focusVisible: { backgroundColor: "#ff0000" } }}>
              ef
            </span>
          </span>
          gh
        </text>
      </div>
    )
    expect(cornerOf(shoot("within-before"), 2)).toEqual(WHITE)
    tab()
    const during = shoot("within-during")
    expect(cornerOf(during, 2), "the span around it").toEqual(BLUE)
    expect(cornerOf(during, 4), "the focused span").toEqual(RED)
    expect(cornerOf(during, 6)).toEqual(WHITE)
    testRoot.unmount()
  })
})
