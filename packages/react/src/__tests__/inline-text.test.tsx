/// DOM inline formatting: spans and text nodes are runs of one paragraph.
///
/// A paragraph is one painted string, so `getPaintedText()` shows it whole and
/// a line count is a height check.

import { createHash } from "node:crypto"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px` } as const

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

    it("matches a search across the runs of a label and a form as one wash", () => {
      for (const node of [
        <label>
          Hello <span style={{ fontWeight: 700 }}>big</span> world
        </label>,
        <form>
          Hello <span style={{ fontWeight: 700 }}>big</span> world
        </form>,
      ]) {
        const testRoot = createTestRoot()
        testRoot.render(
          <div highlight={{ query: "o bi" }} style={{ fontSize: 20 }}>
            {node}
          </div>
        )

        const hits = testRoot.renderer.getPaintedHighlights()
        expect(hits).toHaveLength(1)
        expect(hits[0].text).toBe("Hello big world")
        expect(hits[0].text.slice(hits[0].start, hits[0].end)).toBe("o bi")
      }
    })

    it("counts the matches of a label's paragraph once", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div highlight={{ query: "b" }} style={{ fontSize: 20 }}>
          <label>
            a<span>b</span>c b
          </label>
        </div>
      )

      expect(testRoot.renderer.getPaintedHighlights().map((hit) => hit.start)).toEqual([1, 4])
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

  describe("display contents inside a span", () => {
    it("joins the children of a display contents child to the paragraph", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <text testId="p" style={{ ...TEXT, width: 600 }}>
          a <span testId="s"><span style={{ display: "contents" }}>b</span></span> c
        </text>
      )

      expect(testRoot.renderer.getPaintedText()).toEqual(["a b c"])
      expect(bounds(testRoot, "p").height).toBeLessThanOrEqual(LINE + 2)
    })

    it("flattens a display contents chain and still lets a block inside split the span out", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <text testId="p" style={{ ...TEXT, width: 600 }}>
          a<span><span style={{ display: "contents" }}><span style={{ display: "contents" }}>b</span>c</span></span>d
        </text>
      )
      expect(testRoot.renderer.getPaintedText()).toEqual(["abcd"])

      const block = createTestRoot()
      block.render(
        <text testId="p" style={{ ...TEXT, width: 600 }}>
          a<span><span style={{ display: "contents" }}><div style={{ height: 30 }} /></span></span>d
        </text>
      )
      expect(block.renderer.getPaintedText()).toEqual(["a", "d"])
    })

    it("lets the text inside inherit what the display contents element declares", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div style={{ width: 600, backgroundColor: "#ffffff" }}>
          <text testId="p" style={{ ...TEXT, color: "#000000" }}>
            <span><span style={{ display: "contents", color: "#ff0000" }}>redred</span></span>
          </text>
        </div>
      )

      const area = bounds(testRoot, "p")
      const pixels = shoot(testRoot, "contents-color")
      expect(count(pixels, area, [255, 0, 0])).toBeGreaterThan(20)
      expect(count(pixels, area, [0, 0, 0])).toBe(0)
    })

    it("passes the colour and weight of a direct display contents child to its text", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div style={{ width: 600, backgroundColor: "#ffffff" }}>
          <text testId="p" style={{ ...TEXT, color: "#000000" }}>
            <span style={{ display: "contents", color: "#ff0000" }}>redred</span>
          </text>
          <div testId="q" style={{ ...TEXT, color: "#000000" }}>
            <div style={{ display: "contents", color: "#0000ff" }}>blue</div>
          </div>
        </div>
      )

      const pixels = shoot(testRoot, "contents-direct")
      const red = bounds(testRoot, "p")
      expect(count(pixels, red, [255, 0, 0])).toBeGreaterThan(20)
      expect(count(pixels, red, [0, 0, 0])).toBe(0)
      const blue = bounds(testRoot, "q")
      expect(count(pixels, blue, [0, 0, 255])).toBeGreaterThan(20)
      expect(count(pixels, blue, [0, 0, 0])).toBe(0)
    })

    it("passes what a direct display contents child declares to its text next to a block", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div style={{ width: 600, backgroundColor: "#ffffff" }}>
          <div testId="p" style={{ ...TEXT, color: "#000000" }}>
            <div style={{ display: "contents", color: "#ff0000" }}>
              redred
              <div style={{ height: 30 }} />
              redred
            </div>
          </div>
        </div>
      )

      const area = bounds(testRoot, "p")
      const pixels = shoot(testRoot, "contents-direct-block")
      expect(count(pixels, area, [255, 0, 0])).toBeGreaterThan(40)
      expect(count(pixels, area, [0, 0, 0])).toBe(0)
    })

    it("does not paint the background of a display contents element", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <div style={{ width: 600, backgroundColor: "#ffffff" }}>
          <text testId="p" style={{ ...TEXT, color: "#000000" }}>
            <span><span style={{ display: "contents", backgroundColor: "#ffff00" }}>text</span></span>
          </text>
        </div>
      )

      expect(count(shoot(testRoot, "contents-background"), bounds(testRoot, "p"), [255, 255, 0])).toBe(0)
    })
  })

  describe("the selection key of a paragraph", () => {
    const WASH: Rgb = [0, 255, 0]

    function Line({ first }: { first: boolean }) {
      return (
        <div style={{ padding: 20, backgroundColor: "#ffffff", width: 400, selectionColor: "#00ff00" }}>
          <text testId="p" style={{ fontSize: 20, color: "#000000" }}>
            {first ? "Hello " : null}
            <span>big</span> world
          </text>
        </div>
      )
    }

    it("keeps the selection painted when the first text node is removed", () => {
      const testRoot = createTestRoot()
      testRoot.render(<Line first />)
      expect(testRoot.renderer.dragSelect(21, 30, 900, 30)).toBe("Hello big world")
      expect(count(shoot(testRoot, "selection-key-before"), bounds(testRoot, "p"), WASH)).toBeGreaterThan(200)

      testRoot.render(<Line first={false} />)
      // The paragraph is the same one, now without its first words.
      expect(count(shoot(testRoot, "selection-key-after"), bounds(testRoot, "p"), WASH)).toBeGreaterThan(200)
    })
  })

  describe("textDecoration", () => {
    const WHITE = { width: 120, backgroundColor: "#ffffff", color: "#000000", ...TEXT } as const

    function signature(name: string, decoration: React.CSSProperties, paragraph: object = {}) {
      const testRoot = createTestRoot()
      testRoot.render(
        <text style={{ ...WHITE, ...paragraph }}>
          <span style={decoration as never}>xx</span>
        </text>
      )
      return digest(shoot(testRoot, `decoration-${name}`))
    }

    function digest(pixels: Pixels) {
      const hash = createHash("sha1")
      for (let y = 0; y < LINE + 12; y++) {
        for (let x = 0; x < WHITE.width; x++) hash.update(pixels.at(x, y).join(",") + ";")
      }
      return hash.digest("hex").slice(0, 10)
    }

    function colored(name: string, decoration: object, color: Rgb) {
      const testRoot = createTestRoot()
      testRoot.render(
        <text testId="p" style={WHITE}>
          <span style={decoration as never}>xx</span>
        </text>
      )
      return count(shoot(testRoot, `decoration-${name}`), bounds(testRoot, "p"), color)
    }

    it("draws every line of a shorthand that names several", () => {
      const underline = signature("u", { textDecoration: "underline" })
      const lineThrough = signature("l", { textDecoration: "line-through" })
      const both = signature("ul", { textDecoration: "underline line-through" })
      expect(both).not.toBe(underline)
      expect(both).not.toBe(lineThrough)
      expect(both).not.toBe(signature("none", { textDecoration: "none" }))
      expect(signature("ul-reversed", { textDecoration: "line-through underline" })).toBe(both)
    })

    it("draws the same as the lines set one box at a time", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <text style={WHITE}>
          <span style={{ textDecoration: "underline" }}>
            <span style={{ textDecoration: "line-through" }}>xx</span>
          </span>
        </text>
      )
      expect(signature("ul-both", { textDecoration: "underline line-through" })).toBe(
        digest(shoot(testRoot, "decoration-nested"))
      )
    })

    it("takes the colour of the shorthand for the line only", () => {
      expect(colored("red", { textDecoration: "underline #ff0000" }, [255, 0, 0])).toBeGreaterThan(4)
      expect(colored("red-first", { textDecoration: "#ff0000 line-through" }, [255, 0, 0])).toBeGreaterThan(4)
      expect(colored("red-fn", { textDecoration: "underline rgb(255, 0, 0)" }, [255, 0, 0])).toBeGreaterThan(4)
      expect(colored("plain", { textDecoration: "underline" }, [255, 0, 0])).toBe(0)
    })

    it("draws a wavy underline differently from a solid one", () => {
      const solid = signature("solid", { textDecoration: "underline solid" })
      expect(solid).toBe(signature("solid-default", { textDecoration: "underline" }))
      expect(signature("wavy", { textDecoration: "underline wavy" })).not.toBe(solid)
    })

    it("draws a double, dotted and dashed line differently from a solid one and from each other", () => {
      const signatures = ["solid", "double", "dotted", "dashed", "wavy"].map((style) =>
        signature(`style-${style}`, { textDecoration: `underline ${style} 2px` }, { paddingBottom: 12 })
      )
      expect(new Set(signatures).size).toBe(signatures.length)
    })

    it("reads the longhands like the shorthand", () => {
      expect(signature("long-line", { textDecorationLine: "underline line-through" })).toBe(
        signature("short-line", { textDecoration: "underline line-through" })
      )
      expect(signature("long-style", { textDecorationLine: "underline", textDecorationStyle: "wavy" })).toBe(
        signature("short-style", { textDecoration: "underline wavy" })
      )
      expect(colored("long-color", { textDecorationLine: "underline", textDecorationColor: "#ff0000" }, [255, 0, 0])).toBeGreaterThan(4)
    })

    it("lets a longhand override the shorthand", () => {
      expect(signature("override", { textDecoration: "underline", textDecorationLine: "line-through" })).toBe(
        signature("override-want", { textDecoration: "line-through" })
      )
      expect(colored("override-color", { textDecoration: "underline #ff0000", textDecorationColor: "#0000ff" }, [255, 0, 0])).toBe(0)
    })

    it("ignores a shorthand that is not valid", () => {
      expect(signature("invalid", { textDecoration: "underline fancy" })).toBe(signature("invalid-none", {}))
    })

    it("draws the line of a block's shorthand in its colour", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <text testId="p" style={{ ...WHITE, textDecoration: "underline line-through #ff0000" }}>
          xx
        </text>
      )
      const area = bounds(testRoot, "p")
      const pixels = shoot(testRoot, "decoration-block")
      expect(count(pixels, area, [255, 0, 0])).toBeGreaterThan(20)
    })

    it("keeps the colour of the box that declared the line when a descendant changes the text colour", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <text testId="p" style={WHITE}>
          <span style={{ textDecoration: "underline" }}>
            <span style={{ color: "#ff0000" }}>xx</span>
          </span>
        </text>
      )
      const area = bounds(testRoot, "p")
      const pixels = shoot(testRoot, "decoration-owner-color")
      // The glyphs are red and the line under them stays black.
      expect(count(pixels, area, [255, 0, 0])).toBeGreaterThan(4)
      expect(count(pixels, { ...area, y: area.y + 16, height: 4 }, [0, 0, 0])).toBeGreaterThan(4)
    })

    it("draws the underline of a box inside an underlined box besides the outer one", () => {
      const testRoot = createTestRoot()
      testRoot.render(
        <text testId="p" style={{ ...WHITE, textDecoration: "underline #ff0000" }}>
          <span style={{ textDecoration: "underline dotted 3px #0000ff" }}>xx</span>
        </text>
      )
      const area = bounds(testRoot, "p")
      const pixels = shoot(testRoot, "decoration-accumulated")
      expect(count(pixels, area, [255, 0, 0])).toBeGreaterThan(4)
      expect(count(pixels, area, [0, 0, 255])).toBeGreaterThan(4)
    })

    it("keeps the underline of the box around when the inner box says none", () => {
      expect(signature("kept", { textDecoration: "none" }, { textDecoration: "underline" })).toBe(
        signature("kept-alone", {}, { textDecoration: "underline" })
      )
    })
  })

  describe("fontStyle", () => {
    const WHITE = { width: 220, backgroundColor: "#ffffff", color: "#000000", ...TEXT } as const

    function signature(name: string, node: React.ReactNode) {
      const testRoot = createTestRoot()
      testRoot.render(node)
      const pixels = shoot(testRoot, name)
      const rows: string[] = []
      for (let y = 0; y < LINE + 12; y++) {
        for (let x = 0; x < WHITE.width; x++) rows.push(pixels.at(x, y).join(","))
      }
      return rows.join(";")
    }

    it("slants a block's text and a span inside it inherits it", () => {
      const plain = signature("style-plain", <text style={WHITE}>Hilly fish</text>)
      const italic = signature("style-italic", <text style={{ ...WHITE, fontStyle: "italic" }}>Hilly fish</text>)
      const inherited = signature(
        "style-inherited",
        <text style={{ ...WHITE, fontStyle: "italic" }}>
          Hilly <span>fish</span>
        </text>
      )

      expect(italic).not.toBe(plain)
      expect(inherited).toBe(italic)
    })

    it("lets a span set and reset the style of its own run", () => {
      const plain = signature("style-run-plain", <text style={WHITE}>Hilly fish</text>)
      const italic = signature("style-run-italic", <text style={{ ...WHITE, fontStyle: "italic" }}>Hilly fish</text>)
      const spanItalic = signature(
        "style-run-set",
        <text style={WHITE}>
          Hilly <span style={{ fontStyle: "italic" }}>fish</span>
        </text>
      )
      const spanNormal = signature(
        "style-run-reset",
        <text style={{ ...WHITE, fontStyle: "italic" }}>
          Hilly <span style={{ fontStyle: "normal" }}>fish</span>
        </text>
      )

      expect(spanItalic).not.toBe(plain)
      expect(spanItalic).not.toBe(italic)
      expect(spanNormal).not.toBe(plain)
      expect(spanNormal).not.toBe(italic)
    })
  })
})
