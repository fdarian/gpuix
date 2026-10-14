/// DOM inline formatting: spans and text nodes are runs of one paragraph.
///
/// A paragraph is one painted string, so `getPaintedText()` shows it whole and
/// a line count is a height check.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: LINE } as const

function bounds(testRoot: TestRoot, testId: string) {
  const element = testRoot.renderer.findByTestId(testId)
  expect(element, `missing testId ${testId}`).toBeDefined()
  const box = testRoot.renderer.getElementBounds(element!.id)
  expect(box, `no painted bounds for ${testId}`).not.toBeNull()
  return box!
}

type Rgb = [number, number, number]

function count(pixels: Pixels, area: { x: number; y: number; width: number; height: number }, color: Rgb) {
  let found = 0
  for (let y = Math.floor(area.y); y < Math.ceil(area.y + area.height); y++) {
    for (let x = Math.floor(area.x); x < Math.ceil(area.x + area.width); x++) {
      const [r, g, b] = pixels.at(x, y)
      if (Math.abs(r - color[0]) < 8 && Math.abs(g - color[1]) < 8 && Math.abs(b - color[2]) < 8) {
        found++
      }
    }
  }
  return found
}

function shoot(testRoot: TestRoot, name: string) {
  const file = `${SHOTS_DIR}/inline-text-${name}.png`
  testRoot.renderer.captureScreenshot(file)
  return readLogicalPixels(file, testRoot.renderer)
}

