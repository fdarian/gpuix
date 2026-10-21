/**
 * Writing `scrollTop` lays out nothing, and a layout a read forces advances no animation (#70).
 *
 * In a browser a scroll offset is a paint property: the setter forces a layout only when one is
 * pending (a no-op when clean), clamps, and writes, so N writes in a task with nothing dirty run
 * no layout. Forced layout never runs animation frames either: the animation clock stands still
 * between frames. The cascade this guards against: a spring's `change` handler writes `scrollTop`,
 * the write made the next read lay out, the layout sampled the clock and advanced the spring,
 * its next `change` wrote again, 40 to 60 passes in a frame.
 *
 * The test window draws a dirty window whenever an update flushes, which would hide every pass,
 * so these tests turn that off (`setInlineDraws(false)`) and draw frames with `drawFrame`.
 */

import { useState } from "react"
import { describe, expect, it } from "vitest"
import { animate, useMotionValue, useMotionValueEvent } from "../index.js"
import type { MotionValue } from "../index.js"
import { flushSync } from "../reconciler/reconciler.js"
import type { PublicInstance } from "../types/host.js"
import type { TestRenderer } from "../testing.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WIDTH = 400
const HEIGHT = 300
const ROW = 36

function Rows(props: { count: number }) {
  return Array.from({ length: props.count }, (_, index) => (
    <div key={index} style={{ height: ROW, flexShrink: 0 }} />
  ))
}

/** A frame as the display link runs it: time passes, the events of the last draw are delivered, and
 *  a pending change is drawn once. */
function frame(renderer: TestRenderer, ms = 1000 / 60): void {
  renderer.clockFastForward(ms)
  renderer.dispatchNativeEvents()
  renderer.drawFrame()
}

function mount(node: React.ReactElement, options: { liveClock?: boolean } = {}) {
  const root = createTestRoot({ width: WIDTH, height: HEIGHT })
  if (!options.liveClock) root.renderer.clockPause()
  root.renderer.setInlineDraws(false)
  root.render(node)
  root.renderer.drawFrame()
  return root
}

