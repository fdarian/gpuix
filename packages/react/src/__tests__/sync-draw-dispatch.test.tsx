/**
 * A handler that changes the tree and then reads layout (`getBoundingClientRect`, `scrollTop`) makes
 * the window lay out while the input that called the handler is still being dispatched. The read
 * must see the handler's own change, and the input must still find what it was dispatched to.
 */

import { useRef, useState } from "react"
import { describe, expect, it } from "vitest"
import type { BoundingClientRect, PublicInstance } from "../index"
import { flushSync } from "../reconciler/reconciler"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("layout reads inside an input handler", () => {
  it("reads the neighbour of a dialog its own keydown unmounted, and the key still lands", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const seen: BoundingClientRect[] = []
      const keys: string[] = []
      function App() {
        const [open, setOpen] = useState(true)
        const neighbour = useRef<PublicInstance>(null)
        return (
          <div style={{ width: 300, height: 200 }}>
            {open && (
              <div
                testId="dialog"
                tabIndex={0}
                style={{ width: 100, height: 50 }}
                onKeyDown={(event) => {
                  keys.push(event.key)
                  flushSync(() => setOpen(false))
                  seen.push(neighbour.current!.getBoundingClientRect())
                }}
              />
            )}
            <div ref={neighbour} testId="neighbour" style={{ width: 100, height: 20 }} />
          </div>
        )
      }
      render(<App />)
      const neighbourId = renderer.findByTestId("neighbour").id
      expect(renderer.getElementBounds(neighbourId)?.y).toBe(50)

      renderer.nativeSimulateKeystrokes(renderer.findByTestId("dialog").id, "Escape")

      expect(keys).toEqual(["Escape"])
      expect(seen).toHaveLength(1)
      expect(seen[0]).toMatchObject({ y: 0, height: 20 })
      expect(renderer.findByTestId("dialog")).toBeUndefined()
      expect(renderer.getElementBounds(neighbourId)).toMatchObject({ y: 0, height: 20 })
    } finally {
      unmount()
    }
  })

  it("reads a size the handler's own style change gave", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const widths: number[] = []
      function App() {
        const [width, setWidth] = useState(100)
        const box = useRef<PublicInstance>(null)
        return (
          <div
            testId="field"
            tabIndex={0}
            style={{ width: 300, height: 100 }}
            onKeyDown={() => {
              widths.push(box.current!.getBoundingClientRect().width)
              flushSync(() => setWidth(250))
              widths.push(box.current!.getBoundingClientRect().width)
              flushSync(() => setWidth(40))
              widths.push(box.current!.getBoundingClientRect().width)
            }}
          >
            <div ref={box} style={{ width, height: 10 }} />
          </div>
        )
      }
      render(<App />)

      renderer.nativeSimulateKeystrokes(renderer.findByTestId("field").id, "a")

      expect(widths).toEqual([100, 250, 40])
    } finally {
      unmount()
    }
  })

  it("keeps a scroller scrolling by the wheel after a wheel handler read its scrollTop", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const seen: number[] = []
      function App() {
        const [wheels, setWheels] = useState(0)
        const scroller = useRef<PublicInstance>(null)
        return (
          <div
            ref={scroller}
            testId="scroller"
            style={{ width: 200, height: 100, overflow: "scroll" }}
            onWheel={() => {
              flushSync(() => setWheels((count) => count + 1))
              seen.push(scroller.current!.scrollTop)
            }}
          >
            <div style={{ width: 200, height: 500, flexShrink: 0 }}>
              <text>{`wheels ${wheels}`}</text>
            </div>
          </div>
        )
      }
      render(<App />)
      const scrollerId = renderer.findByTestId("scroller").id
      const scrollTop = () => -(renderer.getScrollOffset(scrollerId)?.[1] ?? 0)

      renderer.nativeSimulateScrollWheel(100, 50, 0, -30)
      expect(scrollTop()).toBe(30)
      renderer.nativeSimulateScrollWheel(100, 50, 0, -30)
      expect(scrollTop()).toBe(60)
      renderer.nativeSimulateScrollWheel(100, 50, 0, -30)
      expect(scrollTop()).toBe(90)

      expect(seen).toEqual([0, 30, 60])
    } finally {
      unmount()
    }
  })

  it("still focuses an input and types into it when the mousedown handler changed the tree and read layout", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const rects: BoundingClientRect[] = []
      function App() {
        const [presses, setPresses] = useState(0)
        const neighbour = useRef<PublicInstance>(null)
        return (
          <div
            style={{ width: 400, height: 200 }}
            onMouseDown={() => {
              flushSync(() => setPresses((count) => count + 1))
              rects.push(neighbour.current!.getBoundingClientRect())
            }}
          >
            <input testId="field" defaultValue="hello" style={{ width: 200, height: 30 }} />
            <div
              ref={neighbour}
              testId="neighbour"
              style={{ width: 100, height: 20 + presses * 10 }}
            />
          </div>
        )
      }
      render(<App />)
      const field = renderer.findByTestId("field")
      const bounds = renderer.getElementBounds(field.id)!
      expect(renderer.getFocusedElementId()).toBeNull()

      renderer.nativeSimulateClick(bounds.x + bounds.width - 5, bounds.y + 15)

      expect(rects).toHaveLength(1)
      expect(rects[0]).toMatchObject({ height: 30 })
      expect(renderer.getFocusedElementId()).toBe(field.id)
      renderer.simulateKeystrokes("a")
      expect(renderer.getTextSelection(field.id)).toMatchObject({ start: 1, end: 1 })
    } finally {
      unmount()
    }
  })
})
