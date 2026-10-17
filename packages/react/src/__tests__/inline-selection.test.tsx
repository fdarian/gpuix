/// Selection, copy and find across an `inline-block`, `inline-flex` or `inline-grid` box that sits
/// on a line of text. The box's text is hosted by the paragraph around it at the byte of its
/// placeholder, so it falls between the text before the box and the text after it: a drag, the
/// wash, the copied text and the match numbers follow the document.
///
/// The expected strings are what headless Chrome's `Selection.toString()` returns for the same
/// page (chrome-headless-shell, never the Chrome app):
///
///     <div style="width:400px;font:20px/20px sans-serif">
///       <div id=a>AA <span id=ib style="display:inline-block">BB</span> CC</div><div id=d>DD</div>
///       <div id=e>EE <span style="display:inline-block"><div>x</div><div>y</div></span> FF</div>
///       <div id=f><span style="display:inline-block">1</span><span style="display:inline-block">2</span></div><div id=g>GG</div>
///       <div id=n>A <span style="display:inline-block">B <span style="display:inline-block">C</span> D</span> E</div>
///       <div id=m>MM <span style="display:inline-flex"><span>1</span><span>2</span></span> NN</div></div>
///
/// `getSelection().setBaseAndExtent(...)` from the first text node to the last (or from between the two
/// B of `ib`, and back) gives "AA BB CC\nDD", "B CC\nD" (and the same reversed), "EE \nx\ny\n FF",
/// "12\nGG", "A B C D E" and "MM \n1\n2\n NN". The font here is the default one: the positions of a
/// drag are read off the boxes, which a ref gives.

import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const TEXT = { fontSize: 20, lineHeight: "20px", color: "#000000" } as const

const inlineBlock = { display: "inline-block" } as const

/** The boxes a render names, by `ref`. */
function setup() {
  const root = createTestRoot()
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string) => {
    const element = refs.get(name)
    expect(element, `no element ${name}`).toBeDefined()
    return element?.getBoundingClientRect() ?? { x: 0, y: 0, width: 0, height: 0 }
  }
  /** A point `fraction` of the way along the box `name`, and in the middle of its first line. */
  const at = (name: string, fraction: number): [number, number] => {
    const box = rect(name)
    return [box.x + box.width * fraction, box.y + Math.min(box.height, 20) / 2]
  }
  return { ...root, ref, rect, at }
}

