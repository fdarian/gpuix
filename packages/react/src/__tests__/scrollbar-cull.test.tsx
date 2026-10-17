// @ts-nocheck
/**
 * A classic scrollbar takes a gutter from the scroller it belongs to, once the scroller overflows.
 * Whether it overflows is worked out when the scroller is prepainted, so a scroller outside an
 * ancestor's clip, which is culled, has to work it out all the same: its size and the size of what
 * holds it must not depend on where the clip is.
 */

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function scene(offscreen: boolean) {
  return (
    <div style={{ width: 300, height: 400, backgroundColor: "#ffffff" }}>
      <div testId="clip" style={{ overflow: "hidden", width: 300, height: 100 }}>
        <div style={{ height: offscreen ? 140 : 0 }} />
        <div testId="scroller" style={{ overflow: "auto", width: 200, height: 50 }}>
          <div testId="child" style={{ width: "100%", height: 200, backgroundColor: "#ff0000" }}>
            <text>content</text>
          </div>
        </div>
      </div>
    </div>
  )
}

/** Blocks that scroll sideways in a list that scrolls down; `cullable` is whether the list may skip the ones below its fold. */
function feed(cullable: boolean) {
  return (
    <div testId="list" style={{ overflow: "auto", width: 300, height: 200 }}>
      {Array.from({ length: 12 }, (_, index) => (
        <div key={index} style={{ marginBottom: 8 }}>
          <div testId={`block-${index}`} style={{ overflowX: "auto", width: 280 }}>
            <div style={{ width: 600, height: 20, backgroundColor: "#335577" }}>
              <text>{`wide line ${index}`}</text>
            </div>
            {/* Takes no room, and a subtree with an absolutely placed box in it is never culled. */}
            {cullable ? null : <div style={{ position: "absolute", width: 0, height: 0 }} />}
          </div>
        </div>
      ))}
    </div>
  )
}

function settle(renderer) {
  for (let frame = 0; frame < 8; frame++) renderer.flush()
}

describeNative("the gutter of a classic scrollbar", () => {
  it("is taken by a scroller outside the clip as by one inside it", () => {
    const measure = (offscreen: boolean) => {
      const root = createTestRoot({ width: 300, height: 400 })
      const { renderer } = root
      try {
        renderer.setScrollbarStyle("classic")
        root.render(scene(offscreen))
        const before = renderer.getCulledSubtreeCount()
        settle(renderer)
        return {
          width: renderer.getElementBounds(renderer.findByTestId("child").id).width,
          culled: renderer.getCulledSubtreeCount() - before,
        }
      } finally {
        root.unmount()
      }
    }
    const inside = measure(false)
    const outside = measure(true)
    expect(inside.culled).toBe(0)
    expect(outside.culled).toBeGreaterThan(0)
    expect(inside.width).toBeLessThan(200)
    expect(outside.width).toBe(inside.width)
  })

  it("does not change when the scroller scrolls into view", () => {
    const root = createTestRoot({ width: 300, height: 400 })
    const { renderer } = root
    try {
      renderer.setScrollbarStyle("classic")
      root.render(scene(true))
      settle(renderer)
      const width = () => renderer.getElementBounds(renderer.findByTestId("child").id).width
      const outside = width()
      renderer.scrollTo(renderer.findByTestId("clip").id, 0, 140)
      const widths = [width()]
      for (let frame = 0; frame < 5; frame++) {
        renderer.flush()
        widths.push(width())
      }
      expect(widths).toEqual(widths.map(() => outside))
    } finally {
      root.unmount()
    }
  })

  it("is part of the height of the blocks of a feed, and of what the feed scrolls", () => {
    const measure = (cullable: boolean) => {
      const root = createTestRoot({ width: 300, height: 200 })
      const { renderer } = root
      try {
        renderer.setScrollbarStyle("classic")
        root.render(feed(cullable))
        const before = renderer.getCulledSubtreeCount()
        settle(renderer)
        const list = renderer.findByTestId("list").id
        const heights = () =>
          Array.from({ length: 12 }, (_, index) => renderer.getElementBounds(renderer.findByTestId(`block-${index}`).id).height)
        const settled = { heights: heights(), scrollHeight: renderer.getScrollMetrics(list)[3] }
        const culled = renderer.getCulledSubtreeCount() - before
        renderer.scrollTo(list, 0, 400)
        settle(renderer)
        return { settled, culled, scrolled: { heights: heights(), scrollHeight: renderer.getScrollMetrics(list)[3] } }
      } finally {
        root.unmount()
      }
    }
    const everything = measure(false)
    const skipping = measure(true)
    expect(everything.culled).toBe(0)
    expect(skipping.culled).toBeGreaterThan(0)
    expect(everything.settled.heights[0]).toBeGreaterThan(20)
    expect(skipping.settled).toEqual(everything.settled)
    expect(skipping.scrolled).toEqual(everything.scrolled)
    expect(everything.scrolled).toEqual(everything.settled)
  })
})
