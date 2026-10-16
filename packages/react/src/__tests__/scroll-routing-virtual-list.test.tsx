import { describe, expect, it } from "vitest"
import type { StyleDesc } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/**
 * An outer scroller (300 x 200, 800 tall inside) holding a 100 px spacer, a virtual list
 * (200 x 100 over 20 rows of 40, so 700 to scroll) and a filler. (50, 150) is over both.
 */
function listInScroller(options: { list?: StyleDesc } = {}) {
  const root = createTestRoot()
  root.render(
    <div testId="outer" style={{ width: 300, height: 200, overflow: "scroll" }}>
      <div style={{ height: 100, flexShrink: 0 }} />
      <virtual-list
        testId="list"
        itemCount={20}
        estimatedItemHeight={40}
        style={{ width: 200, height: 100, ...options.list }}
      >
        {Array.from({ length: 20 }, (_, index) => (
          <div key={index} style={{ height: 40, flexShrink: 0 }}>
            <text>{`row-${index}`}</text>
          </div>
        ))}
      </virtual-list>
      <div style={{ width: 900, height: 600 }} />
    </div>
  )
  const { renderer } = root
  const id = (testId: string) => renderer.findByTestId(testId)!.id
  return {
    ...root,
    renderer,
    offset: (testId: string) => renderer.getScrollOffset(id(testId))!,
    scrollTo: (testId: string, x: number, y: number) => renderer.scrollTo(id(testId), x, y),
  }
}

describeNative("a virtual list in wheel routing", () => {
  it("scrolls itself first, like any innermost scroller", () => {
    const { renderer, offset, unmount } = listInScroller()
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("list")[1]).toBe(30)
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("chains to the scroller around it at its limit", () => {
    const { renderer, offset, scrollTo, unmount } = listInScroller()
    try {
      scrollTo("list", 0, 700)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("list")[1]).toBe(700)
      expect(offset("outer")).toEqual([0, 30])
    } finally {
      unmount()
    }
  })

  it("drops what it could not use, and the axis it does not scroll, until the next gesture", () => {
    const { renderer, offset, scrollTo, unmount } = listInScroller()
    try {
      scrollTo("list", 0, 690)
      renderer.nativeSimulateScrollWheel(50, 150, 20, 30)
      expect(offset("list")[1]).toBe(700)
      expect(offset("outer")).toEqual([0, 0])
      renderer.nativeSimulateScrollWheel(50, 150, 20, 30, undefined, "started")
      expect(offset("list")[1]).toBe(700)
      expect(offset("outer")).toEqual([20, 30])
    } finally {
      unmount()
    }
  })

  it("keeps a gesture and its momentum on the list", () => {
    const { renderer, offset, scrollTo, unmount } = listInScroller()
    try {
      scrollTo("list", 0, 690)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 5, undefined, "started")
      for (let frame = 0; frame < 10; frame++) {
        renderer.nativeSimulateScrollWheel(50, 150, 0, 60, undefined, "moved")
      }
      expect(offset("list")[1]).toBe(700)
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("keeps the delta with overscroll-behavior contain", () => {
    const { renderer, offset, scrollTo, unmount } = listInScroller({
      list: { overscrollBehavior: "contain" },
    })
    try {
      scrollTo("list", 0, 700)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("applies several ticks between frames in full", () => {
    const { renderer, offset, unmount } = listInScroller()
    try {
      // Several events between frames: each is applied once, relative to where the frame began.
      renderer.dispatchScrollWheel(50, 150, 0, 10)
      renderer.dispatchScrollWheel(50, 150, 0, 10)
      renderer.dispatchScrollWheel(50, 150, 0, 10)
      renderer.flush()
      expect(offset("list")[1]).toBe(30)
    } finally {
      unmount()
    }
  })
})