describeNative("inline text", () => {
  it("lays nested spans out as one line of one string", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 600, backgroundColor: "#ffffff" }}>
        <text testId="p" style={{ ...TEXT, color: "#000000" }}>
          plain <span style={{ color: "#ff0000", fontWeight: 700 }}>red <span style={{ backgroundColor: "#ffff00" }}>marked</span></span> tail
        </text>
      </div>
    )

    expect(testRoot.renderer.getPaintedText()).toEqual(["plain red marked tail"])
    expect(bounds(testRoot, "p").height).toBeLessThanOrEqual(LINE + 2)

    const pixels = shoot(testRoot, "nested")
    const area = bounds(testRoot, "p")
    expect(count(pixels, area, [255, 0, 0])).toBeGreaterThan(20)
    expect(count(pixels, area, [255, 255, 0])).toBeGreaterThan(100)
    expect(count(pixels, area, [0, 0, 0])).toBeGreaterThan(20)
  })

  it("lets a nested span inherit its parent span and keep the parent's wash behind it", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 600, backgroundColor: "#ffffff" }}>
        <text testId="p" style={{ ...TEXT, color: "#000000" }}>
          <span style={{ color: "#0000ff", backgroundColor: "#00ff00" }}>
            outer<span>inner</span>
          </span>
        </text>
      </div>
    )

    const area = bounds(testRoot, "p")
    const pixels = shoot(testRoot, "inherit")
    // Both words are blue on green: the inner span declares neither.
    expect(count(pixels, area, [0, 0, 0])).toBe(0)
    expect(count(pixels, area, [0, 255, 0])).toBeGreaterThan(200)
  })

  it("wraps across runs like the same string in one run", () => {
    const words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu"
    // A second test root replaces the first one's native window, so measure one at a time.
    const spanned = createTestRoot()
    spanned.render(
      <div style={{ width: 140 }}>
        <text testId="p" style={TEXT}>
          alpha beta gamma <span style={{ fontWeight: 700 }}>delta epsilon zeta</span> eta theta{" "}
          <span style={{ color: "#ff0000" }}>iota kappa</span> lambda mu
        </text>
      </div>
    )
    expect(spanned.renderer.getPaintedText()).toEqual([words])
    const spannedHeight = bounds(spanned, "p").height
    expect(spannedHeight / LINE).toBeGreaterThanOrEqual(3)

    const plain = createTestRoot()
    plain.render(
      <div style={{ width: 140 }}>
        <text testId="p" style={TEXT}>
          {words}
        </text>
      </div>
    )
    // Bold is wider, so it may wrap further, never less.
    expect(spannedHeight).toBeGreaterThanOrEqual(bounds(plain, "p").height)
  })

  it("flows sibling text nodes of a block div on one line", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <div testId="d" style={{ width: 600, ...TEXT }}>
        {"one "}
        {"two "}
        {"three"}
      </div>
    )

    expect(testRoot.renderer.getPaintedText()).toEqual(["one two three"])
    expect(bounds(testRoot, "d").height).toBeLessThanOrEqual(LINE + 2)
  })

  it("splits paragraphs around a block child", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <div testId="d" style={{ width: 600, ...TEXT }}>
        one <span>two</span>
        <div testId="block" style={{ height: 30 }} />
        three <span>four</span>
      </div>
    )

    expect(testRoot.renderer.getPaintedText()).toEqual(["one two", "three four"])
    const block = bounds(testRoot, "block")
    const whole = bounds(testRoot, "d")
    expect(block.y).toBeGreaterThanOrEqual(whole.y + LINE - 2)
    expect(whole.height).toBeGreaterThanOrEqual(2 * LINE + 30 - 2)
    expect(whole.height).toBeLessThanOrEqual(2 * LINE + 30 + 4)
  })

  it("treats a text outside text or span as a block", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <div testId="d" style={{ width: 600, ...TEXT }}>
        <text>first</text>
        <text>second</text>
      </div>
    )

    expect(testRoot.renderer.getPaintedText()).toEqual(["first", "second"])
    expect(bounds(testRoot, "d").height).toBeGreaterThanOrEqual(2 * LINE - 2)
  })

  it("makes a nested text an inline run", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <text testId="p" style={{ ...TEXT, width: 600 }}>
        a<text style={{ color: "#ff0000" }}>b</text>c
      </text>
    )

    expect(testRoot.renderer.getPaintedText()).toEqual(["abc"])
    expect(bounds(testRoot, "p").height).toBeLessThanOrEqual(LINE + 2)
  })

  describe("flex and grid containers", () => {
    it("makes one item of a run of text nodes and one item per span", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div testId="row" style={{ display: "flex", flexDirection: "row", gap: 40, ...TEXT }}>
          {"left "}
          {"text"}
          <span testId="span">span</span>
        </div>
      )

      expect(testRoot.renderer.getPaintedText()).toEqual(["left text", "span"])
      const row = bounds(testRoot, "row")
      const span = bounds(testRoot, "span")
      // The span is an item of its own, so it has a box and the gap sits
      // between the paragraph and it.
      expect(span.x - row.x).toBeGreaterThan(40 + 20)
      expect(span.height).toBeLessThanOrEqual(LINE + 2)
    })

    it("does not make an item of whitespace between items", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div testId="row" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div testId="a" style={{ height: 10 }} />
          {" "}
          <div testId="b" style={{ height: 10 }} />
        </div>
      )

      expect(bounds(testRoot, "b").y - bounds(testRoot, "a").y).toBeCloseTo(18, 0)
      expect(bounds(testRoot, "row").height).toBeCloseTo(28, 0)
    })
  })

  describe("whitespace", () => {
    it("generates no box from collapsible whitespace alone", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div testId="d" style={{ width: 300, ...TEXT }}>
          {"  \n\t"}
        </div>
      )

      expect(testRoot.renderer.getPaintedText()).toEqual([])
      expect(bounds(testRoot, "d").height).toBe(0)
    })

    it("keeps a no-break space as content", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div testId="d" style={{ width: 300, ...TEXT }}>
          {" "}
        </div>
      )

      expect(testRoot.renderer.getPaintedText()).toEqual([" "])
    })

    it("keeps the space between two spans", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div style={{ width: 300, ...TEXT }}>
          <span>a</span>
          {" "}
          <span>b</span>
        </div>
      )

      expect(testRoot.renderer.getPaintedText()).toEqual(["a b"])
    })
  })

  it("generates nothing for a display none child", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 300, ...TEXT }}>
        a<span style={{ display: "none" }}>hidden</span>b
      </div>
    )

    expect(testRoot.renderer.getPaintedText()).toEqual(["ab"])
  })

  describe("selection, search and accessibility", () => {
    it("copies a paragraph without newlines between its runs", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div style={{ padding: 20 }}>
          <text style={{ fontSize: 20 }}>
            Hello <span style={{ fontWeight: 700 }}>big</span> world
          </text>
        </div>
      )

      expect(testRoot.renderer.dragSelect(21, 30, 900, 30)).toBe("Hello big world")
    })

    it("still separates paragraphs with a newline", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div style={{ display: "flex", flexDirection: "column", padding: 20 }}>
          <text style={{ fontSize: 20 }}>
            one <span>two</span>
          </text>
          <text style={{ fontSize: 20 }}>three</text>
        </div>
      )

      expect(testRoot.renderer.dragSelect(21, 30, 900, 300)).toBe("one two\nthree")
    })

    it("matches a search across the runs as one wash", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div highlight={{ query: "o bi" }}>
          <text style={{ fontSize: 20 }}>
            Hello <span style={{ fontWeight: 700 }}>big</span> world
          </text>
        </div>
      )

      const hits = testRoot.renderer.getPaintedHighlights()
      expect(hits).toHaveLength(1)
      expect(hits[0].text.slice(hits[0].start, hits[0].end)).toBe("o bi")
    })

    it("reads nested inline text in the accessibility value", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <text style={{ ...TEXT, width: 300 }}>
          Hello <span>big <text>wide</text></span> world
        </text>
      )

      const labels = Object.values(testRoot.renderer.getA11yTree().nodes ?? {})
        .map((node) => node.aria)
        .filter((aria) => aria?.role === "Label")
      expect(labels.map((aria) => aria?.value)).toContain("Hello big wide world")
    })
  })
})
