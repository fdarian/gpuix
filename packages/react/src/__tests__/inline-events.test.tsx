/// Events on inline boxes. A `<span>` has no hitbox: the pointer is on it when it
/// is on a glyph of its text, and the span is then the target, with the
/// paragraph's container and its ancestors on the path.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

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

/** The middle of a character cell of the element `testId`. */
function cell(testRoot: TestRoot, testId: string, column: number, line = 0) {
  const element = testRoot.renderer.findByTestId(testId)
  expect(element, `missing testId ${testId}`).toBeDefined()
  const box = testRoot.renderer.getElementBounds(element!.id)
  expect(box, `no painted bounds for ${testId}`).not.toBeNull()
  const width = columnWidth()
  return { x: box!.x + (column + 0.5) * width, y: box!.y + (line + 0.5) * LINE }
}

type Seen = { type: string; target: unknown; currentTarget: unknown }

describeNative("events on inline boxes", () => {
  function setup(node: React.ReactNode) {
    columnWidth()
    const testRoot = createTestRoot()
    testRoot.render(node)
    return testRoot
  }

  function click(testRoot: TestRoot, point: { x: number; y: number }) {
    testRoot.renderer.nativeSimulateClick(point.x, point.y)
  }

  /** `Hello <span>world</span>!` with a listener on the text and on the span. */
  function hello(extra: { spanClick?: (event: any) => void } = {}) {
    const seen: Seen[] = []
    const elements: Record<string, unknown> = {}
    const record = (name: string) => (event: any) => {
      seen.push({ type: name, target: event.target, currentTarget: event.currentTarget })
      extra.spanClick?.(event)
    }
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text
          testId="p"
          ref={(element) => (elements.text = element)}
          style={TEXT}
          onClick={(event) => seen.push({ type: "text", target: event.target, currentTarget: event.currentTarget })}
        >
          Hello <span ref={(element) => (elements.span = element)} onClick={record("span")}>world</span>!
        </text>
      </div>
    )
    return { testRoot, seen, elements }
  }

  it("fires the span's handler for a click over its glyphs and bubbles to the text", () => {
    const { testRoot, seen, elements } = hello()
    click(testRoot, cell(testRoot, "p", 7))

    expect(seen.map((entry) => entry.type)).toEqual(["span", "text"])
    expect(seen[0].target).toBe(elements.span)
    expect(seen[0].currentTarget).toBe(elements.span)
    expect(seen[1].target).toBe(elements.span)
    expect(seen[1].currentTarget).toBe(elements.text)
  })

  it("hits the text, not the span, for a click over the words around it", () => {
    const { testRoot, seen, elements } = hello()
    click(testRoot, cell(testRoot, "p", 2))
    click(testRoot, cell(testRoot, "p", 11))

    expect(seen.map((entry) => entry.type)).toEqual(["text", "text"])
    expect(seen[0].target).toBe(elements.text)
    expect(seen[1].target).toBe(elements.text)
  })

  it("misses the span right of the end of the line and between the words", () => {
    const { testRoot, seen, elements } = hello()
    // The text fills the width of its container, and the line ends after "!".
    click(testRoot, cell(testRoot, "p", 30))

    expect(seen.map((entry) => entry.type)).toEqual(["text"])
    expect(seen[0].target).toBe(elements.text)
  })

  it("stops at the span when its handler stops propagation", () => {
    const { testRoot, seen } = hello({ spanClick: (event) => event.stopPropagation() })
    click(testRoot, cell(testRoot, "p", 6))

    expect(seen.map((entry) => entry.type)).toEqual(["span"])
  })

  it("hits a span on each line it wraps across, and no blank cell of either", () => {
    const seen: string[] = []
    const columns = 10
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={{ ...TEXT, width: Math.ceil((columns + 0.5) * columnWidth()) }}>
          aaaa <span onClick={() => seen.push("span")}>bbbb cccc</span>
        </text>
      </div>
    )
    expect(testRoot.renderer.getPaintedText()).toEqual(["aaaa bbbb cccc"])

    click(testRoot, cell(testRoot, "p", 6, 0))
    click(testRoot, cell(testRoot, "p", 1, 1))
    expect(seen).toEqual(["span", "span"])

    // Before the span on the first line, after the span on the second.
    click(testRoot, cell(testRoot, "p", 1, 0))
    click(testRoot, cell(testRoot, "p", 7, 1))
    expect(seen).toEqual(["span", "span"])
  })

  it("sends the boundary events of a pointer moving between spans of one paragraph", () => {
    const seen: string[] = []
    const record = (name: string) => () => seen.push(name)
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text
          testId="p"
          style={TEXT}
          onMouseEnter={record("text enter")}
          onMouseLeave={record("text leave")}
        >
          <span onMouseEnter={record("a enter")} onMouseLeave={record("a leave")}>
            one
          </span>{" "}
          <span onMouseEnter={record("b enter")} onMouseLeave={record("b leave")}>
            two
          </span>
        </text>
      </div>
    )
    // "one two": a is columns 0-2, the space is 3, b is 4-6.
    const move = (column: number) => {
      const point = cell(testRoot, "p", column)
      testRoot.renderer.nativeSimulateMouseMove(point.x, point.y)
    }
    move(1)
    expect(seen).toEqual(["text enter", "a enter"])
    move(5)
    expect(seen).toEqual(["text enter", "a enter", "a leave", "b enter"])
    move(3)
    expect(seen.slice(4)).toEqual(["b leave"])
    move(0)
    expect(seen.slice(5)).toEqual(["a enter"])
    testRoot.renderer.nativeSimulateMouseMove(390, 150)
    expect(seen.slice(6)).toEqual(["a leave", "text leave"])
  })

  it("reports mouseover and mouseout with the spans as related targets", () => {
    const seen: Array<[string, unknown, unknown]> = []
    const elements: Record<string, unknown> = {}
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text
          testId="p"
          style={TEXT}
          onMouseOver={(event) => seen.push(["over", event.target, event.relatedTarget])}
          onMouseOut={(event) => seen.push(["out", event.target, event.relatedTarget])}
        >
          <span ref={(element) => (elements.a = element)}>one</span>{" "}
          <span ref={(element) => (elements.b = element)}>two</span>
        </text>
      </div>
    )
    const move = (column: number) => {
      const point = cell(testRoot, "p", column)
      testRoot.renderer.nativeSimulateMouseMove(point.x, point.y)
    }
    move(1)
    move(5)
    expect(seen.slice(1)).toEqual([
      ["out", elements.a, elements.b],
      ["over", elements.b, elements.a],
    ])
  })

  it("targets a nested span and puts the span around it on the path", () => {
    const seen: Array<{ path: unknown[]; target: unknown }> = []
    const elements: Record<string, unknown> = {}
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text
          testId="p"
          style={TEXT}
          onClick={(event) => seen.push({ path: event.composedPath(), target: event.target })}
        >
          <span ref={(element) => (elements.outer = element)}>
            out <span ref={(element) => (elements.inner = element)}>in</span>
          </span>
        </text>
      </div>
    )
    click(testRoot, cell(testRoot, "p", 4))
    click(testRoot, cell(testRoot, "p", 1))

    expect(seen[0].target).toBe(elements.inner)
    expect(seen[0].path[0]).toBe(elements.inner)
    expect(seen[0].path[1]).toBe(elements.outer)
    expect(seen[1].target).toBe(elements.outer)
  })

  it("hits a nested text with a listener, which is no longer a block", () => {
    const seen: string[] = []
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT}>
          Hello <text onClick={() => seen.push("nested")}>world</text>
        </text>
      </div>
    )
    expect(testRoot.renderer.getPaintedText()).toEqual(["Hello world"])
    click(testRoot, cell(testRoot, "p", 8))
    click(testRoot, cell(testRoot, "p", 1))
    expect(seen).toEqual(["nested"])
  })

  it("lets a span with pointerEvents none through to the text", () => {
    const seen: string[] = []
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT} onClick={(event) => seen.push(String(event.target === event.currentTarget))}>
          Hello <span style={{ pointerEvents: "none" }} onClick={() => seen.push("span")}>world</span>
        </text>
      </div>
    )
    click(testRoot, cell(testRoot, "p", 8))
    expect(seen).toEqual(["true"])
  })

  it("clicks a span by element, with the span as the target and the text in the path", () => {
    const seen: Array<[string, unknown]> = []
    let span: { id: number } | undefined
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT} onClick={(event) => seen.push(["text", event.target])}>
          Hello{" "}
          <span ref={(element) => (span = element as unknown as { id: number })} onClick={() => seen.push(["span", null])}>
            world
          </span>
        </text>
      </div>
    )
    testRoot.renderer.clickElement(span!.id)
    expect(seen.map(([name]) => name)).toEqual(["span", "text"])
    expect(seen[1][1]).toBe(span)
  })

  it("still starts a selection with a press on a span, unless mousedown is prevented", () => {
    const prevent = { value: false }
    const testRoot = setup(
      <div style={{ width: 400 }}>
        <text testId="p" style={TEXT}>
          Hello{" "}
          <span
            onMouseDown={(event) => {
              if (prevent.value) event.preventDefault()
            }}
          >
            world
          </span>
        </text>
      </div>
    )
    const drag = () => {
      const from = cell(testRoot, "p", 6)
      const to = cell(testRoot, "p", 14)
      testRoot.renderer.nativeSimulateMouseDown(from.x, from.y)
      testRoot.renderer.nativeSimulateMouseMove(to.x, to.y)
      testRoot.renderer.nativeSimulateMouseUp(to.x, to.y)
      return testRoot.renderer.getSelectedText()
    }
    expect(drag()).toBe("world")
    testRoot.renderer.clearSelection()
    prevent.value = true
    expect(drag()).toBeNull()
  })

  it("finds the text of a span that listens by its text", () => {
    const { testRoot } = hello()
    expect(testRoot.renderer.findByText("world")).toBeDefined()
    expect(testRoot.renderer.getAllText().join("")).toContain("world")
  })

  describe("state styles", () => {
    const BLUE = [0, 0, 255]

    function bluePixels(testRoot: TestRoot, name: string) {
      const file = `${SHOTS_DIR}/inline-events-${name}.png`
      fs.rmSync(file, { force: true })
      testRoot.renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, testRoot.renderer)
      const box = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("p")!.id)!
      let found = 0
      for (let y = Math.floor(box.y); y < Math.ceil(box.y + box.height); y++) {
        for (let x = Math.floor(box.x); x < Math.ceil(box.x + box.width); x++) {
          const [r, g, b] = pixels.at(x, y)
          if (Math.abs(r - BLUE[0]) < 8 && Math.abs(g - BLUE[1]) < 8 && Math.abs(b - BLUE[2]) < 8) found++
        }
      }
      return found
    }

    function paragraph(style: Record<string, unknown>) {
      return setup(
        <div style={{ width: 400, backgroundColor: "#ffffff" }}>
          <text testId="p" style={TEXT}>
            Hello <span style={style}>world</span>!
          </text>
        </div>
      )
    }

    it("paints the hover style of a span while the pointer is on it", () => {
      const testRoot = paragraph({ hover: { color: "#0000ff" } })
      expect(bluePixels(testRoot, "hover-off")).toBe(0)

      const on = cell(testRoot, "p", 8)
      testRoot.renderer.nativeSimulateMouseMove(on.x, on.y)
      expect(bluePixels(testRoot, "hover-on")).toBeGreaterThan(20)

      const off = cell(testRoot, "p", 1)
      testRoot.renderer.nativeSimulateMouseMove(off.x, off.y)
      expect(bluePixels(testRoot, "hover-gone")).toBe(0)
    })

    it("paints the active style of a span while its press is held", () => {
      const testRoot = paragraph({ active: { color: "#0000ff" } })
      const on = cell(testRoot, "p", 8)
      testRoot.renderer.nativeSimulateMouseDown(on.x, on.y)
      expect(bluePixels(testRoot, "active-on")).toBeGreaterThan(20)
      testRoot.renderer.nativeSimulateMouseUp(on.x, on.y)
      expect(bluePixels(testRoot, "active-off")).toBe(0)
    })

    it("paints a span's groupHover when the pointer is on the group around it", () => {
      const testRoot = setup(
        <div style={{ width: 400, backgroundColor: "#ffffff" }}>
          <text testId="p" style={{ ...TEXT, group: true }}>
            Hello <span style={{ groupHover: { color: "#0000ff" } }}>world</span>!
          </text>
        </div>
      )
      expect(bluePixels(testRoot, "group-off")).toBe(0)
      const hello = cell(testRoot, "p", 1)
      testRoot.renderer.nativeSimulateMouseMove(hello.x, hello.y)
      expect(bluePixels(testRoot, "group-on")).toBeGreaterThan(20)
    })
  })
})
