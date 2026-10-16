import { describe, expect, it } from "vitest"
import type { ReactNode } from "react"
import type { TestRenderer } from "../testing.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function setup(tree: ReactNode) {
  const root = createTestRoot()
  root.render(tree)
  return root
}

function metrics(renderer: TestRenderer, testId: string) {
  const id = renderer.findByTestId(testId)!.id
  const [scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight] =
    renderer.getScrollMetrics(id)!
  return { scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight }
}

function bounds(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
}

describeNative("scrollable extent follows CSS scrollable overflow", () => {
  it("lets a padded scroller scroll by its content and padding once, not twice", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{ width: 200, height: 100, padding: 20, overflow: "scroll" }}
      >
        <div style={{ height: 300 }} />
      </div>
    )
    try {
      // 20 padding above the content, the content, 20 padding below it.
      expect(metrics(renderer, "scroller").scrollHeight).toBe(340)
      const id = renderer.findByTestId("scroller")!.id
      renderer.scrollTo(id, 0, 10_000)
      expect(renderer.getScrollOffset(id)).toEqual([0, 240])
    } finally {
      unmount()
    }
  })

  it("never scrolls content that fits", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{ width: 200, height: 100, padding: 20, overflow: "scroll" }}
      >
        <div style={{ height: 40 }} />
      </div>
    )
    try {
      const id = renderer.findByTestId("scroller")!.id
      renderer.nativeSimulateScrollWheel(100, 50, 0, 80)
      expect(renderer.getScrollOffset(id)).toEqual([0, 0])
      expect(metrics(renderer, "scroller").scrollHeight).toBe(100)
    } finally {
      unmount()
    }
  })

  it("never scrolls an empty scroller", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{ width: 200, height: 100, padding: 30, overflow: "scroll" }}
      />
    )
    try {
      const id = renderer.findByTestId("scroller")!.id
      renderer.nativeSimulateScrollWheel(100, 50, 0, 80)
      expect(renderer.getScrollOffset(id)).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("lets an absolute child at the origin leave the scroll range alone", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{ width: 200, height: 100, padding: 20, overflow: "scroll" }}
      >
        <div style={{ position: "absolute", top: 0, left: 0, width: 50, height: 50 }} />
      </div>
    )
    try {
      expect(metrics(renderer, "scroller").scrollHeight).toBe(100)
    } finally {
      unmount()
    }
  })

  it("does not let the scrollbar overlay count as content", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{
          width: 200,
          height: 100,
          padding: 20,
          overflow: "scroll",
          scrollbarColor: "#888888",
        }}
      >
        <div style={{ height: 30 }} />
      </div>
    )
    try {
      expect(metrics(renderer, "scroller")).toMatchObject({
        scrollHeight: 100,
        clientHeight: 100,
      })
    } finally {
      unmount()
    }
  })

  it("keeps a bordered scroller's client area inside its border", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{
          width: 200,
          height: 100,
          borderWidth: 4,
          borderStyle: "solid",
          borderColor: "#ffffff",
          overflow: "scroll",
        }}
      >
        <div style={{ height: 300 }} />
      </div>
    )
    try {
      expect(metrics(renderer, "scroller")).toMatchObject({
        clientWidth: 192,
        clientHeight: 92,
        scrollHeight: 300,
      })
    } finally {
      unmount()
    }
  })
})

describeNative("overflow never changes display", () => {
  it("lays out an x-only scroller as the block it is, and lets a wide child overflow", () => {
    const { renderer, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflowX: "scroll" }}>
        <div testId="a" style={{ width: 500, height: 30 }} />
        <div testId="b" style={{ width: 500, height: 30 }} />
      </div>
    )
    try {
      // Block layout stacks the children. A forced flex row would put them side by side.
      expect(bounds(renderer, "b").y).toBe(bounds(renderer, "a").y + 30)
      expect(metrics(renderer, "scroller")).toMatchObject({
        scrollWidth: 500,
        clientWidth: 200,
      })
    } finally {
      unmount()
    }
  })

  it("scrolls a wide child of a two-axis scroller", () => {
    const { renderer, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div style={{ width: 500, height: 300 }} />
      </div>
    )
    try {
      expect(metrics(renderer, "scroller")).toMatchObject({
        scrollWidth: 500,
        scrollHeight: 300,
      })
    } finally {
      unmount()
    }
  })

  it("scrolls the row of a flex scroller without wrapping its children", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{
          display: "flex",
          flexDirection: "row",
          width: 200,
          height: 100,
          overflowX: "scroll",
        }}
      >
        <div testId="a" style={{ width: 150, height: 30, flexShrink: 0 }} />
        <div testId="b" style={{ width: 150, height: 30, flexShrink: 0 }} />
      </div>
    )
    try {
      expect(bounds(renderer, "b").x).toBe(bounds(renderer, "a").x + 150)
      expect(metrics(renderer, "scroller").scrollWidth).toBe(300)
    } finally {
      unmount()
    }
  })

  it("lets a flex scroller's own flex item shrink below its content, as CSS does", () => {
    const { renderer, unmount } = setup(
      <div style={{ display: "flex", flexDirection: "row", width: 300, height: 100 }}>
        <div
          testId="scroller"
          style={{ flexGrow: 1, height: 100, overflowX: "scroll" }}
        >
          <div style={{ width: 600, height: 30 }} />
        </div>
        <div testId="sibling" style={{ width: 100, height: 100, flexShrink: 0 }} />
      </div>
    )
    try {
      expect(bounds(renderer, "scroller").width).toBe(200)
      expect(metrics(renderer, "scroller").scrollWidth).toBe(600)
    } finally {
      unmount()
    }
  })
})

