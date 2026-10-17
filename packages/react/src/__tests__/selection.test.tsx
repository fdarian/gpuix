/// Text selection through the real GPUI pipeline.
///
/// The window is 1024x768 (VisualTestAppContext default) and text lays out at
/// the window's default font size, so the coordinates here are deliberately
/// generous: a drag from the far left to the far right of a line selects the
/// whole line without depending on exact glyph advances.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot } from "../testing.js"

describe("text selection", () => {
  it("selects text inside one element", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20 }}>
        <text style={{ fontSize: 20 }}>hello world</text>
      </div>
    )

    const selected = renderer.dragSelect(21, 30, 900, 30)
    expect(selected).toBe("hello world")
  })

  it("starts a selection in the empty space before the glyphs", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 40 }}>
        <text style={{ fontSize: 20 }}>hello world</text>
      </div>
    )

    // Parent padding sits outside every TextLayout bounds. The down must
    // still clamp to the nearest line, the same way a drag already does.
    expect(renderer.dragSelect(8, 50, 900, 50)).toBe("hello world")
    renderer.clearSelection()
    expect(renderer.dragSelect(8, 50, 8, 50)).toBeNull()
  })

  it("does not start a selection from a press far below the text", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 40 }}>
        <text style={{ fontSize: 20 }}>hello world</text>
      </div>
    )

    // Composer, titlebar, and empty chrome sit off the line. Starting there
    // must not claim the nearest paragraph.
    expect(renderer.dragSelect(50, 700, 900, 50)).toBeNull()
  })

  it("selects across sibling elements in document order", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20, gap: 8 }}>
        <text style={{ fontSize: 20 }}>first line</text>
        <text style={{ fontSize: 20 }}>second line</text>
        <text style={{ fontSize: 20 }}>third line</text>
      </div>
    )

    // Dragging past the last line clamps to the nearest registered element,
    // which is how a drag into the gutter behaves in any text editor.
    const selected = renderer.dragSelect(21, 30, 900, 300)
    expect(selected).toBe("first line\nsecond line\nthird line")
  })

  // Hit testing follows paint order, but the range, the wash and the copied text follow the
  // document: a positioned sibling paints late and must not move in the selection.
  describe("document order against paint order", () => {
    const lines = (first: React.CSSProperties, second: React.CSSProperties = {}) => (
      <div style={{ display: "flex", flexDirection: "column", padding: 20, gap: 8 }}>
        <text style={{ fontSize: 20, ...first }}>first line</text>
        <text style={{ fontSize: 20, ...second }}>second line</text>
        <text style={{ fontSize: 20 }}>third line</text>
      </div>
    )
    const expected = "first line\nsecond line\nthird line"

    it("copies a position: relative sibling where it sits in the document", () => {
      const { render, renderer } = createTestRoot()
      render(lines({ position: "relative" }))
      expect(renderer.dragSelect(21, 30, 900, 300)).toBe(expected)
    })

    it("keeps a reversed drag in document order over a positioned sibling", () => {
      const { render, renderer } = createTestRoot()
      render(lines({ position: "relative" }))
      expect(renderer.dragSelect(900, 105, 21, 30)).toBe(expected)
    })

    it("starts at the positioned sibling and ends in the middle of the document", () => {
      const { render, renderer } = createTestRoot()
      render(lines({ position: "relative" }))
      // Down on "first line", up past the end of "second line".
      expect(renderer.dragSelect(21, 30, 900, 70)).toBe("first line\nsecond line")
    })

    it("selects from a later line up into a positioned sibling", () => {
      const { render, renderer } = createTestRoot()
      render(lines({}, { position: "relative" }))
      // Down on "third line", up onto "second line".
      expect(renderer.dragSelect(900, 105, 21, 70)).toBe("second line\nthird line")
    })

    it("keeps a z-index: -1 sibling in document order", () => {
      const { render, renderer } = createTestRoot()
      render(lines({ position: "relative", zIndex: -1 }))
      expect(renderer.dragSelect(21, 30, 900, 300)).toBe(expected)
    })

    it("keeps a positioned sibling with a positive z-index in document order", () => {
      const { render, renderer } = createTestRoot()
      render(lines({}, { position: "relative", zIndex: 5 }))
      expect(renderer.dragSelect(21, 30, 900, 300)).toBe(expected)
    })

    it("ignores flex order, as the DOM selection does", () => {
      const { render, renderer } = createTestRoot()
      render(lines({ order: 2 }))
      // Painted top to bottom: second, third, first. A drag from the start of "third" to the end
      // of "first" selects what the document holds between them.
      expect(renderer.dragSelect(21, 70, 900, 105)).toBe("second line")
    })

    it("orders nested positioned text inside later siblings", () => {
      const { render, renderer } = createTestRoot()
      render(
        <div style={{ display: "flex", flexDirection: "column", padding: 20, gap: 8 }}>
          <div style={{ position: "relative" }}>
            <text style={{ fontSize: 20 }}>alpha</text>
            <text style={{ fontSize: 20, position: "relative" }}>beta</text>
          </div>
          <text style={{ fontSize: 20 }}>gamma</text>
        </div>
      )
      expect(renderer.dragSelect(21, 30, 900, 300)).toBe("alpha\nbeta\ngamma")
    })

    it("still hits the text painted on top when two overlap", () => {
      const { render, renderer } = createTestRoot()
      render(
        <div style={{ position: "relative", padding: 20 }}>
          <text style={{ fontSize: 20 }}>underneath</text>
          <text style={{ fontSize: 20, position: "absolute", left: 20, top: 20 }}>on top</text>
        </div>
      )
      // The press and the drag both land on glyphs of the two; paint order picks the later one.
      const selected = renderer.dragSelect(21, 30, 50, 30)
      expect(selected).not.toBeNull()
      expect(selected).not.toContain("under")
      expect("on top".startsWith(selected!)).toBe(true)
    })
  })

  it("takes a partial span from the anchor element and whole spans below", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20, gap: 8 }}>
        <text style={{ fontSize: 20 }}>aaaaaaaaaa</text>
        <text style={{ fontSize: 20 }}>bbbb</text>
      </div>
    )

    const selected = renderer.dragSelect(21, 30, 900, 300)
    expect(selected).toBe("aaaaaaaaaa\nbbbb")

    renderer.clearSelection()
    // Start halfway through the first line: the anchor span must be partial.
    const partial = renderer.dragSelect(60, 30, 900, 300)
    expect(partial).not.toBeNull()
    expect(partial!.startsWith("aaaaaaaaaa")).toBe(false)
    expect(partial!.endsWith("\nbbbb")).toBe(true)
  })

  it("resolves a reversed drag the same way", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20, gap: 8 }}>
        <text style={{ fontSize: 20 }}>alpha</text>
        <text style={{ fontSize: 20 }}>beta</text>
      </div>
    )

    const downward = renderer.dragSelect(21, 30, 900, 62)
    renderer.clearSelection()
    const upward = renderer.dragSelect(900, 62, 21, 30)
    expect(downward).toBe("alpha\nbeta")
    expect(upward).toBe(downward)
  })

  it("keeps text nested in styled divs selectable", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20 }}>
        <div style={{ display: "flex", backgroundColor: "#1e1e2e", padding: 4 }}>
          <text style={{ fontSize: 20, color: "#cdd6f4" }}>nested text</text>
        </div>
      </div>
    )

    expect(renderer.dragSelect(25, 34, 900, 34)).toBe("nested text")
  })

  it("opts out of selection with userSelect none", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20 }}>
        <text style={{ fontSize: 20, userSelect: "none" }}>untouchable</text>
      </div>
    )

    expect(renderer.dragSelect(21, 30, 900, 30)).toBeNull()
  })

  it("starts no selection from a mousedown a handler prevented", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div
        style={{ display: "flex", flexDirection: "column", padding: 20 }}
        onMouseDown={(event) => event.preventDefault()}
      >
        <text style={{ fontSize: 20 }}>hello world</text>
      </div>
    )

    expect(renderer.dragSelect(21, 30, 900, 30)).toBeNull()
  })

  it("inherits userSelect none from an ancestor", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20, userSelect: "none" }}>
        <div style={{ display: "flex" }}>
          <text style={{ fontSize: 20 }}>toolbar label</text>
        </div>
      </div>
    )

    expect(renderer.dragSelect(21, 30, 900, 30)).toBeNull()
  })

  it("re-enables selection under a userSelect none ancestor", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20, userSelect: "none" }}>
        <text style={{ fontSize: 20, userSelect: "text" }}>selectable again</text>
      </div>
    )

    expect(renderer.dragSelect(21, 30, 900, 30)).toBe("selectable again")
  })

  it("clears the selection", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20 }}>
        <text style={{ fontSize: 20 }}>clear me</text>
      </div>
    )

    expect(renderer.dragSelect(21, 30, 900, 30)).toBe("clear me")
    renderer.clearSelection()
    expect(renderer.getSelectedText()).toBeNull()
  })

  it("selects nothing for a click without movement", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20 }}>
        <text style={{ fontSize: 20 }}>just a click</text>
      </div>
    )

    expect(renderer.dragSelect(40, 30, 40, 30)).toBeNull()
  })

  it("survives a click that leases the root view", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20 }}>
        <text style={{ fontSize: 20 }}>just a click</text>
      </div>
    )

    renderer.nativeSimulateClick(40, 30)
    expect(renderer.getSelectedText()).toBeNull()
  })

  it("applies lineHeight to wrapped text", () => {
    const a = createTestRoot()
    a.render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20, width: 200 }}>
        <text style={{ fontSize: 16, lineHeight: "18px" }}>
          one two three four five six seven eight nine ten
        </text>
      </div>
    )
    // A tight line height packs the wrapped lines closer, so a drag that ends
    // at a fixed y reaches further into the text.
    const tight = a.renderer.dragSelect(21, 26, 900, 60)

    const b = createTestRoot()
    b.render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20, width: 200 }}>
        <text style={{ fontSize: 16, lineHeight: "40px" }}>
          one two three four five six seven eight nine ten
        </text>
      </div>
    )
    const loose = b.renderer.dragSelect(21, 26, 900, 60)

    expect(tight).not.toBeNull()
    expect(loose).not.toBeNull()
    expect(tight!.length).toBeGreaterThan(loose!.length)
  })

  it("selects text rendered as a div child, not only text nodes", () => {
    const { render, renderer } = createTestRoot()
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20, fontSize: 20 }}>
        <div>plain div text</div>
      </div>
    )

    expect(renderer.dragSelect(21, 30, 900, 30)).toBe("plain div text")
  })

  it("fires onSelectionChange once per real change, including clear", () => {
    const values: Array<string | null> = []
    const { render, renderer } = createTestRoot({
      onSelectionChange: (event) => {
        values.push(event.value ?? null)
      },
    })
    render(
      <div style={{ display: "flex", flexDirection: "column", padding: 20 }}>
        <text style={{ fontSize: 20 }}>hello world</text>
      </div>
    )

    expect(renderer.dragSelect(21, 30, 900, 30)).toBe("hello world")
    renderer.dispatchNativeEvents()
    expect(values).toEqual(["hello world"])

    renderer.flush()
    renderer.dispatchNativeEvents()
    expect(values).toEqual(["hello world"])

    renderer.clearSelection()
    renderer.dispatchNativeEvents()
    expect(values).toEqual(["hello world", null])
  })
})
