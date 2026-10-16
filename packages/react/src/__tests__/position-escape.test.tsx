/// `position: absolute` and `position: fixed` boxes are clipped and scrolled by the ancestors at or
/// inside their containing block, not by the static ones between: the dropdown that hangs out of
/// an `overflow: hidden` card. Every test drives the real paint and hit test through the GPU test
/// renderer.

import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("position escapes the overflow of its ancestors", () => {
  function setup() {
    const root = createTestRoot()
    const clicks: string[] = []
    const dropdown = (style: Record<string, unknown>) => (
      <div testId="dropdown" onClick={() => clicks.push("dropdown")} style={{ width: 150, height: 50, ...style }} />
    )
    const bounds = (testId: string) => root.renderer.getElementBounds(root.renderer.findByTestId(testId)!.id)!
    return { ...root, clicks, dropdown, bounds }
  }

  /** A 100 by 100 card at (200, 100) in a positioned block whose padding box starts at (0, 0). */
  const card = (style: Record<string, unknown>, children: ReactNode) => (
    <div style={{ position: "relative", width: 600, height: 400, paddingLeft: 200, paddingTop: 100 }}>
      <div testId="card" style={{ width: 100, height: 100, overflow: "hidden", ...style }}>
        {children}
      </div>
    </div>
  )

  it("shows and clicks an absolute dropdown outside a static overflow: hidden card", () => {
    const { render, renderer, clicks, dropdown, bounds, unmount } = setup()
    try {
      render(card({}, dropdown({ position: "absolute", left: 10, top: 10 })))
      renderer.dispatchNativeEvents()

      // The card spans (200, 100) to (300, 200); the dropdown is placed against the block.
      expect(bounds("dropdown")).toMatchObject({ x: 10, y: 10, width: 150, height: 50 })
      renderer.nativeSimulateClick(50, 30)
      expect(clicks).toEqual(["dropdown"])
    } finally {
      unmount()
    }
  })

  it("clips an absolute dropdown by an overflow: hidden card that is its containing block", () => {
    const { render, renderer, clicks, dropdown, unmount } = setup()
    try {
      render(card({ position: "relative" }, dropdown({ position: "absolute", left: 50, top: 50 })))
      renderer.dispatchNativeEvents()

      // Inside the card, the dropdown spans (250, 150) to (400, 200) and the card ends at 300.
      renderer.nativeSimulateClick(350, 170)
      expect(clicks).toEqual([])
      renderer.nativeSimulateClick(280, 180)
      expect(clicks).toEqual(["dropdown"])
    } finally {
      unmount()
    }
  })

  it("clips an absolute dropdown by overflow around its containing block", () => {
    const { render, renderer, clicks, dropdown, unmount } = setup()
    try {
      render(
        <div style={{ width: 120, height: 120, overflow: "hidden" }}>
          <div style={{ position: "relative", width: 400, height: 300 }}>
            <div style={{ marginLeft: 20, width: 50, height: 50, overflow: "hidden" }}>
              {dropdown({ position: "absolute", left: 30, top: 10 })}
            </div>
          </div>
        </div>
      )
      renderer.dispatchNativeEvents()

      // The static 50 wide card does not reach it, the 120 wide window around the block does.
      renderer.nativeSimulateClick(100, 30)
      expect(clicks).toEqual(["dropdown"])
      renderer.nativeSimulateClick(150, 30)
      expect(clicks).toEqual(["dropdown"])
    } finally {
      unmount()
    }
  })

  it("does not move an absolute dropdown with a static scroller between it and its block", () => {
    const { render, renderer, clicks, dropdown, bounds, unmount } = setup()
    try {
      render(
        <div style={{ position: "relative", width: 400, height: 300 }}>
          <div testId="scroller" style={{ width: 100, height: 100, overflow: "scroll" }}>
            <div testId="content" style={{ height: 1000 }} />
            {dropdown({ position: "absolute", left: 200, top: 10 })}
          </div>
        </div>
      )
      renderer.dispatchNativeEvents()
      expect(bounds("dropdown")).toMatchObject({ x: 200, y: 10 })

      renderer.scrollTo(renderer.findByTestId("scroller")!.id, 0, 40)
      renderer.flush()

      expect(bounds("content").y, "the content moves").toBe(-40)
      expect(bounds("dropdown")).toMatchObject({ x: 200, y: 10 })
      renderer.nativeSimulateClick(220, 30)
      expect(clicks).toEqual(["dropdown"])
    } finally {
      unmount()
    }
  })

  it("moves an absolute dropdown with a positioned scroller", () => {
    const { render, renderer, clicks, dropdown, bounds, unmount } = setup()
    try {
      render(
        <div style={{ width: 400, height: 300 }}>
          <div testId="scroller" style={{ position: "relative", width: 100, height: 100, overflow: "scroll" }}>
            <div style={{ height: 1000 }} />
            {dropdown({ position: "absolute", left: 10, top: 60 })}
          </div>
        </div>
      )
      renderer.dispatchNativeEvents()
      expect(bounds("dropdown")).toMatchObject({ x: 10, y: 60 })

      renderer.scrollTo(renderer.findByTestId("scroller")!.id, 0, 40)
      renderer.flush()

      expect(bounds("dropdown")).toMatchObject({ x: 10, y: 20 })
      renderer.nativeSimulateClick(30, 30)
      expect(clicks).toEqual(["dropdown"])
    } finally {
      unmount()
    }
  })

  it("lets a fixed dropdown out of a positioned overflow: hidden card and a scroller", () => {
    const { render, renderer, clicks, dropdown, bounds, unmount } = setup()
    try {
      render(
        <div style={{ position: "relative", width: 600, height: 400, paddingLeft: 200, paddingTop: 100 }}>
          <div testId="scroller" style={{ position: "relative", width: 100, height: 100, overflow: "scroll" }}>
            <div style={{ height: 1000 }} />
            {dropdown({ position: "fixed", left: 10, top: 10 })}
          </div>
        </div>
      )
      renderer.dispatchNativeEvents()
      renderer.scrollTo(renderer.findByTestId("scroller")!.id, 0, 40)
      renderer.flush()

      expect(bounds("dropdown")).toMatchObject({ x: 10, y: 10 })
      renderer.nativeSimulateClick(50, 30)
      expect(clicks).toEqual(["dropdown"])
    } finally {
      unmount()
    }
  })

  it("lists an escaped dropdown among the elements at a point outside its static card", () => {
    const { render, renderer, dropdown, unmount } = setup()
    try {
      render(card({}, dropdown({ position: "absolute", left: 10, top: 10 })))
      renderer.dispatchNativeEvents()
      const id = (testId: string) => renderer.findByTestId(testId)!.id

      expect(renderer.getElementsAt(50, 30)).toContain(id("dropdown"))
    } finally {
      unmount()
    }
  })

  it("scrolls a nearest scroller around the containing block, not a static one that cannot move it", () => {
    const { render, renderer, dropdown, unmount } = setup()
    try {
      render(
        <div testId="outer" style={{ position: "relative", width: 200, height: 100, overflow: "scroll" }}>
          <div style={{ height: 800 }} />
          <div testId="inner" style={{ width: 100, height: 100, overflow: "scroll" }}>
            <div style={{ height: 800 }} />
            {dropdown({ position: "absolute", left: 10, top: 400 })}
          </div>
        </div>
      )
      renderer.dispatchNativeEvents()
      const scrollTop = (testId: string) => renderer.getScrollOffset(renderer.findByTestId(testId)!.id)![1]

      renderer.scrollIntoView(renderer.findByTestId("dropdown")!.id)
      renderer.flush()

      expect(scrollTop("inner"), "the inner scroller does not move the dropdown").toBe(0)
      expect(scrollTop("outer")).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })
})
