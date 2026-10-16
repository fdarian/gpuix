import { describe, expect, it } from "vitest"
import type { ReactNode } from "react"
import type { ScrollEvent } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function setup(tree: ReactNode) {
  const root = createTestRoot()
  root.render(tree)
  const id = (testId: string) => root.renderer.findByTestId(testId)!.id
  return {
    ...root,
    id,
    offset: (testId: string) => root.renderer.getScrollOffset(id(testId))!,
    press: (testId: string, keys: string) =>
      root.renderer.nativeSimulateKeystrokes(id(testId), keys),
  }
}

function scroller(props: { onScroll?: (event: ScrollEvent) => void; onKeyDown?: (event: any) => void } = {}) {
  return (
    <div
      testId="scroller"
      tabIndex={0}
      style={{ width: 200, height: 100, overflow: "auto" }}
      onScroll={props.onScroll}
      onKeyDown={props.onKeyDown}
    >
      <div style={{ height: 1000 }} />
    </div>
  )
}

describeNative("keyboard scrolling", () => {
  it("scrolls 40px on the arrow keys", () => {
    const { press, offset, unmount } = setup(scroller())
    try {
      press("scroller", "ArrowDown")
      expect(offset("scroller")).toEqual([0, 40])
      press("scroller", "ArrowDown")
      expect(offset("scroller")).toEqual([0, 80])
      press("scroller", "ArrowUp")
      expect(offset("scroller")).toEqual([0, 40])
    } finally {
      unmount()
    }
  })

  it("scrolls 90% of the client size on PageDown, PageUp and Space", () => {
    const { press, offset, unmount } = setup(scroller())
    try {
      press("scroller", "PageDown")
      expect(offset("scroller")).toEqual([0, 90])
      press("scroller", "PageUp")
      expect(offset("scroller")).toEqual([0, 0])
      press("scroller", "Space")
      expect(offset("scroller")).toEqual([0, 90])
      press("scroller", "Shift+Space")
      expect(offset("scroller")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("jumps to the ends on Home and End", () => {
    const { press, offset, unmount } = setup(scroller())
    try {
      press("scroller", "End")
      expect(offset("scroller")).toEqual([0, 900])
      press("scroller", "Home")
      expect(offset("scroller")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("scrolls sideways on left and right", () => {
    const { press, offset, unmount } = setup(
      <div
        testId="scroller"
        tabIndex={0}
        style={{ width: 200, height: 100, overflow: "auto" }}
      >
        <div style={{ width: 800, height: 400 }} />
      </div>
    )
    try {
      press("scroller", "ArrowRight")
      expect(offset("scroller")).toEqual([40, 0])
      press("scroller", "ArrowLeft")
      expect(offset("scroller")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("scrolls the scroller around a focused descendant", () => {
    const { press, offset, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "auto" }}>
        <div testId="inside" tabIndex={0} style={{ height: 1000 }} />
      </div>
    )
    try {
      press("inside", "ArrowDown")
      expect(offset("scroller")).toEqual([0, 40])
    } finally {
      unmount()
    }
  })

  it("scrolls the innermost scroller that can move, then the one around it", () => {
    const { press, offset, unmount } = setup(
      <div testId="outer" style={{ width: 300, height: 200, overflow: "auto" }}>
        <div testId="inner" tabIndex={0} style={{ width: 200, height: 100, overflow: "auto" }}>
          <div style={{ height: 130 }} />
        </div>
        <div style={{ height: 800 }} />
      </div>
    )
    try {
      press("inner", "ArrowDown")
      expect(offset("inner")).toEqual([0, 30])
      expect(offset("outer")).toEqual([0, 0])
      press("inner", "ArrowDown")
      expect(offset("inner")).toEqual([0, 30])
      expect(offset("outer")).toEqual([0, 40])
    } finally {
      unmount()
    }
  })

  it("does nothing once an onKeyDown handler prevents the default", () => {
    const { press, offset, unmount } = setup(
      scroller({ onKeyDown: (event) => event.preventDefault() })
    )
    try {
      press("scroller", "ArrowDown")
      expect(offset("scroller")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("scrolls even when a handler listens, if it does not prevent the default", () => {
    const keys: string[] = []
    const { press, offset, unmount } = setup(
      scroller({ onKeyDown: (event) => keys.push(event.key) })
    )
    try {
      press("scroller", "ArrowDown")
      expect(keys).toEqual(["ArrowDown"])
      expect(offset("scroller")).toEqual([0, 40])
    } finally {
      unmount()
    }
  })

  it("leaves a focused text input its keys", () => {
    const { press, offset, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "auto" }}>
        <input testId="field" style={{ width: 150, height: 24 }} />
        <div style={{ height: 1000 }} />
      </div>
    )
    try {
      press("field", "Space")
      press("field", "PageDown")
      expect(offset("scroller")).toEqual([0, 0])
    } finally {
      unmount()
    }
  })

  it("leaves Space to a focused control that handles clicks", () => {
    const { press, offset, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "auto" }}>
        <div testId="button" tabIndex={0} onClick={() => {}} style={{ height: 30 }} />
        <div style={{ height: 1000 }} />
      </div>
    )
    try {
      press("button", "Space")
      expect(offset("scroller")).toEqual([0, 0])
      press("button", "ArrowDown")
      expect(offset("scroller")).toEqual([0, 40])
    } finally {
      unmount()
    }
  })

  it("scrolls the scroller under the pointer when nothing is focused", () => {
    const { renderer, offset, unmount } = setup(scroller())
    try {
      renderer.nativeSimulateMouseMove(50, 50)
      renderer.simulateKeystrokes("ArrowDown")
      expect(offset("scroller")).toEqual([0, 40])
    } finally {
      unmount()
    }
  })

  it("reports the scroll to onScroll", () => {
    const tops: number[] = []
    const { press, unmount } = setup(scroller({ onScroll: (event) => tops.push(event.scrollTop) }))
    try {
      press("scroller", "ArrowDown")
      expect(tops).toEqual([40])
    } finally {
      unmount()
    }
  })
})
