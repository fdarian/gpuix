import { describe, expect, it } from "vitest"
import type { StyleDesc } from "@gpuix/native/host"
import type { WheelEvent } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/**
 * An outer scroller (300 x 200, 900 x 800 inside) holding a 100 px spacer, an inner
 * scroller (200 x 100, 400 tall inside, so 300 to scroll) and a filler. The inner scroller
 * occupies y 100 to 200 of the outer's viewport, so (50, 150) is over both.
 */
function nested(options: {
  inner?: StyleDesc
  outer?: StyleDesc
  onOuterWheel?: (event: WheelEvent) => void
}) {
  const root = createTestRoot()
  root.render(
    <div
      testId="outer"
      style={{ width: 300, height: 200, overflow: "scroll", ...options.outer }}
      onWheel={options.onOuterWheel}
    >
      <div style={{ height: 100, flexShrink: 0 }} />
      <div
        testId="inner"
        style={{ width: 200, height: 100, overflow: "scroll", ...options.inner }}
      >
        <div style={{ width: 200, height: 400 }} />
      </div>
      <div style={{ width: 900, height: 600 }} />
    </div>
  )
  const renderer = root.renderer
  const id = (testId: string) => renderer.findByTestId(testId)!.id
  return {
    ...root,
    renderer,
    offset: (testId: string) => renderer.getScrollOffset(id(testId))!,
    scrollTo: (testId: string, x: number, y: number) => renderer.scrollTo(id(testId), x, y),
  }
}