describeNative("a scrollTop write", () => {
  it("lays out at most once a frame when a spring's change handler writes it", async () => {
    let position!: MotionValue<number>
    let scroller: PublicInstance | null = null
    let events = 0
    function Page() {
      position = useMotionValue(0)
      useMotionValueEvent(position, "change", (top) => {
        events++
        if (scroller !== null) scroller.scrollTop = top
      })
      return (
        <div
          ref={(instance) => {
            scroller = instance
          }}
          style={{ width: WIDTH, height: HEIGHT, overflowY: "scroll", flexDirection: "column" }}
        >
          <Rows count={400} />
        </div>
      )
    }
    // The clock runs, as it does under the display link: a layout that sampled it would move the spring.
    const { renderer, unmount } = mount(<Page />, { liveClock: true })
    try {
      let target = 0
      expect(scroller!.scrollTop).toBe(0)
      renderer.drawFrame()
      const layoutPasses = renderer.getLayoutPassCount()
      const passes = renderer.getLayoutCounters().passes
      const frames = 30
      for (let index = 0; index < frames; index++) {
        if (index % 2 === 0) {
          if (position.isAnimating()) position.stop()
          target += ROW * 2
          animate(position, target, { type: "spring", duration: 0.08, bounce: 0 })
        }
        renderer.dispatchNativeEvents()
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
        await new Promise((resolve) => setTimeout(resolve, 16))
      }
      expect(events, "the spring moved and its handler ran").toBeGreaterThan(10)
      expect(scroller!.scrollTop, "and the scroller followed").toBeGreaterThan(ROW * 2)
      expect(renderer.getLayoutPassCount() - layoutPasses, "no layout-only pass").toBe(0)
      // A verified frame builds the page a second time, whole, and that build is a pass of its own.
      if (!verifying) expect(renderer.getLayoutCounters().passes - passes, "the draws' own").toBeLessThanOrEqual(frames)
    } finally {
      renderer.setInlineDraws(true)
      unmount()
    }
  })

  it("runs no layout for any number of writes when nothing is dirty, and clamps each", () => {
    let scroller: PublicInstance | null = null
    const { renderer, unmount } = mount(
      <div
        ref={(instance) => {
          scroller = instance
        }}
        style={{ width: WIDTH, height: HEIGHT, overflowY: "scroll", flexDirection: "column" }}
      >
        <Rows count={25} />
      </div>
    )
    try {
      const max = 25 * ROW - HEIGHT
      expect(scroller!.scrollTop).toBe(0)
      const layoutPasses = renderer.getLayoutPassCount()
      const passes = renderer.getLayoutCounters().passes
      const painted = renderer.getPaintedFrameCount()
      for (let index = 1; index <= 50; index++) {
        scroller!.scrollTop = index * 100
        scroller!.scrollLeft = 0
      }
      expect(scroller!.scrollTop, "clamped to the largest offset").toBe(max)
      expect(scroller!.scrollHeight - scroller!.clientHeight).toBe(max)
      scroller!.scrollTop = -20
      expect(scroller!.scrollTop, "and to the start").toBe(0)
      expect(renderer.getLayoutPassCount() - layoutPasses).toBe(0)
      expect(renderer.getLayoutCounters().passes - passes).toBe(0)
      expect(renderer.getPaintedFrameCount() - painted, "a write draws nothing itself").toBe(0)

      scroller!.scrollTop = 100
      renderer.drawFrame()
      expect(renderer.getPaintedFrameCount() - painted, "the frame still redraws").toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      unmount()
    }
  })

  it("lays out once after a style change in the same task and clamps against the new extent", () => {
    let scroller: PublicInstance | null = null
    let setCount!: (count: number) => void
    function Page() {
      const [count, set] = useState(25)
      setCount = set
      return (
        <div
          ref={(instance) => {
            scroller = instance
          }}
          style={{ width: WIDTH, height: HEIGHT, overflowY: "scroll", flexDirection: "column" }}
        >
          <Rows count={count} />
        </div>
      )
    }
    const { renderer, unmount } = mount(<Page />)
    try {
      expect(scroller!.scrollTop).toBe(0)
      const layoutPasses = renderer.getLayoutPassCount()

      flushSync(() => setCount(100))
      scroller!.scrollTop = 1e6

      expect(scroller!.scrollTop, "the content grew before the write was clamped").toBe(100 * ROW - HEIGHT)
      expect(renderer.getLayoutPassCount() - layoutPasses, "one layout for the change").toBe(1)
      scroller!.scrollTop = 500
      scroller!.scrollTop = 1e6
      expect(renderer.getLayoutPassCount() - layoutPasses, "and none for the writes after it").toBe(1)

      flushSync(() => setCount(10))
      expect(scroller!.scrollTop, "content that shrinks pulls the offset back").toBe(Math.max(0, 10 * ROW - HEIGHT))
    } finally {
      renderer.setInlineDraws(true)
      unmount()
    }
  })

  it("moves what a bounds read sees", () => {
    let scroller: PublicInstance | null = null
    let child: PublicInstance | null = null
    const { renderer, unmount } = mount(
      <div
        ref={(instance) => {
          scroller = instance
        }}
        style={{ width: WIDTH, height: HEIGHT, overflowY: "scroll", flexDirection: "column" }}
      >
        <Rows count={5} />
        <div
          ref={(instance) => {
            child = instance
          }}
          style={{ height: ROW, flexShrink: 0 }}
        />
        <Rows count={20} />
      </div>
    )
    try {
      const top = child!.getBoundingClientRect().top
      renderer.drawFrame()
      const layoutPasses = renderer.getLayoutPassCount()

      scroller!.scrollTop = 120
      expect(renderer.getLayoutPassCount() - layoutPasses, "the write lays out nothing").toBe(0)
      expect(child!.getBoundingClientRect().top, "a bounds read sees the scrolled position").toBe(top - 120)
      expect(renderer.getLayoutPassCount() - layoutPasses, "and lays out for it, once").toBe(1)
      scroller!.scrollTop = 150
      expect(child!.getBoundingClientRect().top).toBe(top - 150)
      expect(renderer.getLayoutPassCount() - layoutPasses).toBe(2)
    } finally {
      renderer.setInlineDraws(true)
      unmount()
    }
  })
})

describeNative("a layout pass a read forces", () => {
  it("advances no motion value and fires no change event", () => {
    let width!: MotionValue<number>
    const events: number[] = []
    function Page() {
      width = useMotionValue(10)
      useMotionValueEvent(width, "change", (value) => events.push(value))
      return (
        <div style={{ flexDirection: "column", alignItems: "flex-start" }}>
          <div testId="box" style={{ width, height: 20 }} />
        </div>
      )
    }
    const { renderer, unmount } = mount(<Page />)
    try {
      const boxWidth = () => renderer.getElementBounds(renderer.findByTestId("box").id).width
      animate(width, 210, { duration: 1, ease: "linear" })
      frame(renderer, 100)
      renderer.dispatchNativeEvents()
      const drawn = boxWidth()
      expect(drawn, "the frame advanced it").toBeGreaterThan(10)
      const delivered = events.length
      expect(delivered, "and told JS").toBeGreaterThan(0)

      // Time passes and something dirties the window: the read lays out, but only a draw samples.
      const layoutPasses = renderer.getLayoutPassCount()
      renderer.clockFastForward(400)
      renderer.flush()
      expect(boxWidth(), "the layout shows the last frame's value").toBe(drawn)
      expect(renderer.getLayoutPassCount() - layoutPasses, "and it did lay out").toBe(1)
      renderer.dispatchNativeEvents()
      expect(events.length, "and fired nothing").toBe(delivered)

      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(boxWidth(), "the next draw takes the time").toBeGreaterThan(drawn + 50)
      expect(events.length).toBeGreaterThan(delivered)
    } finally {
      renderer.setInlineDraws(true)
      unmount()
    }
  })
})
