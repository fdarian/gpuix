import { describe, expect, it } from "vitest"
import { AnimatePresence, MotionConfig, motion } from "../index.js"
import type { MotionTransition } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const column = { display: "flex", flexDirection: "column", alignItems: "flex-start" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function widthOf(renderer: TestRenderer, testId: string): number {
  return renderer.getElementBounds(renderer.findByTestId(testId).id).width
}

/** Mounts a box animating `width` through `keyframes` and reads it after each wait, in ms. */
function sample(
  keyframes: Array<number | null>,
  transition: MotionTransition,
  waits: number[],
  onMotionComplete?: () => void
): number[] {
  const app = createTestRoot()
  const renderer = app.renderer
  renderer.clockPause()
  app.render(
    <div style={column}>
      <motion.div
        testId="box"
        initial={{ width: 0, height: 10 }}
        animate={{ width: keyframes }}
        transition={transition}
        onMotionComplete={onMotionComplete}
      />
    </div>
  )
  return waits.map((wait) => {
    frame(renderer, wait)
    return widthOf(renderer, "box")
  })
}

describeNative("motion keyframes", () => {
  it("passes through each keyframe, spread evenly over the duration", () => {
    let completions = 0
    const widths = sample([0, 100, 50], { duration: 1, ease: "linear" }, [250, 250, 250, 250], () => completions++)
    expect(widths[0]).toBeCloseTo(50, 0)
    expect(widths[1]).toBeCloseTo(100, 0)
    expect(widths[2]).toBeCloseTo(75, 0)
    expect(widths[3]).toBeCloseTo(50, 0)
    expect(completions).toBe(1)
  })

  it("places keyframes at times", () => {
    const widths = sample([0, 100, 50], { duration: 1, ease: "linear", times: [0, 0.1, 1] }, [50, 50, 450])
    expect(widths[0]).toBeCloseTo(50, 0)
    expect(widths[1]).toBeCloseTo(100, 0)
    expect(widths[2]).toBeCloseTo(75, 0)
  })

  it("eases each segment on its own", () => {
    const widths = sample([0, 100, 0], { duration: 1, ease: ["linear", "easeIn"] }, [250, 500])
    expect(widths[0]).toBeCloseTo(50, 0)
    // easeIn starts slowly: a quarter of the way down is not yet half way.
    expect(widths[1]).toBeGreaterThan(50)
  })

  it("starts a null first keyframe from the current width", () => {
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    app.render(
      <div style={column}>
        <motion.div
          testId="box"
          initial={{ width: 40 }}
          animate={{ width: [null, 100] }}
          transition={{ duration: 1, ease: "linear" }}
        />
      </div>
    )
    expect(widthOf(renderer, "box")).toBeCloseTo(40, 0)
    frame(renderer, 500)
    expect(widthOf(renderer, "box")).toBeCloseTo(70, 0)
  })

  describe("repeat", () => {
    const easeIn = { duration: 1, ease: "easeIn", repeat: 1 } as const
    const waits = [250, 500, 500]

    it("loops from the first keyframe", () => {
      const [first, , second] = sample([0, 100], { ...easeIn, repeatType: "loop" }, waits)
      expect(second).toBeCloseTo(first, 0)
    })

    it("reverse plays the iteration backwards in time", () => {
      const [, later, second] = sample([0, 100], { ...easeIn, repeatType: "reverse" }, waits)
      expect(second).toBeCloseTo(later, 0)
    })

    it("mirror plays the keyframes in reverse with the same easing", () => {
      const [first, , second] = sample([0, 100], { ...easeIn, repeatType: "mirror" }, waits)
      expect(second).toBeCloseTo(100 - first, 0)
    })

    it("holds the end of each iteration for repeatDelay", () => {
      let completions = 0
      const widths = sample(
        [0, 100],
        { duration: 1, ease: "linear", repeat: 1, repeatDelay: 1 },
        [1500, 750, 750, 1000],
        () => completions++
      )
      expect(widths[0]).toBe(100)
      expect(widths[1]).toBeCloseTo(25, 0)
      expect(widths[2]).toBeCloseTo(100, 0)
      expect(completions).toBe(1)
    })

    it("never completes with repeat Infinity", () => {
      let completions = 0
      const widths = sample(
        [0, 100],
        { duration: 1, ease: "linear", repeat: Infinity },
        [250, 10_000, 500],
        () => completions++
      )
      expect(widths[0]).toBeCloseTo(25, 0)
      expect(widths[1]).toBeCloseTo(25, 0)
      expect(widths[2]).toBeCloseTo(75, 0)
      expect(completions).toBe(0)
    })

    it("sends Infinity to the renderer as the string it accepts, wherever the curve sits", () => {
      const app = createTestRoot()
      const renderer = app.renderer
      renderer.clockPause()
      app.render(
        <MotionConfig transition={{ default: { repeat: Infinity } }}>
          <div style={column}>
            <motion.div testId="config" animate={{ width: [0, 100] }} />
            <motion.div
              testId="own"
              animate={{ width: [0, 100], height: [0, 10] }}
              transition={{ duration: 1, height: { repeat: Infinity } }}
            />
          </div>
        </MotionConfig>
      )
      expect(renderer.findByTestId("config").customProps?.motion).toMatchObject({
        transition: { default: { repeat: "Infinity" } },
      })
      expect(renderer.findByTestId("own").customProps?.motion).toMatchObject({
        transition: { duration: 1, height: { repeat: "Infinity" } },
      })
    })
  })

  it("leaves AnimatePresence only after the exit keyframes finish", () => {
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    const tree = (show: boolean) => (
      <div style={column}>
        <AnimatePresence>
          {show ? (
            <motion.div
              key="card"
              testId="card"
              animate={{ width: 100 }}
              exit={{ width: [100, 150, 0] }}
              transition={{ duration: 1, ease: "linear" }}
            />
          ) : null}
        </AnimatePresence>
      </div>
    )
    app.render(tree(true))
    frame(renderer, 1200)
    app.render(tree(false))
    frame(renderer, 500)
    expect(widthOf(renderer, "card")).toBeCloseTo(150, 0)
    frame(renderer, 250)
    expect(widthOf(renderer, "card")).toBeCloseTo(75, 0)
    frame(renderer, 400)
    expect(renderer.findByTestId("card")).toBeUndefined()
  })
})