describeNative("wheel routing", () => {
  it("scrolls the innermost scroller under the pointer", () => {
    const { renderer, offset, unmount } = nested({})
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("inner")).toEqual([0, 30])
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("chains to the ancestor once the inner scroller is at its limit", () => {
    const { renderer, offset, scrollTo, unmount } = nested({})
    try {
      scrollTo("inner", 0, 300)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("inner")).toEqual([0, 300])
      expect(offset("outer")).toEqual([0, 30])
    } finally {
      unmount()
    }
  })

  it("drops what the inner scroller could not use, and chains on the next gesture", () => {
    const { renderer, offset, scrollTo, unmount } = nested({})
    try {
      scrollTo("inner", 0, 290)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("inner")).toEqual([0, 300])
      expect(offset("outer")).toEqual([0, 0])
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30, undefined, "started")
      expect(offset("inner")).toEqual([0, 300])
      expect(offset("outer")).toEqual([0, 30])
    } finally {
      unmount()
    }
  })

  it("chains upward when scrolling back from the other limit", () => {
    const { renderer, offset, unmount } = nested({})
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, -30)
      expect(offset("inner")).toEqual([0, 0])
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("treats the axes independently", () => {
    const { renderer, offset, unmount } = nested({
      inner: { overflow: undefined, overflowY: "scroll" },
      outer: { overflow: "scroll" },
    })
    try {
      // The inner scroller only goes along y, but it takes the whole event: the x delta is
      // dropped, not given to the outer scroller.
      renderer.nativeSimulateScrollWheel(50, 150, 20, 30)
      expect(offset("inner")).toEqual([0, 30])
      expect(offset("outer")).toEqual([0, 0])
      // A sideways gesture finds nothing to scroll in the inner scroller, so the outer one has it.
      renderer.nativeSimulateScrollWheel(50, 150, 20, 0, undefined, "started")
      expect(offset("inner")).toEqual([0, 30])
      expect(offset("outer")).toEqual([20, 0])
    } finally {
      unmount()
    }
  })

  it("gives a tick to one scroller whole, and chains an axis it cannot use only on the next gesture", () => {
    const { renderer, offset, scrollTo, unmount } = nested({
      inner: { overflow: "scroll" },
    })
    try {
      // The inner scroller has no horizontal range, so a diagonal tick moves it down and the
      // sideways part is dropped.
      scrollTo("inner", 0, 100)
      renderer.nativeSimulateScrollWheel(50, 150, 20, 30)
      expect(offset("inner")).toEqual([0, 130])
      expect(offset("outer")).toEqual([0, 0])
      // At the bottom the inner scroller takes nothing, so the outer one has both parts.
      scrollTo("inner", 0, 300)
      renderer.nativeSimulateScrollWheel(50, 150, 10, 30, undefined, "started")
      expect(offset("inner")).toEqual([0, 300])
      expect(offset("outer")).toEqual([10, 30])
    } finally {
      unmount()
    }
  })

  it("keeps the delta in a scroller with overscroll-behavior contain", () => {
    const { renderer, offset, scrollTo, unmount } = nested({
      inner: { overscrollBehavior: "contain" },
    })
    try {
      scrollTo("inner", 0, 300)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("keeps the delta in a scroller with overscroll-behavior none, per axis", () => {
    const { renderer, offset, scrollTo, unmount } = nested({
      inner: { overscrollBehaviorY: "none" },
    })
    try {
      scrollTo("inner", 0, 300)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("lets an auto scroller chain even when the other axis contains", () => {
    const { renderer, offset, scrollTo, unmount } = nested({
      inner: { overscrollBehaviorX: "contain" },
    })
    try {
      scrollTo("inner", 0, 300)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("outer")).toEqual([0, 30])
    } finally {
      unmount()
    }
  })

  it("never leaves an offset out of range", () => {
    const { renderer, offset, unmount } = nested({})
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 5000)
      expect(offset("inner")).toEqual([0, 300])
      expect(offset("outer")).toEqual([0, 0])
      // The next gesture finds the inner scroller at its limit, and the outer one clamps too.
      renderer.nativeSimulateScrollWheel(50, 150, 0, 5000, undefined, "started")
      expect(offset("inner")).toEqual([0, 300])
      expect(offset("outer")).toEqual([0, 600])
    } finally {
      unmount()
    }
  })

  it("still runs wheel listeners on ancestors of the scroller that took the delta", () => {
    const wheels: WheelEvent[] = []
    const { renderer, offset, unmount } = nested({ onOuterWheel: (e) => wheels.push(e) })
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("inner")).toEqual([0, 30])
      expect(wheels).toHaveLength(1)
      expect(wheels[0]!.deltaY).toBe(30)
    } finally {
      unmount()
    }
  })

  it("scrolls only a scroller that can move in the direction of the delta", () => {
    const { renderer, offset, scrollTo, unmount } = nested({})
    try {
      scrollTo("inner", 0, 300)
      // Back up the inner scroller, which has room above, not the outer one.
      renderer.nativeSimulateScrollWheel(50, 150, 0, -30)
      expect(offset("inner")).toEqual([0, 270])
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })
})

describeNative("scroll latching", () => {
  it("keeps a gesture on the scroller that started it, momentum included", () => {
    const { renderer, offset, scrollTo, unmount } = nested({})
    try {
      scrollTo("inner", 0, 290)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 5, undefined, "started")
      renderer.nativeSimulateScrollWheel(50, 150, 0, 50, undefined, "moved")
      renderer.nativeSimulateScrollWheel(50, 150, 0, 50, undefined, "ended")
      renderer.nativeSimulateScrollWheel(50, 150, 0, 50, undefined, "moved")
      expect(offset("inner")).toEqual([0, 300])
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("does not let momentum hop to the parent when the inner scroller runs out", () => {
    const { renderer, offset, unmount } = nested({})
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 200, undefined, "started")
      renderer.nativeSimulateScrollWheel(50, 150, 0, 0, undefined, "ended")
      // Momentum frames follow the finger lift, well past the inner scroller's limit.
      for (let frame = 0; frame < 20; frame++) {
        renderer.nativeSimulateScrollWheel(50, 150, 0, 60, undefined, "moved")
      }
      expect(offset("inner")).toEqual([0, 300])
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("hands the next gesture to the ancestor when the inner scroller is at its limit", () => {
    const { renderer, offset, scrollTo, unmount } = nested({})
    try {
      scrollTo("inner", 0, 300)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30, undefined, "started")
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30, undefined, "moved")
      expect(offset("outer")).toEqual([0, 60])
      // The ancestor took the gesture, so the inner scroller is not retried.
      scrollTo("inner", 0, 100)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30, undefined, "moved")
      expect(offset("inner")).toEqual([0, 100])
      expect(offset("outer")).toEqual([0, 90])
    } finally {
      unmount()
    }
  })

  it("keeps the latch when the pointer leaves the scroller mid-gesture", () => {
    const { renderer, offset, unmount } = nested({})
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 10, undefined, "started")
      // (250, 50) is over the outer scroller only.
      renderer.nativeSimulateScrollWheel(250, 50, 0, 10, undefined, "moved")
      expect(offset("inner")).toEqual([0, 20])
      expect(offset("outer")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("treats every wheel call without a phase as a gesture of its own", () => {
    const { renderer, offset, unmount } = nested({})
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 400)
      expect(offset("inner")).toEqual([0, 300])
      renderer.nativeSimulateScrollWheel(250, 50, 0, 30)
      expect(offset("outer")).toEqual([0, 30])
    } finally {
      unmount()
    }
  })
})

describeNative("scrolling under inert and pointer-events: none", () => {
  type InnerProps = { inert?: boolean; pointerEvents?: "none" | "auto" }

  function Nested(props: InnerProps) {
    return (
      <div testId="outer" style={{ width: 300, height: 200, overflow: "scroll" }}>
        <div style={{ height: 100, flexShrink: 0 }} />
        <div
          testId="inner"
          inert={props.inert}
          style={{ width: 200, height: 100, overflow: "scroll", pointerEvents: props.pointerEvents }}
        >
          <div style={{ width: 200, height: 400 }} />
        </div>
        <div style={{ width: 900, height: 600 }} />
      </div>
    )
  }

  function setup(props: InnerProps) {
    const root = createTestRoot()
    root.render(<Nested {...props} />)
    const renderer = root.renderer
    const id = (testId: string) => renderer.findByTestId(testId)!.id
    return {
      ...root,
      renderer,
      offset: (testId: string) => renderer.getScrollOffset(id(testId))!,
      rerender: (next: InnerProps) => root.render(<Nested {...next} />),
    }
  }

  it.each([
    ["inert", { inert: true }],
    ["pointerEvents: none", { pointerEvents: "none" as const }],
  ])("scrolls the ancestor when the wheel is over a scroller with %s", (_name, props) => {
    const { renderer, offset, unmount } = setup(props)
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 30)
      expect(offset("inner")).toEqual([0, 0])
      expect(offset("outer")).toEqual([0, 30])
    } finally {
      unmount()
    }
  })

  it("keeps a scroller inside an inert subtree out of the wheel, pointerEvents: auto included", () => {
    const root = createTestRoot()
    root.render(
      <div testId="outer" style={{ width: 300, height: 200, overflow: "scroll" }}>
        <div inert style={{ width: 200, height: 100 }}>
          <div testId="inner" style={{ width: 200, height: 100, overflow: "scroll", pointerEvents: "auto" }}>
            <div style={{ width: 200, height: 400 }} />
          </div>
        </div>
        <div style={{ width: 900, height: 600 }} />
      </div>
    )
    const renderer = root.renderer
    try {
      renderer.nativeSimulateScrollWheel(50, 50, 0, 30)
      expect(renderer.getScrollOffset(renderer.findByTestId("inner")!.id)).toEqual([0, 0])
      expect(renderer.getScrollOffset(renderer.findByTestId("outer")!.id)).toEqual([0, 30])
    } finally {
      root.unmount()
    }
  })

  it.each([
    ["inert", { inert: true }],
    ["pointerEvents: none", { pointerEvents: "none" as const }],
  ])("does not keep a latched gesture on a scroller that became %s", (_name, props) => {
    const { renderer, offset, rerender, unmount } = setup({})
    try {
      renderer.nativeSimulateScrollWheel(50, 150, 0, 10, undefined, "started")
      expect(offset("inner")).toEqual([0, 10])

      rerender(props)
      renderer.nativeSimulateScrollWheel(50, 150, 0, 10, undefined, "moved")
      expect(offset("inner")).toEqual([0, 10])
      expect(offset("outer")).toEqual([0, 10])
    } finally {
      unmount()
    }
  })
})