describeNative("absolute children of a scroller", () => {
  it("scroll with the content", () => {
    const { renderer, unmount } = setup(
      <div testId="scroller" style={{ position: "relative", width: 200, height: 100, overflow: "scroll" }}>
        <div style={{ height: 400 }} />
        <div
          testId="pinned"
          style={{ position: "absolute", top: 100, left: 0, width: 50, height: 20 }}
        />
      </div>
    )
    try {
      const before = bounds(renderer, "pinned").y
      renderer.scrollTo(renderer.findByTestId("scroller")!.id, 0, 50)
      expect(bounds(renderer, "pinned").y).toBe(before - 50)
    } finally {
      unmount()
    }
  })

  it("extend what the scroller can scroll to", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{ position: "relative", width: 200, height: 100, overflow: "scroll" }}
      >
        <div
          style={{ position: "absolute", top: 250, left: 0, width: 50, height: 20 }}
        />
      </div>
    )
    try {
      expect(metrics(renderer, "scroller").scrollHeight).toBe(270)
    } finally {
      unmount()
    }
  })

  it("do not extend it when the scroller is not their containing block", () => {
    const { renderer, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div
          style={{ position: "absolute", top: 250, left: 0, width: 50, height: 20 }}
        />
      </div>
    )
    try {
      expect(metrics(renderer, "scroller").scrollHeight).toBe(100)
    } finally {
      unmount()
    }
  })
})

describeNative("overflow values", () => {
  it("scrolls on auto exactly like scroll", () => {
    const { renderer, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "auto" }}>
        <div style={{ height: 400 }} />
      </div>
    )
    try {
      const id = renderer.findByTestId("scroller")!.id
      renderer.nativeSimulateScrollWheel(100, 50, 0, 40)
      expect(renderer.getScrollOffset(id)).toEqual([0, 40])
      expect(metrics(renderer, "scroller").scrollHeight).toBe(400)
    } finally {
      unmount()
    }
  })

  it("scrolls only the axis that says auto", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        style={{ width: 200, height: 100, overflowX: "hidden", overflowY: "auto" }}
      >
        <div style={{ width: 500, height: 400 }} />
      </div>
    )
    try {
      const id = renderer.findByTestId("scroller")!.id
      renderer.nativeSimulateScrollWheel(100, 50, 30, 40)
      expect(renderer.getScrollOffset(id)).toEqual([0, 40])
    } finally {
      unmount()
    }
  })

  it.each(["clip", "visible"])("is not a scroll container on %s", (overflow) => {
    const { renderer, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow }}>
        <div style={{ height: 400 }} />
      </div>
    )
    try {
      renderer.nativeSimulateScrollWheel(100, 50, 0, 40)
      const id = renderer.findByTestId("scroller")!.id
      expect(renderer.getScrollOffset(id)).toBeNull()
    } finally {
      unmount()
    }
  })

  it("is a scroll container that only code scrolls on hidden", () => {
    const { renderer, unmount } = setup(
      <div
        testId="scroller"
        tabIndex={0}
        style={{ width: 200, height: 100, padding: 10, overflow: "hidden" }}
      >
        <div style={{ height: 400 }} />
      </div>
    )
    try {
      const id = renderer.findByTestId("scroller")!.id
      // The same extent as any scroller: 10 + 400 + 10 padding and the 10 above.
      expect(metrics(renderer, "scroller")).toMatchObject({ scrollHeight: 420, clientHeight: 100 })

      renderer.nativeSimulateScrollWheel(100, 50, 0, 40)
      expect(renderer.getScrollOffset(id)).toEqual([0, 0])
      renderer.nativeSimulateKeystrokes(id, "ArrowDown")
      renderer.nativeSimulateKeystrokes(id, "PageDown")
      expect(renderer.getScrollOffset(id)).toEqual([0, 0])

      renderer.scrollTo(id, 0, 60)
      expect(renderer.getScrollOffset(id)).toEqual([0, 60])
      renderer.scrollTo(id, 0, 10_000)
      expect(renderer.getScrollOffset(id)).toEqual([0, 320])
    } finally {
      unmount()
    }
  })

  it("lets a wheel over hidden go to the scroller around it", () => {
    const { renderer, unmount } = setup(
      <div testId="outer" style={{ width: 200, height: 100, overflow: "auto" }}>
        <div style={{ width: 200, height: 100, overflow: "hidden" }}>
          <div style={{ height: 400 }} />
        </div>
        <div style={{ height: 400 }} />
      </div>
    )
    try {
      renderer.nativeSimulateScrollWheel(100, 50, 0, 40)
      expect(renderer.getScrollOffset(renderer.findByTestId("outer")!.id)).toEqual([0, 40])
    } finally {
      unmount()
    }
  })

  it("scrolls a hidden element into view for a descendant", () => {
    const { renderer, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "hidden" }}>
        <div style={{ height: 300 }} />
        <div testId="target" style={{ height: 20 }} />
      </div>
    )
    try {
      renderer.scrollIntoView(renderer.findByTestId("target")!.id, "start")
      expect(renderer.getScrollOffset(renderer.findByTestId("scroller")!.id)).toEqual([0, 220])
    } finally {
      unmount()
    }
  })

  it("clips what lies outside on clip, like hidden", () => {
    const { renderer, unmount } = setup(
      <div style={{ width: 300, height: 300 }}>
        <div testId="clipped" style={{ width: 100, height: 100, overflow: "clip" }}>
          <div testId="inside" style={{ width: 400, height: 20 }} />
        </div>
      </div>
    )
    try {
      const clipped = bounds(renderer, "clipped")
      const outside = renderer.getElementsAt(clipped.x + clipped.width + 50, clipped.y + 10)
      expect(outside).not.toContain(renderer.findByTestId("inside")!.id)
    } finally {
      unmount()
    }
  })
})

