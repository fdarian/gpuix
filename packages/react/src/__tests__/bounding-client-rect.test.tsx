import { useLayoutEffect, useRef, useState } from "react"
import { describe, expect, it } from "vitest"
import type { BoundingClientRect, PublicInstance } from "../index"
import { motion } from "../index"
import { flushSync } from "../reconciler/reconciler"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const ZERO: BoundingClientRect = {
  x: 0,
  y: 0,
  width: 0,
  height: 0,
  top: 0,
  left: 0,
  right: 0,
  bottom: 0,
}

describeNative("getBoundingClientRect", () => {
  it("answers in the commit that mounts the element", () => {
    const { render, unmount } = createTestRoot()
    try {
      let rect: BoundingClientRect | undefined
      function App() {
        const ref = useRef<PublicInstance>(null)
        useLayoutEffect(() => {
          rect = ref.current!.getBoundingClientRect()
        }, [])
        return (
          <div style={{ padding: 10 }}>
            <div ref={ref} style={{ width: 120, height: 40 }} />
          </div>
        )
      }
      render(<App />)

      expect(rect).toEqual({
        x: 10,
        y: 10,
        width: 120,
        height: 40,
        left: 10,
        top: 10,
        right: 130,
        bottom: 50,
      })
    } finally {
      unmount()
    }
  })

  it("reflects a style change made earlier in the same turn", () => {
    const { render, unmount } = createTestRoot()
    try {
      let setWidth!: (width: number) => void
      let element: PublicInstance | null = null
      function App() {
        const [width, set] = useState(50)
        setWidth = set
        return (
          <div
            ref={(instance) => {
              element = instance
            }}
            style={{ width, height: 20 }}
          />
        )
      }
      render(<App />)
      expect(element!.getBoundingClientRect().width).toBe(50)

      flushSync(() => setWidth(130))
      expect(element!.getBoundingClientRect().width).toBe(130)
    } finally {
      unmount()
    }
  })

  it("is the border box, for an input as for a div", () => {
    const { render, unmount } = createTestRoot()
    try {
      const style = { width: 100, height: 30, borderWidth: 3, borderColor: "#ffffff" }
      let div: PublicInstance | null = null
      let input: PublicInstance | null = null
      render(
        <div style={{ flexDirection: "column" }}>
          <div
            ref={(instance) => {
              div = instance
            }}
            style={style}
          />
          <input
            ref={(instance) => {
              input = instance
            }}
            style={style}
          />
        </div>
      )

      const box = (rect: BoundingClientRect) => [rect.width, rect.height]
      expect(box(div!.getBoundingClientRect())).toEqual([100, 30])
      expect(box(input!.getBoundingClientRect())).toEqual([100, 30])
      expect(input!.getBoundingClientRect().x).toBe(div!.getBoundingClientRect().x)
    } finally {
      unmount()
    }
  })

  it("reports a zero rect for an element without a box, and null from the raw renderer", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      let hidden: PublicInstance | null = null
      render(
        <div
          ref={(instance) => {
            hidden = instance
          }}
          style={{ display: "none", width: 50, height: 50 }}
        />
      )

      expect(hidden!.getBoundingClientRect()).toEqual(ZERO)
      expect(renderer.getElementBounds(hidden!.id)).toBeNull()
    } finally {
      unmount()
    }
  })

  it("reports the layout box of a visibility: hidden element and of its descendants", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      let hidden: PublicInstance | null = null
      let inner: PublicInstance | null = null
      render(
        <div style={{ padding: 10 }}>
          <div
            ref={(instance) => {
              hidden = instance
            }}
            style={{ visibility: "hidden", width: 50, height: 40, padding: 5 }}
          >
            <div
              ref={(instance) => {
                inner = instance
              }}
              style={{ width: 20, height: 10 }}
            />
          </div>
        </div>
      )

      expect(hidden!.getBoundingClientRect()).toMatchObject({ x: 10, y: 10, width: 50, height: 40 })
      expect(inner!.getBoundingClientRect()).toMatchObject({ x: 15, y: 15, width: 20, height: 10 })
      expect(renderer.getElementBounds(hidden!.id)).toMatchObject({ width: 50, height: 40 })
    } finally {
      unmount()
    }
  })

  it("keeps a child at the same offset from a parent that is moving", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      let parent: PublicInstance | null = null
      let child: PublicInstance | null = null
      render(
        <div style={{ flexDirection: "column" }}>
          <motion.div
            ref={(instance: PublicInstance | null) => {
              parent = instance
            }}
            initial={{ top: 0 }}
            animate={{ top: 100 }}
            transition={{ duration: 1, ease: "linear" }}
            style={{ position: "relative", width: 80, height: 80 }}
          >
            <div
              ref={(instance) => {
                child = instance
              }}
              style={{ width: 20, height: 20, marginTop: 12 }}
            />
          </motion.div>
        </div>
      )
      renderer.clockPause()
      renderer.clockFastForward(300)

      const offsets: number[] = []
      const tops: number[] = []
      // Each pair is read in one turn; the animation clock keeps running between turns.
      for (let step = 0; step < 4; step++) {
        const parentRect = parent!.getBoundingClientRect()
        const childRect = child!.getBoundingClientRect()
        tops.push(parentRect.top)
        offsets.push(childRect.top - parentRect.top)
        renderer.clockFastForward(100)
      }

      expect(new Set(offsets).size).toBe(1)
      expect(tops[0]).toBeGreaterThan(0)
      expect(tops[3]!).toBeGreaterThan(tops[0]!)
    } finally {
      unmount()
    }
  })
})
