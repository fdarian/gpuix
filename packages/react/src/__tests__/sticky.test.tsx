import { describe, expect, it } from "vitest"
import type { ReactNode } from "react"
import type { StyleDesc } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function setup(tree: ReactNode) {
  const root = createTestRoot()
  root.render(tree)
  const { renderer } = root
  const id = (testId: string) => renderer.findByTestId(testId)!.id
  const origin = (testId: string) => renderer.getElementBounds(id(testId))!
  return {
    ...root,
    renderer,
    /** Where `testId` is within the scroller, so the answer does not depend on the window. */
    y: (testId: string) => origin(testId).y - origin("scroller").y,
    x: (testId: string) => origin(testId).x - origin("scroller").x,
    scrollTo: (left: number, top: number) => renderer.scrollTo(id("scroller"), -left, -top),
  }
}

const sticky = (style: StyleDesc): StyleDesc => ({ position: "sticky", ...style })

describeNative("position: sticky", () => {
  it("keeps a header at the top of its scroller while the content scrolls", () => {
    const { y, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div testId="header" style={sticky({ top: 0, height: 20, backgroundColor: "#336699" })} />
        <div testId="body" style={{ height: 500 }} />
      </div>
    )
    try {
      expect(y("header")).toBe(0)
      for (const top of [10, 50, 300]) {
        scrollTo(0, top)
        expect(y("header"), `scrolled ${top}`).toBe(0)
        expect(y("body"), "the rest of the content moves as usual").toBe(20 - top)
      }
    } finally {
      unmount()
    }
  })

  it("lays out like relative: its insets do not move its siblings", () => {
    const { y, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div testId="header" style={sticky({ top: 10, height: 20 })} />
        <div testId="body" style={{ height: 500 }} />
      </div>
    )
    try {
      expect(y("body")).toBe(20)
    } finally {
      unmount()
    }
  })

  it("holds the gap its inset names", () => {
    const { y, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div testId="header" style={sticky({ top: 10, height: 20 })} />
        <div style={{ height: 500 }} />
      </div>
    )
    try {
      scrollTo(0, 60)
      expect(y("header")).toBe(10)
    } finally {
      unmount()
    }
  })

  it("is pushed out when its parent scrolls past", () => {
    const { y, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div testId="section" style={{ height: 150 }}>
          <div testId="header" style={sticky({ top: 0, height: 20 })} />
        </div>
        <div style={{ height: 400 }} />
      </div>
    )
    try {
      scrollTo(0, 100)
      expect(y("header"), "the section still reaches below it").toBe(0)
      scrollTo(0, 140)
      // The section ends 10px into the scroller, so the header sits above it: 10 - 20.
      expect(y("header")).toBe(-10)
      scrollTo(0, 200)
      expect(y("header"), "and travels with the section's end from there").toBe(-70)
    } finally {
      unmount()
    }
  })

  it("keeps each section's header apart", () => {
    const { y, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div style={{ height: 150 }}>
          <div testId="first" style={sticky({ top: 0, height: 20 })} />
        </div>
        <div style={{ height: 150 }}>
          <div testId="second" style={sticky({ top: 0, height: 20 })} />
        </div>
        <div style={{ height: 300 }} />
      </div>
    )
    try {
      scrollTo(0, 100)
      expect(y("first")).toBe(0)
      expect(y("second"), "not yet reached").toBe(50)
      scrollTo(0, 200)
      expect(y("second")).toBe(0)
      expect(y("first")).toBeLessThan(0)
    } finally {
      unmount()
    }
  })

  it("sticks on the left while the row scrolls sideways", () => {
    const { x, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 100, height: 60, overflow: "scroll" }}>
        <div style={{ display: "flex", flexDirection: "row", width: 430, height: 40 }}>
          <div testId="label" style={sticky({ left: 5, width: 30, height: 40, flexShrink: 0 })} />
          <div style={{ width: 400, height: 40, flexShrink: 0 }} />
        </div>
      </div>
    )
    try {
      expect(x("label")).toBe(5)
      scrollTo(100, 0)
      expect(x("label")).toBe(5)
      scrollTo(330, 0)
      expect(x("label")).toBe(5)
    } finally {
      unmount()
    }
  })

  it("is pushed out sideways when its parent scrolls past", () => {
    const { x, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 100, height: 60, overflow: "scroll" }}>
        <div style={{ display: "flex", flexDirection: "row", height: 40, width: 500 }}>
          <div style={{ width: 150, height: 40, flexShrink: 0 }}>
            <div testId="label" style={sticky({ left: 0, width: 30, height: 40 })} />
          </div>
          <div style={{ width: 350, height: 40, flexShrink: 0 }} />
        </div>
      </div>
    )
    try {
      scrollTo(100, 0)
      expect(x("label")).toBe(0)
      scrollTo(140, 0)
      expect(x("label"), "the 150px cell ends 10px in: 10 - 30").toBe(-20)
    } finally {
      unmount()
    }
  })

  it("sticks inside the nearest scroller, not an outer one", () => {
    const { y, renderer, scrollTo, unmount } = setup(
      <div testId="outer" style={{ width: 200, height: 150, overflow: "scroll" }}>
        <div style={{ height: 20 }} />
        <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
          <div testId="header" style={sticky({ top: 0, height: 20 })} />
          <div style={{ height: 500 }} />
        </div>
        <div style={{ height: 600 }} />
      </div>
    )
    try {
      renderer.scrollTo(renderer.findByTestId("outer")!.id, 0, -10)
      scrollTo(0, 50)
      expect(y("header")).toBe(0)
    } finally {
      unmount()
    }
  })
})