describeNative("overflow beside a visible axis", () => {
  const hitsAt = (renderer: TestRenderer, dx: number, dy: number) => {
    const clip = bounds(renderer, "clip")
    return renderer.getElementsAt(clip.x + dx, clip.y + dy)
  }

  function spill(style: Record<string, string>) {
    return setup(
      <div style={{ width: 400, height: 400 }}>
        <div testId="clip" style={{ width: 100, height: 100, ...style }}>
          <div testId="inside" style={{ width: 300, height: 300 }} />
        </div>
      </div>
    )
  }

  it("clips only x for overflowX clip: the child is hit below the box, not beside it", () => {
    const { renderer, unmount } = spill({ overflowX: "clip", overflowY: "visible" })
    try {
      const inside = renderer.findByTestId("inside")!.id
      expect(hitsAt(renderer, 50, 200)).toContain(inside)
      expect(hitsAt(renderer, 200, 50)).not.toContain(inside)
    } finally {
      unmount()
    }
  })

  it("clips only y for overflowY clip: the child is hit beside the box, not below it", () => {
    const { renderer, unmount } = spill({ overflowX: "visible", overflowY: "clip" })
    try {
      const inside = renderer.findByTestId("inside")!.id
      expect(hitsAt(renderer, 200, 50)).toContain(inside)
      expect(hitsAt(renderer, 50, 200)).not.toContain(inside)
    } finally {
      unmount()
    }
  })

  it.each(["hidden", "scroll", "auto"])(
    "clips both axes for %s beside visible, which computes to auto",
    (scrolling) => {
      const { renderer, unmount } = spill({ overflowX: scrolling, overflowY: "visible" })
      try {
        const inside = renderer.findByTestId("inside")!.id
        expect(hitsAt(renderer, 200, 50)).not.toContain(inside)
        expect(hitsAt(renderer, 50, 200)).not.toContain(inside)
        // The visible axis became a scroll container axis too.
        expect(metrics(renderer, "clip").scrollHeight).toBe(300)
      } finally {
        unmount()
      }
    }
  )
})

describeNative("simulated wheel sign", () => {
  it("scrolls down on a positive deltaY and right on a positive deltaX, like WheelEvent", () => {
    const { renderer, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div style={{ width: 600, height: 400 }} />
      </div>
    )
    try {
      renderer.nativeSimulateScrollWheel(100, 50, 0, 60)
      expect(metrics(renderer, "scroller").scrollTop).toBe(60)
      renderer.nativeSimulateScrollWheel(100, 50, 0, -20)
      expect(metrics(renderer, "scroller").scrollTop).toBe(40)
      renderer.nativeSimulateScrollWheel(100, 50, 30, 0)
      expect(metrics(renderer, "scroller").scrollLeft).toBe(30)
    } finally {
      unmount()
    }
  })
})
