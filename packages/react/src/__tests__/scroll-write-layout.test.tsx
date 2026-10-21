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

import { describe, expect, it } from "vitest"
import { animate, useMotionValue, useMotionValueEvent } from "../index.js"
import type { MotionValue } from "../index.js"
import type { TestRenderer } from "../testing.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WIDTH = 400
const HEIGHT = 300

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
