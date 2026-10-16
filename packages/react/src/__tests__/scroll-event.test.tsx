import { describe, expect, it } from "vitest"
import type { ScrollEvent } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** `onScroll` reports the offset the way the DOM reads it: pixels from the start. */
const offset = (event: ScrollEvent) => [event.scrollLeft, event.scrollTop]

function scroller(onScroll?: (event: ScrollEvent) => void) {
  return (
    <div
      testId="scroller"
      style={{ width: 200, height: 100, overflow: "scroll" }}
      onScroll={onScroll}
    >
      <div style={{ height: 500 }} />
    </div>
  )
}

describeNative("onScroll", () => {
  it("reports movement, not the position it starts at", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const offsets: unknown[] = []
      render(scroller((event) => offsets.push(offset(event))))
      renderer.dispatchNativeEvents()
      expect(offsets, "a first paint is not a scroll").toEqual([])

      const id = renderer.findByTestId("scroller")!.id
      renderer.scrollTo(id, 0, 100)
      renderer.dispatchNativeEvents()
      expect(offsets).toEqual([[0, 100]])
      expect(renderer.getScrollOffset(id)).toEqual([0, 100])
    } finally {
      unmount()
    }
  })

  it("reports a wheel scroll with the offset it leaves", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const offsets: number[][] = []
      render(scroller((event) => offsets.push(offset(event) as number[])))
      renderer.nativeSimulateScrollWheel(100, 50, 0, 40)

      expect(offsets).toHaveLength(1)
      const id = renderer.findByTestId("scroller")!.id
      expect(offsets[0]).toEqual(renderer.getScrollOffset(id))
      expect(offsets[0]![1]).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })

  it("stays quiet while the offset holds, and for sub-pixel noise", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const offsets: unknown[] = []
      const app = () => scroller((event) => offsets.push(offset(event)))
      render(app())
      const id = renderer.findByTestId("scroller")!.id
      renderer.scrollTo(id, 0, 30)
      renderer.dispatchNativeEvents()
      expect(offsets).toHaveLength(1)

      render(app())
      renderer.scrollTo(id, 0, 30.004)
      renderer.dispatchNativeEvents()
      expect(offsets).toHaveLength(1)
    } finally {
      unmount()
    }
  })

  it("starts from where the element already is when the listener arrives late", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const offsets: unknown[] = []
      render(scroller())
      const id = renderer.findByTestId("scroller")!.id
      renderer.scrollTo(id, 0, 80)

      render(scroller((event) => offsets.push(offset(event))))
      renderer.dispatchNativeEvents()
      expect(offsets).toEqual([])

      renderer.scrollTo(id, 0, 120)
      renderer.dispatchNativeEvents()
      expect(offsets).toEqual([[0, 120]])
    } finally {
      unmount()
    }
  })

  it("reports a scroll that follows scrollIntoView", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const offsets: unknown[] = []
      render(
        <div
          testId="scroller"
          style={{ width: 200, height: 100, overflow: "scroll" }}
          onScroll={(event) => offsets.push(offset(event))}
        >
          <div style={{ height: 400 }} />
          <div testId="target" style={{ height: 20 }} />
        </div>
      )
      renderer.dispatchNativeEvents()
      expect(offsets).toEqual([])

      renderer.scrollIntoView(renderer.findByTestId("target")!.id)
      renderer.dispatchNativeEvents()
      expect(offsets).toHaveLength(1)
      expect((offsets[0] as number[])[1]).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })

  it("ignores an element that does not scroll", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const offsets: unknown[] = []
      render(
        <div
          style={{ width: 200, height: 100 }}
          onScroll={(event) => offsets.push(offset(event))}
        />
      )
      renderer.dispatchNativeEvents()
      expect(offsets).toEqual([])
    } finally {
      unmount()
    }
  })
})
