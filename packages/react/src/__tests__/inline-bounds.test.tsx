/// The box of an inline element: a `<span>` has no host box, so its bounds are the
/// union of the line fragments its paragraph paints for it.

import React from "react"
import { describe, expect, it } from "vitest"
import type { IntersectionObserverEntry, PublicInstance, ResizeObserverEntry } from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

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

function setup() {
  columnWidth()
  const root = createTestRoot()
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const element = (name: string) => refs.get(name)!
  return { ...root, ref, element }
}

describeNative("bounds of an inline element", () => {
  it("is the width of its glyphs on a single line", () => {
    const { render, ref, element, unmount } = setup()
    try {
      const column = columnWidth()
      render(
        <div style={{ width: 400 }}>
          <text ref={ref("text")} style={TEXT}>
            Hello <span ref={ref("span")}>world</span>!
          </text>
        </div>
      )
      const text = element("text").getBoundingClientRect()
      const span = element("span").getBoundingClientRect()

      expect(span.x).toBeCloseTo(text.x + 6 * column, 2)
      expect(span.width).toBeCloseTo(5 * column, 2)
      expect(span.y).toBeCloseTo(text.y, 2)
      expect(span.height).toBeCloseTo(LINE, 2)
      expect(span.right).toBeCloseTo(span.x + span.width, 2)
      expect(span.bottom).toBeCloseTo(span.y + span.height, 2)
    } finally {
      unmount()
    }
  })

  it("is the union of its lines when it wraps", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const column = columnWidth()
      // "aa bbbbbb" fills the first line, and "cccccc" does not fit on it.
      render(
        <div style={{ width: 12 * column }}>
          <text ref={ref("text")} style={TEXT}>
            aa <span ref={ref("span")}>bbbbbb cccccc</span> dd
          </text>
        </div>
      )
      const text = element("text").getBoundingClientRect()
      const span = element("span").getBoundingClientRect()

      expect(span.height, "two lines").toBeCloseTo(2 * LINE, 2)
      expect(span.x, "starts at the left edge of the second line").toBeCloseTo(text.x, 2)
      expect(span.y).toBeCloseTo(text.y, 2)
      // The first line's fragment is "bbbbbb", plus the space that hangs at the wrap.
      expect(span.width, "reaches the end of the first line").toBeGreaterThanOrEqual(9 * column - 0.01)
      expect(span.width).toBeLessThanOrEqual(10 * column + 0.01)
      expect(renderer.getElementBounds((element("span") as any).id)).toMatchObject({
        height: span.height,
        width: span.width,
      })
    } finally {
      unmount()
    }
  })

  it("is nested inside the bounds of the span around it", () => {
    const { render, ref, element, unmount } = setup()
    try {
      const column = columnWidth()
      render(
        <div style={{ width: 400 }}>
          <text style={TEXT}>
            <span ref={ref("outer")}>
              ab<span ref={ref("inner")}>cd</span>ef
            </span>
          </text>
        </div>
      )
      const outer = element("outer").getBoundingClientRect()
      const inner = element("inner").getBoundingClientRect()

      expect(outer.width).toBeCloseTo(6 * column, 2)
      expect(inner.width).toBeCloseTo(2 * column, 2)
      expect(inner.x).toBeCloseTo(outer.x + 2 * column, 2)
    } finally {
      unmount()
    }
  })

  it("is a zero rect for a span with no text, and for one that stops painting", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const app = (shown: boolean) => (
        <div style={{ width: 400 }}>
          <text style={TEXT}>
            Hello <span ref={ref("empty")}> </span>
            {shown ? <span ref={ref("span")}>world</span> : null}
          </text>
        </div>
      )
      render(app(true))
      expect(element("span").getBoundingClientRect().width).toBeGreaterThan(0)
      expect(element("empty").getBoundingClientRect()).toMatchObject({ width: 0, height: 0 })
      expect(renderer.getElementBounds((element("empty") as any).id)).toBeNull()

      render(app(false))
      expect(element("span").getBoundingClientRect()).toMatchObject({ width: 0, height: 0 })
    } finally {
      unmount()
    }
  })
})

describeNative("observing an inline element", () => {
  it("ResizeObserver reports the size of the span, and again when it wraps", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const column = columnWidth()
      const entries: ResizeObserverEntry[] = []
      const app = (columns: number) => (
        <div style={{ width: columns * column }}>
          <text style={TEXT}>
            aa <span ref={ref("span")}>bbbbbb cccccc</span>
          </text>
        </div>
      )
      render(app(40))
      new ResizeObserver((batch) => entries.push(...batch)).observe(element("span"))
      renderer.dispatchNativeEvents()

      expect(entries).toHaveLength(1)
      expect(entries[0]!.target).toBe(element("span"))
      expect(entries[0]!.borderBoxSize[0]!.inlineSize).toBeCloseTo(13 * column, 2)
      expect(entries[0]!.borderBoxSize[0]!.blockSize).toBeCloseTo(LINE, 2)

      render(app(12))
      renderer.dispatchNativeEvents()
      expect(entries).toHaveLength(2)
      expect(entries[1]!.borderBoxSize[0]!.blockSize).toBeCloseTo(2 * LINE, 2)
    } finally {
      unmount()
    }
  })

  it("ResizeObserver reports a span that stops painting as 0 by 0", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const entries: ResizeObserverEntry[] = []
      const app = (visibility: "visible" | "hidden") => (
        <div style={{ width: 400 }}>
          <text style={{ ...TEXT, visibility }}>
            Hello <span ref={ref("span")}>world</span>
          </text>
        </div>
      )
      render(app("visible"))
      new ResizeObserver((batch) => entries.push(...batch)).observe(element("span"))
      renderer.dispatchNativeEvents()
      expect(entries[0]!.borderBoxSize[0]!.inlineSize).toBeGreaterThan(0)

      render(app("hidden"))
      renderer.dispatchNativeEvents()
      expect(entries.at(-1)!.borderBoxSize[0]).toEqual({ inlineSize: 0, blockSize: 0 })
    } finally {
      unmount()
    }
  })

  it("IntersectionObserver intersects the span with the clip of its container", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const column = columnWidth()
      const entries: IntersectionObserverEntry[] = []
      // The container shows the first line only, and the span is on the second.
      const app = (height: number) => (
        <div style={{ width: 12 * column, height, overflow: "hidden" }}>
          <text style={TEXT}>
            aaaaaaaaaa <span ref={ref("span")}>bbbb</span>
          </text>
        </div>
      )
      render(app(LINE))
      new IntersectionObserver((batch) => entries.push(...batch)).observe(element("span"))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.isIntersecting)).toEqual([false])

      render(app(2 * LINE))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.isIntersecting)).toEqual([false, true])
      expect(entries[1]!.intersectionRatio).toBe(1)
    } finally {
      unmount()
    }
  })
})
