import { describe, expect, it } from "vitest"
import { readScrollMetrics } from "@gpuix/native/host"
import type { PublicInstance } from "../types/host.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function mount() {
  const refs: Record<string, PublicInstance> = {}
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs[name] = instance
  }
  const root = createTestRoot()
  root.render(
    <div
      ref={ref("scroller")}
      testId="scroller"
      style={{ width: 200, height: 100, padding: 10, overflow: "scroll" }}
    >
      <div style={{ width: 500, height: 400, flexShrink: 0 }} />
      <div ref={ref("target")} testId="target" style={{ height: 20 }} />
    </div>
  )
  return { ...root, refs }
}

describeNative("scroll members of a host element", () => {
  it("reads the geometry the DOM reads", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      expect(scroller.clientWidth).toBe(200)
      expect(scroller.clientHeight).toBe(100)
      expect(scroller.scrollWidth).toBe(520)
      expect(scroller.scrollHeight).toBe(440)
      expect(scroller.scrollTop).toBe(0)
      expect(scroller.scrollLeft).toBe(0)
    } finally {
      unmount()
    }
  })

  it("scrolls instantly when scrollTop and scrollLeft are set", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      scroller.scrollTop = 120
      expect(scroller.scrollTop).toBe(120)
      scroller.scrollLeft = 30
      expect(scroller.scrollLeft).toBe(30)
      scroller.scrollTop = 100_000
      expect(scroller.scrollTop, "clamped to the range").toBe(340)
      scroller.scrollTop = -50
      expect(scroller.scrollTop, "never before the start").toBe(0)
    } finally {
      unmount()
    }
  })

  it("scrolls with scrollTo(options), keeping an axis it does not name", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      scroller.scrollTo({ top: 50, left: 20 })
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([20, 50])
      scroller.scrollTo({ top: 70 })
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([20, 70])
      scroller.scrollTo({ left: 0, behavior: "instant" })
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([0, 70])
    } finally {
      unmount()
    }
  })

  it("scrolls with scrollTo(x, y)", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      scroller.scrollTo(15, 25)
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([15, 25])
    } finally {
      unmount()
    }
  })

  it("scrolls relative to the current position with scrollBy", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      scroller.scrollTo(10, 10)
      scroller.scrollBy(5, 20)
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([15, 30])
      scroller.scrollBy({ top: -10 })
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([15, 20])
    } finally {
      unmount()
    }
  })

  it("answers zero for an element native has not painted", () => {
    const { renderer, unmount } = createTestRoot()
    try {
      expect(readScrollMetrics(renderer, 987_654)).toEqual({
        scrollLeft: 0,
        scrollTop: 0,
        scrollWidth: 0,
        scrollHeight: 0,
        clientWidth: 0,
        clientHeight: 0,
      })
    } finally {
      unmount()
    }
  })
})

describeNative("scrollIntoView", () => {
  function nestedTarget() {
    const refs: Record<string, PublicInstance> = {}
    const ref = (name: string) => (instance: PublicInstance | null) => {
      if (instance) refs[name] = instance
    }
    const root = createTestRoot()
    root.render(
      <div
        ref={ref("outer")}
        style={{ width: 300, height: 200, overflow: "scroll" }}
      >
        <div style={{ height: 300 }} />
        <div style={{ padding: 10 }}>
          <div
            ref={ref("middle")}
            style={{ width: 280, height: 150, overflow: "scroll" }}
          >
            <div style={{ height: 500 }} />
            <div>
              <div style={{ height: 40 }} />
              <div ref={ref("target")} style={{ width: 30, height: 20 }} />
              <div style={{ height: 200 }} />
            </div>
          </div>
        </div>
        <div style={{ height: 600 }} />
      </div>
    )
    return { ...root, refs }
  }

  it("scrolls every scrollable ancestor to a deeply nested descendant", () => {
    const { refs, unmount } = nestedTarget()
    try {
      refs.target!.scrollIntoView()
      // The target is 540 down in the middle scroller, which sits 310 down in the outer one,
      // so both scroll to put it at the start of the view.
      expect(refs.middle!.scrollTop).toBe(540)
      expect(refs.outer!.scrollTop).toBe(310)
    } finally {
      unmount()
    }
  })

  it("aligns the block with start, center and end", () => {
    const { refs, unmount } = nestedTarget()
    try {
      const middle = refs.middle!
      refs.target!.scrollIntoView({ block: "start" })
      expect(middle.scrollTop).toBe(540)
      refs.target!.scrollIntoView({ block: "end" })
      expect(middle.scrollTop).toBe(540 + 20 - 150)
      refs.target!.scrollIntoView({ block: "center" })
      expect(middle.scrollTop).toBe(540 + 10 - 75)
    } finally {
      unmount()
    }
  })

  it("does nothing for block nearest when the target is in view", () => {
    const { refs, unmount } = nestedTarget()
    try {
      const middle = refs.middle!
      refs.target!.scrollIntoView({ block: "start" })
      const outerTop = refs.outer!.scrollTop
      const middleTop = middle.scrollTop
      refs.target!.scrollIntoView({ block: "nearest" })
      expect(middle.scrollTop).toBe(middleTop)
      expect(refs.outer!.scrollTop).toBe(outerTop)
    } finally {
      unmount()
    }
  })

  it("takes the DOM's boolean argument", () => {
    const { refs, unmount } = nestedTarget()
    try {
      refs.target!.scrollIntoView(false)
      expect(refs.middle!.scrollTop).toBe(540 + 20 - 150)
      refs.target!.scrollIntoView(true)
      expect(refs.middle!.scrollTop).toBe(540)
    } finally {
      unmount()
    }
  })
})
