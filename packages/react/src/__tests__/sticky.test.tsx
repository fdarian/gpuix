import path from "path"
import { describe, expect, it } from "vitest"
import type { ReactNode } from "react"
import type { StyleDesc } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

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

describeNative("position: sticky is positioned", () => {
  it("paints above the in-flow rows that scroll under it, though they come later", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      let hit = ""
      const on = (id: string) => () => (hit = id)
      render(
        <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
          <div style={sticky({ top: 0, height: 20, backgroundColor: "#336699" })} onClick={on("header")} />
          <div style={{ height: 40, backgroundColor: "#993333" }} onClick={on("row 1")} />
          <div style={{ height: 40, backgroundColor: "#993333" }} onClick={on("row 2")} />
          <div style={{ height: 400 }} />
        </div>
      )
      renderer.scrollTo(renderer.findByTestId("scroller")!.id, 0, -50)
      renderer.nativeSimulateClick(10, 10)
      renderer.dispatchNativeEvents()
      expect(hit).toBe("header")
    } finally {
      unmount()
    }
  })

  it("is the containing block of an absolute child, and carries it while stuck", () => {
    const { x, y, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div testId="header" style={sticky({ top: 0, width: 100, height: 20 })}>
          <div testId="badge" style={{ position: "absolute", right: 0, top: 0, width: "50%", height: 10 }} />
        </div>
        <div style={{ height: 500 }} />
      </div>
    )
    try {
      expect(x("badge"), "against the header, not the scroller").toBe(50)
      expect(y("badge")).toBe(0)
      scrollTo(0, 80)
      expect(y("header")).toBe(0)
      expect(y("badge"), "moves with the stuck header").toBe(0)
    } finally {
      unmount()
    }
  })

  it("paints a first-child header above the opaque rows that scroll under it", () => {
    const root = createTestRoot()
    try {
      root.render(
        <div testId="scroller" style={{ width: 100, height: 100, overflow: "scroll" }}>
          <div testId="header" style={sticky({ top: 0, height: 20, backgroundColor: "#0000ff" })} />
          <div style={{ height: 40, backgroundColor: "#ff0000" }} />
          <div style={{ height: 40, backgroundColor: "#ff0000" }} />
          <div style={{ height: 400, backgroundColor: "#ff0000" }} />
        </div>
      )
      const renderer = root.renderer
      const scroller = renderer.findByTestId("scroller")!.id
      renderer.scrollTo(scroller, 0, -50)
      renderer.flush()
      const file = path.join(SHOTS_DIR, `sticky-paint-${process.pid}.png`)
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      const origin = renderer.getElementBounds(scroller)!
      const header = renderer.getElementBounds(renderer.findByTestId("header")!.id)!
      expect(header.y - origin.y, "stuck at the top").toBe(0)
      expect(pixels.at(origin.x + 30, origin.y + 10).slice(0, 3), "the header, not the row beneath it").toEqual([0, 0, 255])
      expect(pixels.at(origin.x + 30, origin.y + 30).slice(0, 3), "the rows show below the header").toEqual([255, 0, 0])
    } finally {
      root.unmount()
    }
  })

  it("resolves an absolute child's right and top against the header, not the scroller", () => {
    const { x, y, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div testId="header" style={sticky({ top: 0, width: 120, height: 30 })}>
          <div testId="badge" style={{ position: "absolute", top: 0, right: 0, width: 20, height: 10 }} />
        </div>
        <div style={{ height: 500 }} />
      </div>
    )
    try {
      expect(x("badge"), "120 - 20, not 200 - 20").toBe(100)
      expect(y("badge")).toBe(0)
      scrollTo(0, 60)
      expect(x("badge")).toBe(100)
      expect(y("badge"), "carried by the stuck header").toBe(0)
    } finally {
      unmount()
    }
  })

  it("resolves a percentage inset against the scrollport", () => {
    const { y, scrollTo, unmount } = setup(
      <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
        <div style={{ height: 30 }} />
        <div testId="header" style={sticky({ top: "10%", height: 20 })} />
        <div style={{ height: 500 }} />
      </div>
    )
    try {
      scrollTo(0, 100)
      expect(y("header"), "10% of the 100px scrollport").toBe(10)
    } finally {
      unmount()
    }
  })
})

