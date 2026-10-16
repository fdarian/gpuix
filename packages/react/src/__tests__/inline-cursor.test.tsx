/// The `cursor` of an inline box. The paragraph asks GPUI for it over each line
/// fragment of the span's text, so it follows the pointer with no repaint, and the
/// container's cursor shows everywhere else.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: LINE, fontFamily: "Menlo", color: "#000000" } as const

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

describeNative("cursor on inline boxes", () => {
  function setup(node: React.ReactNode) {
    columnWidth()
    const testRoot = createTestRoot()
    testRoot.render(node)
    return testRoot
  }

  /** Moves the pointer to the middle of a character cell of the element `testId`
   *  and reads the cursor. */
  function cursorAt(testRoot: TestRoot, column: number, line = 0, testId = "p") {
    const element = testRoot.renderer.findByTestId(testId)
    expect(element, `missing testId ${testId}`).toBeDefined()
    const box = testRoot.renderer.getElementBounds(element!.id)!
    const width = columnWidth()
    testRoot.renderer.nativeSimulateMouseMove(
      box.x + (column + 0.5) * width,
      box.y + (line + 0.5) * LINE
    )
    return testRoot.renderer.getCursor()
  }

  it("shows the cursor of a span over its glyphs and nothing over the rest", () => {
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT}>
          Hello <span style={{ cursor: "pointer" }}>world</span>!
        </text>
      </div>
    )
    // "Hello world!": the span is columns 6-10.
    expect(cursorAt(testRoot, 1)).toBe("default")
    expect(cursorAt(testRoot, 6)).toBe("pointer")
    expect(cursorAt(testRoot, 10)).toBe("pointer")
    expect(cursorAt(testRoot, 11)).toBe("default")
    expect(cursorAt(testRoot, 30)).toBe("default")
    expect(cursorAt(testRoot, 8)).toBe("pointer")
    testRoot.unmount()
  })

  it("leaves the container's cursor where no span sets one", () => {
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={{ ...TEXT, cursor: "grab" }}>
          Hello <span>plain</span> <span style={{ cursor: "pointer" }}>link</span>
        </text>
      </div>
    )
    // "Hello plain link": plain is 6-10, link is 12-15.
    expect(cursorAt(testRoot, 1)).toBe("grab")
    expect(cursorAt(testRoot, 7)).toBe("grab")
    expect(cursorAt(testRoot, 13)).toBe("pointer")
    expect(cursorAt(testRoot, 30)).toBe("grab")
    testRoot.unmount()
  })

  it("takes the cursor of the innermost box that sets one", () => {
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT}>
          <span style={{ cursor: "pointer" }}>
            ab<span style={{ cursor: "text" }}>cd</span>
            <span style={{ color: "#ff0000" }}>ef</span>
            <span style={{ cursor: "default" }}>gh</span>
          </span>
        </text>
      </div>
    )
    expect(cursorAt(testRoot, 0)).toBe("pointer")
    expect(cursorAt(testRoot, 2)).toBe("text")
    expect(cursorAt(testRoot, 4)).toBe("pointer")
    expect(cursorAt(testRoot, 6)).toBe("default")
    expect(cursorAt(testRoot, 8)).toBe("default")
    testRoot.unmount()
  })

  it("lets a descendant of display: contents inherit its cursor", () => {
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT}>
          <span style={{ display: "contents", cursor: "pointer" }}>
            ab<span style={{ color: "#ff0000" }}>cd</span>
          </span>
        </text>
      </div>
    )
    expect(cursorAt(testRoot, 0)).toBe("default")
    expect(cursorAt(testRoot, 2)).toBe("pointer")
    testRoot.unmount()
  })

  it("follows a span onto the second line when the paragraph wraps", () => {
    const column = columnWidth()
    const testRoot = setup(
      <div style={{ width: 12.5 * column }}>
        <text testId="p" style={TEXT}>
          {"aa bbbbbb "}
          <span style={{ cursor: "pointer" }}>cccccc</span>
          {" dd"}
        </text>
      </div>
    )
    // "aa bbbbbb " fills the first line, and "cccccc dd" is the second.
    expect(cursorAt(testRoot, 1, 0)).toBe("default")
    expect(cursorAt(testRoot, 8, 0)).toBe("default")
    expect(cursorAt(testRoot, 0, 1)).toBe("pointer")
    expect(cursorAt(testRoot, 5, 1)).toBe("pointer")
    expect(cursorAt(testRoot, 7, 1)).toBe("default")
    testRoot.unmount()
  })

  it("covers both lines of a span that wraps in the middle", () => {
    const column = columnWidth()
    const testRoot = setup(
      <div style={{ width: 8.5 * column }}>
        <text testId="p" style={TEXT}>
          {"ab "}
          <span style={{ cursor: "pointer" }}>cdefg hijk</span>
        </text>
      </div>
    )
    // "ab cdefg" then "hijk".
    expect(cursorAt(testRoot, 1, 0)).toBe("default")
    expect(cursorAt(testRoot, 4, 0)).toBe("pointer")
    expect(cursorAt(testRoot, 1, 1)).toBe("pointer")
    expect(cursorAt(testRoot, 5, 1)).toBe("default")
    testRoot.unmount()
  })

  it("takes a changed cursor without the pointer moving to a new element", () => {
    const column = columnWidth()
    const tree = (cursor: string | undefined) => (
      <div style={{ width: 20 * column }}>
        <text testId="p" style={TEXT}>
          Hello <span style={{ color: "#0000ff", cursor }}>world</span>
        </text>
      </div>
    )
    const testRoot = setup(tree("pointer"))
    expect(cursorAt(testRoot, 7)).toBe("pointer")

    testRoot.render(tree("text"))
    expect(cursorAt(testRoot, 7)).toBe("text")

    testRoot.render(tree(undefined))
    expect(cursorAt(testRoot, 7)).toBe("default")
    expect(cursorAt(testRoot, 8)).toBe("default")

    testRoot.render(tree("grab"))
    expect(cursorAt(testRoot, 8)).toBe("grab")
    expect(cursorAt(testRoot, 1)).toBe("default")
    testRoot.unmount()
  })

  it("takes the cursor of a hover style once the span is hovered", () => {
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT}>
          Hello <span style={{ hover: { cursor: "pointer" } }}>world</span>
        </text>
      </div>
    )
    expect(cursorAt(testRoot, 1)).toBe("default")
    expect(cursorAt(testRoot, 7)).toBe("pointer")
    expect(cursorAt(testRoot, 2)).toBe("default")
    testRoot.unmount()
  })

  it("keeps a span with pointer-events: none off the cursor", () => {
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT}>
          <span style={{ cursor: "pointer" }}>
            ab<span style={{ cursor: "text", pointerEvents: "none" }}>cd</span>
          </span>
        </text>
      </div>
    )
    expect(cursorAt(testRoot, 0)).toBe("pointer")
    expect(cursorAt(testRoot, 2)).toBe("pointer")
    testRoot.unmount()
  })

  describe("the pointer", () => {
    it("still enters and leaves the container once, and targets the span", () => {
      const seen: string[] = []
      const elements: Record<string, unknown> = {}
      const testRoot = setup(
        <div style={{ width: 400 }}>
          <text
            testId="p"
            style={TEXT}
            onMouseEnter={() => seen.push("text enter")}
            onMouseLeave={() => seen.push("text leave")}
            onClick={(event) => seen.push(event.target === elements.span ? "click span" : "click other")}
          >
            Hello{" "}
            <span
              ref={(element) => (elements.span = element)}
              style={{ cursor: "pointer" }}
              onMouseEnter={() => seen.push("span enter")}
              onMouseLeave={() => seen.push("span leave")}
            >
              world
            </span>
            !
          </text>
        </div>
      )
      cursorAt(testRoot, 1)
      cursorAt(testRoot, 7)
      cursorAt(testRoot, 1)
      expect(seen).toEqual(["text enter", "span enter", "span leave"])

      cursorAt(testRoot, 7)
      const element = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("p")!.id)!
      const point = { x: element.x + 7.5 * columnWidth(), y: element.y + LINE / 2 }
      testRoot.renderer.nativeSimulateClick(point.x, point.y)
      expect(seen.slice(4)).toEqual(["click span"])

      testRoot.renderer.nativeSimulateMouseMove(390, 150)
      expect(seen.slice(5)).toEqual(["span leave", "text leave"])
      expect(testRoot.renderer.getCursor()).toBe("default")
      testRoot.unmount()
    })

    it("keeps an element in front of the paragraph in front of its cursor", () => {
      const testRoot = setup(
        <div style={{ width: 400, position: "relative" }}>
          <text testId="p" style={TEXT}>
            <span style={{ cursor: "pointer" }}>Hello world</span>
          </text>
          <div
            testId="cover"
            style={{ position: "absolute", left: 0, top: 0, width: 400, height: LINE, cursor: "grab" }}
          />
        </div>
      )
      expect(cursorAt(testRoot, 2)).toBe("grab")
      testRoot.unmount()
    })
  })
})