describeNative("selection across an inline-block", () => {
  const around = (ref: (name: string) => (instance: PublicInstance | null) => void, box: React.CSSProperties = {}) => (
    <div style={{ width: 400, ...TEXT }}>
      <div ref={ref("line")}>
        AA{" "}
        <span ref={ref("box")} style={{ ...inlineBlock, ...box }}>
          BB
        </span>{" "}
        CC
      </div>
      <div ref={ref("next")}>DD</div>
    </div>
  )

  it("copies the text before the box, in it and after it in order, on one line", () => {
    const { render, renderer, ref, at, unmount } = setup()
    try {
      render(around(ref))
      const [downX, downY] = at("line", 0.01)
      const [upX, upY] = at("next", 0.99)
      expect(renderer.dragSelect(downX, downY, upX, upY)).toBe("AA BB CC\nDD")
    } finally {
      unmount()
    }
  })

  it("starts inside the box and takes the rest of the line after it", () => {
    const { render, renderer, ref, at, unmount } = setup()
    try {
      render(around(ref))
      // Down inside the second B, up past the end of the next line.
      const [downX, downY] = at("box", 0.7)
      const [upX, upY] = at("next", 3)
      expect(renderer.dragSelect(downX, downY, upX, upY)).toBe("B CC\nDD")
    } finally {
      unmount()
    }
  })

  it("reverses: from the next block up into the box", () => {
    const { render, renderer, ref, at, unmount } = setup()
    try {
      render(around(ref))
      const [downX, downY] = at("next", 3)
      const [upX, upY] = at("box", 0.7)
      expect(renderer.dragSelect(downX, downY, upX, upY)).toBe("B CC\nDD")
    } finally {
      unmount()
    }
  })

  it("does not reach the box from the text before it", () => {
    const { render, renderer, ref, at, rect, unmount } = setup()
    try {
      render(around(ref))
      const [downX, downY] = at("line", 0.01)
      const selected = renderer.dragSelect(downX, downY, rect("box").x - 2, downY)
      expect(selected).toMatch(/^AA ?$/)
    } finally {
      unmount()
    }
  })

  it("selects inside the text of a box alone", () => {
    const { render, renderer, ref, at, unmount } = setup()
    try {
      render(around(ref))
      const [downX, downY] = at("box", 0.02)
      const [upX, upY] = at("box", 0.7)
      expect(renderer.dragSelect(downX, downY, upX, upY)).toBe("B")
    } finally {
      unmount()
    }
  })

  it("takes a box that is lifted by position: relative where it sits", () => {
    const { render, renderer, ref, at, unmount } = setup()
    try {
      render(around(ref, { position: "relative", top: 2 }))
      const [downX, downY] = at("line", 0.01)
      const [upX, upY] = at("next", 0.99)
      expect(renderer.dragSelect(downX, downY, upX, upY)).toBe("AA BB CC\nDD")
    } finally {
      unmount()
    }
  })

  it("keeps two boxes in a row on one line", () => {
    const { render, renderer, ref, at, unmount } = setup()
    try {
      render(
        <div style={{ width: 400, ...TEXT }}>
          <div ref={ref("line")}>
            <span style={inlineBlock}>1</span>
            <span style={inlineBlock}>2</span>
          </div>
          <div ref={ref("next")}>GG</div>
        </div>
      )
      const [downX, downY] = at("line", 0.01)
      const [upX, upY] = at("next", 0.99)
      expect(renderer.dragSelect(downX, downY, upX, upY)).toBe("12\nGG")
    } finally {
      unmount()
    }
  })

  it("breaks the line around the blocks of a box, as the DOM does", () => {
    const { render, renderer, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 400, ...TEXT }}>
          <div ref={ref("line")}>
            EE{" "}
            <span style={inlineBlock}>
              <div>x</div>
              <div>y</div>
            </span>{" "}
            FF
          </div>
        </div>
      )
      // The line is 40px tall: the box is two lines and the text sits on the last one's baseline.
      const line = rect("line")
      expect(renderer.dragSelect(line.x + 1, line.y + 30, line.x + 390, line.y + 30)).toBe("EE \nx\ny\n FF")
    } finally {
      unmount()
    }
  })

  it("breaks the line between the items of an inline-flex box", () => {
    const { render, renderer, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 400, ...TEXT }}>
          <div ref={ref("line")}>
            MM{" "}
            <span style={{ display: "inline-flex" }}>
              <span>1</span>
              <span>2</span>
            </span>{" "}
            NN
          </div>
        </div>
      )
      const line = rect("line")
      expect(renderer.dragSelect(line.x + 1, line.y + 10, line.x + 390, line.y + 10)).toBe("MM \n1\n2\n NN")
    } finally {
      unmount()
    }
  })

  it("follows a box in a box", () => {
    const { render, renderer, ref, rect, unmount } = setup()
    try {
      render(
        <div style={{ width: 400, ...TEXT }}>
          <div ref={ref("line")}>
            A{" "}
            <span style={inlineBlock}>
              B <span style={inlineBlock}>C</span> D
            </span>{" "}
            E
          </div>
        </div>
      )
      const line = rect("line")
      expect(renderer.dragSelect(line.x + 1, line.y + 10, line.x + 390, line.y + 10)).toBe("A B C D E")
    } finally {
      unmount()
    }
  })
})

describeNative("find across an inline-block", () => {
  // <div>a <span style="display:inline-block">b a</span> a</div>: three matches of "a", the text
  // of the box between the other two.
  const find = (activeIndex: number) => (
    <div style={{ width: 400, ...TEXT }} highlight={{ query: "a", activeIndex }}>
      a <span style={inlineBlock}>b a</span> a
    </div>
  )

  it("numbers the match inside the box between the matches around it", () => {
    const { render, renderer, unmount } = setup()
    try {
      render(find(1))
      const hits = renderer.getPaintedHighlights()
      expect(hits).toHaveLength(3)
      expect(hits.filter((hit) => hit.active).map((hit) => hit.text)).toEqual(["b a"])
    } finally {
      unmount()
    }
  })

  it("numbers the match before the box first and the one after it last", () => {
    const { render, renderer, unmount } = setup()
    try {
      render(find(0))
      // The paragraph holds the text around the box, and its two matches are one string with a
      // wash each: [start of the string, after the box].
      const active = (activeIndex: number) => {
        render(find(activeIndex))
        return renderer
          .getPaintedHighlights()
          .filter((hit) => hit.active)
          .map((hit) => (hit.text === "b a" ? "box" : hit.start === 0 ? "before" : "after"))
      }
      expect(active(0)).toEqual(["before"])
      expect(active(1)).toEqual(["box"])
      expect(active(2)).toEqual(["after"])
    } finally {
      unmount()
    }
  })
})
