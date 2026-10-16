// @ts-nocheck
/** Focus moves scroll the focused element into view only if needed, to the nearest edge. */

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const item = (testId: string, extra = {}) => (
  <div key={testId} testId={testId} tabIndex={0} style={{ width: 100, height: 40, flexShrink: 0, ...extra }} />
)

function setup(element: React.ReactElement) {
  const root = createTestRoot({ width: 300, height: 400 })
  root.render(element)
  const { renderer } = root
  const idOf = (testId: string) => renderer.findByTestId(testId).id
  const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId ?? null
  const scrollTop = (testId: string) => 0 - (renderer.getScrollOffset(idOf(testId))?.[1] ?? 0)
  return { root, renderer, idOf, focused, scrollTop }
}

// Four 40px items in a 100px scroller: i0 and i1 are visible, i2 is half hidden.
const list = (props = {}) => (
  <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }} {...props}>
    {["i0", "i1", "i2", "i3"].map((id) => item(id))}
  </div>
)

describeNative("focus scrolls into view if needed", () => {
  describe("Tab and Shift+Tab", () => {
    it("scrolls an off-screen item to the nearest edge", () => {
      const { renderer, focused, scrollTop } = setup(list())
      renderer.simulateKeystrokes("Tab Tab")
      expect(focused()).toBe("i1")
      expect(scrollTop("scroller")).toBe(0)
      renderer.simulateKeystrokes("Tab")
      expect(focused()).toBe("i2")
      // i2 spans 80..120 and the port ends at 100: its bottom edge goes to the port's.
      expect(scrollTop("scroller")).toBe(20)
      renderer.simulateKeystrokes("Tab")
      expect(focused()).toBe("i3")
      expect(scrollTop("scroller")).toBe(60)
    })

    it("scrolls back to the nearest edge on Shift+Tab", () => {
      const { renderer, focused, scrollTop, idOf } = setup(list())
      renderer.focusElement(idOf("i3"))
      renderer.scrollTo(idOf("scroller"), 0, -60)
      renderer.simulateKeystrokes("Shift+Tab")
      expect(focused()).toBe("i2")
      expect(scrollTop("scroller")).toBe(60)
      renderer.simulateKeystrokes("Shift+Tab")
      expect(focused()).toBe("i1")
      // i1 spans 40..80; the port is 60..160, so its top edge goes to the port's.
      expect(scrollTop("scroller")).toBe(40)
    })

    it("does not scroll to a visible item", () => {
      const { renderer, focused, scrollTop, idOf } = setup(list())
      renderer.focusElement(idOf("i0"))
      renderer.scrollTo(idOf("scroller"), 0, -20)
      renderer.simulateKeystrokes("Tab")
      expect(focused()).toBe("i1")
      // i1 spans 40..80 and the port is 20..120.
      expect(scrollTop("scroller")).toBe(20)
    })

    it("scrolls every scrolling ancestor of a nested item", () => {
      const { renderer, focused, scrollTop } = setup(
        <div testId="outer" style={{ width: 250, height: 100, overflow: "scroll" }}>
          {item("before", { height: 20 })}
          <div style={{ width: 100, height: 200, flexShrink: 0 }} />
          <div testId="inner" style={{ width: 200, height: 60, flexShrink: 0, overflow: "scroll" }}>
            {["n0", "n1", "n2", "n3"].map((id) => item(id))}
          </div>
        </div>
      )
      renderer.simulateKeystrokes("Tab Tab")
      expect(focused()).toBe("n0")
      // n0 spans 220..260 in the outer content; the outer port is 100 tall.
      expect(scrollTop("outer")).toBe(160)
      expect(scrollTop("inner")).toBe(0)
      renderer.simulateKeystrokes("Tab Tab")
      expect(focused()).toBe("n2")
      expect(scrollTop("inner")).toBe(60)
      // Scrolled in the inner port, n2 spans 240..280 in the outer content.
      expect(scrollTop("outer")).toBe(180)
    })
  })

  describe("element.focus()", () => {
    it("does not scroll a visible element", () => {
      let target
      const { scrollTop } = setup(
        <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
          {item("i0")}
          <div ref={(value) => (target = value)} testId="i1" tabIndex={0} style={{ width: 100, height: 40, flexShrink: 0 }} />
          {item("i2")}
          {item("i3")}
        </div>
      )
      target.focus()
      expect(scrollTop("scroller")).toBe(0)
    })

    it("scrolls an off-screen element to the nearest edge, not the start", () => {
      let target
      const { scrollTop, focused } = setup(
        <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
          {item("i0")}
          {item("i1")}
          {item("i2")}
          <div ref={(value) => (target = value)} testId="i3" tabIndex={0} style={{ width: 100, height: 40, flexShrink: 0 }} />
          {item("i4")}
          {item("i5")}
        </div>
      )
      target.focus()
      expect(focused()).toBe("i3")
      // i3 spans 120..160: its bottom edge goes to the port's, not its top.
      expect(scrollTop("scroller")).toBe(60)
    })

    it("does not scroll with preventScroll", () => {
      let target
      const { scrollTop, focused } = setup(
        <div testId="scroller" style={{ width: 200, height: 100, overflow: "scroll" }}>
          {item("i0")}
          {item("i1")}
          {item("i2")}
          <div ref={(value) => (target = value)} testId="i3" tabIndex={0} style={{ width: 100, height: 40, flexShrink: 0 }} />
          {item("i4")}
          {item("i5")}
        </div>
      )
      target.focus({ preventScroll: true })
      expect(focused()).toBe("i3")
      expect(scrollTop("scroller")).toBe(0)
    })

    it("scrolls nested scrollers", () => {
      let target
      const { scrollTop } = setup(
        <div testId="outer" style={{ width: 250, height: 100, overflow: "scroll" }}>
          <div style={{ width: 100, height: 200, flexShrink: 0 }} />
          <div testId="inner" style={{ width: 200, height: 60, flexShrink: 0, overflow: "scroll" }}>
            {item("n0")}
            {item("n1")}
            <div ref={(value) => (target = value)} testId="n2" tabIndex={0} style={{ width: 100, height: 40, flexShrink: 0 }} />
            {item("n3")}
          </div>
        </div>
      )
      target.focus()
      expect(scrollTop("inner")).toBe(60)
      // Scrolled in the inner port (200..260 in the outer content), n2 ends at 260.
      expect(scrollTop("outer")).toBe(160)
    })
  })
})
