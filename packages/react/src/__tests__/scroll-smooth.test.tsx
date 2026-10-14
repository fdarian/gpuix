import { describe, expect, it } from "vitest"
import type { StyleDesc } from "@gpuix/native/host"
import type { ScrollEvent } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function setup(style: StyleDesc = {}, onScroll?: (event: ScrollEvent) => void) {
  const root = createTestRoot()
  root.render(
    <div
      testId="scroller"
      style={{ width: 200, height: 100, overflow: "auto", ...style }}
      onScroll={onScroll}
    >
      <div style={{ height: 1000 }} />
      <div testId="target" style={{ height: 20 }} />
    </div>
  )
  const { renderer } = root
  const id = renderer.findByTestId("scroller")!.id
  renderer.clockPause()
  return {
    ...root,
    id,
    top: () => 0 - renderer.getScrollOffset(id)![1],
    /** Moves the clock and draws the frame it lands on, as the display link would. */
    advance(milliseconds: number) {
      renderer.clockFastForward(milliseconds)
      renderer.flush()
      renderer.dispatchNativeEvents()
    },
  }
}

describeNative("scrollBehavior: smooth", () => {
  it("animates a scrollTo that names no behavior", () => {
    const { renderer, id, top, advance, unmount } = setup({ scrollBehavior: "smooth" })
    try {
      renderer.scrollTo(id, 0, -300)
      expect(top(), "the first frame starts the animation").toBe(0)

      advance(100)
      expect(top()).toBeGreaterThan(0)
      expect(top()).toBeLessThan(300)

      advance(1000)
      expect(top()).toBe(300)
    } finally {
      unmount()
    }
  })

  it("takes between 300 and 450 ms", () => {
    const { renderer, id, top, advance, unmount } = setup({ scrollBehavior: "smooth" })
    try {
      renderer.scrollTo(id, 0, -300)
      advance(250)
      expect(top(), "not done before the shortest duration").toBeLessThan(300)
      advance(250)
      expect(top(), "done by the longest").toBe(300)
    } finally {
      unmount()
    }
  })

  it("lets an explicit instant behavior win", () => {
    const { renderer, id, top, unmount } = setup({ scrollBehavior: "smooth" })
    try {
      renderer.scrollTo(id, 0, -300, "instant")
      expect(top()).toBe(300)
    } finally {
      unmount()
    }
  })

  it("lets an explicit smooth behavior win over the default", () => {
    const { renderer, id, top, advance, unmount } = setup()
    try {
      renderer.scrollTo(id, 0, -300, "smooth")
      advance(100)
      expect(top()).toBeGreaterThan(0)
      expect(top()).toBeLessThan(300)
    } finally {
      unmount()
    }
  })

  it("scrolls instantly without the style or a behavior", () => {
    const { renderer, id, top, unmount } = setup()
    try {
      renderer.scrollTo(id, 0, -300)
      expect(top()).toBe(300)
    } finally {
      unmount()
    }
  })

  it("fires onScroll on the frames it moves", () => {
    const tops: number[] = []
    const { renderer, id, advance, unmount } = setup({ scrollBehavior: "smooth" }, (event) =>
      tops.push(event.scrollTop)
    )
    try {
      renderer.scrollTo(id, 0, -300)
      for (let frame = 0; frame < 8; frame++) advance(50)
      expect(tops.length).toBeGreaterThan(2)
      expect(tops).toEqual([...tops].sort((a, b) => a - b))
      expect(tops.at(-1)).toBe(300)
    } finally {
      unmount()
    }
  })

  it("ends when the wheel takes over", () => {
    const { renderer, id, top, advance, unmount } = setup({ scrollBehavior: "smooth" })
    try {
      renderer.scrollTo(id, 0, -300)
      advance(100)
      renderer.nativeSimulateScrollWheel(50, 50, 0, 10)
      const afterWheel = top()
      advance(1000)
      expect(top()).toBe(afterWheel)
    } finally {
      unmount()
    }
  })

  it("animates scrollIntoView to the target", () => {
    const { renderer, top, advance, unmount } = setup({ scrollBehavior: "smooth" })
    try {
      renderer.scrollIntoView(renderer.findByTestId("target")!.id, "start")
      expect(top()).toBe(0)
      advance(100)
      expect(top()).toBeGreaterThan(0)
      advance(1000)
      expect(top()).toBe(920)
    } finally {
      unmount()
    }
  })

  it("is not applied to a scroller that never asked for it", () => {
    const root = createTestRoot()
    root.render(
      <div style={{ scrollBehavior: "smooth" } as StyleDesc}>
        <div
          testId="plain"
          style={{ width: 200, height: 100, overflow: "auto" }}
        >
          <div style={{ height: 1000 }} />
        </div>
      </div>
    )
    try {
      const id = root.renderer.findByTestId("plain")!.id
      root.renderer.scrollTo(id, 0, -300)
      expect(root.renderer.getScrollOffset(id)).toEqual([0, -300])
    } finally {
      root.unmount()
    }
  })
})
