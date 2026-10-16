/// CSS `white-space`: collapsing, forced breaks and wrapping, through the real
/// GPUI paint pipeline. A paragraph is one painted string, so
/// `getPaintedText()` shows what is left of the strings after processing.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo" } as const

function lines(testRoot: TestRoot, testId: string) {
  const element = testRoot.renderer.findByTestId(testId)
  expect(element, `missing testId ${testId}`).toBeDefined()
  const box = testRoot.renderer.getElementBounds(element!.id)
  expect(box, `no painted bounds for ${testId}`).not.toBeNull()
  return Math.round(box!.height / LINE)
}

function paint(node: React.ReactNode) {
  const testRoot = createTestRoot()
  testRoot.render(node)
  return testRoot
}

describeNative("white-space", () => {
  describe("collapsing (normal)", () => {
    it("collapses spaces, tabs and line feeds inside a string", () => {
      const testRoot = paint(
        <div style={{ width: 400, ...TEXT }}>{"a   b\t\tc\n\n  d"}</div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["a b c d"])
    })

    it("turns a line feed into a space and does not break the line", () => {
      const testRoot = paint(
        <div testId="p" style={{ width: 400, ...TEXT }}>{"one\ntwo"}</div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["one two"])
      expect(lines(testRoot, "p")).toBe(1)
    })

    it("collapses across span edges and keeps the space in the first run", () => {
      const testRoot = paint(
        <text style={{ width: 400, ...TEXT }}>
          {"a  "}
          <span style={{ fontWeight: 700 }}>{"  b  "}</span>
          {"  c"}
        </text>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["a b c"])
    })

    it("keeps one space for a span of spaces between words", () => {
      const testRoot = paint(
        <text style={{ width: 400, ...TEXT }}>
          {"a"}
          <span>{"   "}</span>
          {"b"}
        </text>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["a b"])
    })

    it("strips the space at the start and end of a paragraph", () => {
      const testRoot = paint(
        <text testId="p" style={{ width: 400, ...TEXT }}>
          {"  \n  hello "}
          <span>{"world \n "}</span>
        </text>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["hello world"])
      expect(lines(testRoot, "p")).toBe(1)
    })

    it("strips the space on each side of a block child", () => {
      const testRoot = paint(
        <div style={{ width: 400, ...TEXT }}>
          {"one "}
          <div>block</div>
          {" two"}
        </div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["one", "block", "two"])
    })

    it("keeps a no-break space", () => {
      const testRoot = paint(
        <div style={{ width: 400, ...TEXT }}>{" a  b"}</div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual([" a  b"])
    })

    it("wraps a collapsed paragraph", () => {
      const testRoot = paint(
        <div testId="p" style={{ width: 100, ...TEXT }}>
          {"alpha   beta   gamma   delta"}
        </div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["alpha beta gamma delta"])
      expect(lines(testRoot, "p")).toBeGreaterThan(1)
    })
  })

  describe("nowrap", () => {
    it("collapses and never wraps", () => {
      const testRoot = paint(
        <div testId="p" style={{ width: 100, whiteSpace: "nowrap", ...TEXT }}>
          {"alpha   beta \n gamma   delta"}
        </div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["alpha beta gamma delta"])
      expect(lines(testRoot, "p")).toBe(1)
    })
  })

  describe("pre", () => {
    it("keeps spaces and tabs, breaks at line feeds and never wraps", () => {
      const testRoot = paint(
        <div
          testId="p"
          style={{ width: 80, whiteSpace: "pre", ...TEXT }}
        >
          {"  a  b\n    a very long second line"}
        </div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["  a  b\n    a very long second line"])
      expect(lines(testRoot, "p")).toBe(2)
    })

    it("does not make a line of one final line feed", () => {
      const testRoot = paint(
        <div testId="p" style={{ width: 200, whiteSpace: "pre", ...TEXT }}>
          {"a\nb\n"}
        </div>
      )
      expect(lines(testRoot, "p")).toBe(2)
    })

    it("expands a tab to the next multiple of eight columns", () => {
      const testRoot = paint(
        <div style={{ width: 400, whiteSpace: "pre", ...TEXT }}>{"ab\tc"}</div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["ab      c"])
    })

    it("keeps the white space of a string that is only white space", () => {
      const testRoot = paint(
        <div style={{ width: 400, whiteSpace: "pre", ...TEXT }}>{"   "}</div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["   "])
    })
  })

  describe("pre-wrap", () => {
    it("keeps spaces and wraps", () => {
      const testRoot = paint(
        <div testId="p" style={{ width: 100, whiteSpace: "pre-wrap", ...TEXT }}>
          {"alpha  beta  gamma\ndelta"}
        </div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["alpha  beta  gamma\ndelta"])
      expect(lines(testRoot, "p")).toBeGreaterThan(2)
    })
  })

  describe("pre-line", () => {
    it("collapses spaces, keeps line feeds and wraps", () => {
      const testRoot = paint(
        <div testId="p" style={{ width: 400, whiteSpace: "pre-line", ...TEXT }}>
          {"  a   b  \n   c\n\nd  "}
        </div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["a b\nc\n\nd"])
      expect(lines(testRoot, "p")).toBe(4)

      const narrow = paint(
        <div testId="p" style={{ width: 80, whiteSpace: "pre-line", ...TEXT }}>
          {"alpha beta gamma delta"}
        </div>
      )
      expect(lines(narrow, "p")).toBeGreaterThan(1)
    })
  })

  describe("inheritance", () => {
    it("is inherited by text in descendant blocks, and a descendant can override it", () => {
      const testRoot = paint(
        <div style={{ width: 400, whiteSpace: "pre", ...TEXT }}>
          <div>{"a  b"}</div>
          <div style={{ whiteSpace: "normal" }}>{"a  b"}</div>
          <text>{"c  d"}</text>
        </div>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["a  b", "a b", "c  d"])
    })

    it("lets a span declare its own", () => {
      const testRoot = paint(
        <text style={{ width: 400, ...TEXT }}>
          {"a  "}
          <span style={{ whiteSpace: "pre" }}>{"b  c"}</span>
          {"  d"}
        </text>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["a b  c d"])
    })

    it("follows a change of the container", () => {
      const testRoot = createTestRoot()
      const view = (whiteSpace: "normal" | "pre") => (
        <div style={{ width: 400, whiteSpace, ...TEXT }}>{"a   b"}</div>
      )
      testRoot.render(view("normal"))
      expect(testRoot.renderer.getPaintedText()).toEqual(["a b"])
      testRoot.render(view("pre"))
      expect(testRoot.renderer.getPaintedText()).toEqual(["a   b"])
    })
  })

  describe("search, selection and lookup after collapsing", () => {
    const view = (query: string) => (
      <div style={{ width: 600 }} highlight={{ query }}>
        <text style={{ ...TEXT, fontSize: 20 }}>
          {"alpha   \n  "}
          <span style={{ fontWeight: 700 }}>{"  beta  "}</span>
          {"   gamma"}
          <span style={{ backgroundColor: "#ffff00" }}>{"   \n  delta  "}</span>
        </text>
      </div>
    )

    it("finds a match in the collapsed text, across a span edge", () => {
      const testRoot = paint(view("a beta g"))
      const hits = testRoot.renderer.getPaintedHighlights()
      expect(hits).toHaveLength(1)
      expect(hits[0].text).toBe("alpha beta gamma delta")
      expect(hits[0].text.slice(hits[0].start, hits[0].end)).toBe("a beta g")
    })

    it("does not find text that only the raw string has", () => {
      expect(paint(view("alpha   ")).renderer.getPaintedHighlights()).toEqual([])
    })

    it("paints a match in a later span where that span is", () => {
      const testRoot = paint(view("delta"))
      const hits = testRoot.renderer.getPaintedHighlights()
      expect(hits).toHaveLength(1)
      const whole = paint(view("alpha beta gamma delta")).renderer.getPaintedHighlights()[0]
      const later = hits[0].rects[0]
      expect(later.x).toBeGreaterThan(whole.rects[0].x + whole.rects[0].width / 2)
      expect(hits[0].text.slice(hits[0].start, hits[0].end)).toBe("delta")
    })

    it("selects a later span from where it is painted", () => {
      const testRoot = paint(view("delta"))
      const rect = testRoot.renderer.getPaintedHighlights()[0].rects[0]
      const y = rect.y + rect.height / 2
      expect(testRoot.renderer.dragSelect(rect.x + 1, y, rect.x + rect.width + 4, y)).toBe("delta")
    })

    it("selects and copies the collapsed text", () => {
      const testRoot = paint(view("alpha"))
      expect(testRoot.renderer.dragSelect(1, 10, 590, 10)).toBe("alpha beta gamma delta")
    })

    it("finds an element by its collapsed text", () => {
      const testRoot = paint(<text style={TEXT}>{"hello  \n   world"}</text>)
      expect(testRoot.renderer.findByText("hello world")).toBeDefined()
      expect(testRoot.renderer.findByText("hello   world")).toBeDefined()
    })

    it("reads the collapsed text in the accessibility value", () => {
      const testRoot = paint(
        <text style={{ ...TEXT, width: 300 }}>
          {"  Hello  "}
          <span>{"  big \n"}</span>
          {" world "}
        </text>
      )
      const labels = Object.values(testRoot.renderer.getA11yTree().nodes ?? {})
        .map((node) => node.aria)
        .filter((aria) => aria?.role === "Label")
      expect(labels.map((aria) => aria?.value)).toContain("Hello big world")
    })
  })
})
