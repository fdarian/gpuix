import { describe, expect, it } from "vitest"
import {
  AnimatePresence,
  MotionConfig,
  animate,
  motion,
  motionValue,
  useReducedMotionConfig,
} from "../index.js"
import type { MotionValue } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const linear = { duration: 1, ease: "linear" } as const
const column = { display: "flex", flexDirection: "column", alignItems: "flex-start" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function widthOf(renderer: TestRenderer, testId: string): number {
  return renderer.getElementBounds(renderer.findByTestId(testId).id).width
}

function Box(props: { transition?: object; onMotionComplete?: () => void }) {
  return (
    <motion.div
      testId="box"
      initial={{ width: 0, height: 10 }}
      animate={{ width: 100, height: 10 }}
      transition={props.transition}
      onMotionComplete={props.onMotionComplete}
    />
  )
}

describeNative("MotionConfig", () => {
  it("animates without a config, as MotionConfig defaults to never", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <div style={column}>
        <Box transition={linear} />
      </div>
    )
    frame(renderer, 500)
    expect(widthOf(renderer, "box")).toBeCloseTo(50, 0)
  })

  it("jumps width under reducedMotion always, and completes", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const completions: string[] = []
    render(
      <MotionConfig reducedMotion="always">
        <div style={column}>
          <Box transition={linear} onMotionComplete={() => completions.push("done")} />
        </div>
      </MotionConfig>
    )
    expect(widthOf(renderer, "box")).toBe(100)
    frame(renderer, 16)
    expect(completions).toEqual(["done"])
  })

  it("keeps animating padding and gap while width and height jump", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <MotionConfig reducedMotion="always">
        <div style={column}>
          <motion.div
            testId="padded"
            style={{ height: 10 }}
            initial={{ paddingLeft: 0 }}
            animate={{ paddingLeft: 100 }}
            transition={linear}
          />
          <motion.div
            testId="spaced"
            style={{ display: "flex", flexDirection: "row" }}
            initial={{ gap: 0 }}
            animate={{ gap: 100 }}
            transition={linear}
          >
            <div style={{ width: 10, height: 10 }} />
            <div style={{ width: 10, height: 10 }} />
          </motion.div>
          <motion.div
            testId="sized"
            initial={{ width: 0, height: 0 }}
            animate={{ width: 300, height: 20 }}
            transition={linear}
          />
        </div>
      </MotionConfig>
    )
    const sized = renderer.getElementBounds(renderer.findByTestId("sized").id)
    expect(sized.width).toBe(300)
    expect(sized.height).toBe(20)
    expect(widthOf(renderer, "padded")).toBe(0)
    expect(widthOf(renderer, "spaced")).toBe(20)
    frame(renderer, 500)
    expect(widthOf(renderer, "padded")).toBeCloseTo(50, 0)
    expect(widthOf(renderer, "spaced")).toBeCloseTo(70, 0)
    frame(renderer, 600)
    expect(widthOf(renderer, "padded")).toBe(100)
    expect(widthOf(renderer, "spaced")).toBe(120)
  })

  it("still animates opacity", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const completions: string[] = []
    render(
      <MotionConfig reducedMotion="always">
        <div style={column}>
          <motion.div
            testId="box"
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: 100, opacity: 1 }}
            transition={linear}
            onMotionComplete={() => completions.push("done")}
          />
        </div>
      </MotionConfig>
    )
    expect(widthOf(renderer, "box")).toBe(100)
    frame(renderer, 500)
    expect(completions).toEqual([])
    frame(renderer, 600)
    expect(completions).toEqual(["done"])
  })

  it("lets AnimatePresence finish an exit that only moves or resizes", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (show: boolean) => (
      <MotionConfig reducedMotion="always">
        <AnimatePresence>
          {show ? (
            <motion.div key="card" testId="card" animate={{ width: 100 }} exit={{ width: 0 }} transition={linear} />
          ) : null}
        </AnimatePresence>
      </MotionConfig>
    )
    render(app(true))
    render(app(false))
    frame(renderer, 16)
    expect(renderer.findByTestId("card")).toBeUndefined()
  })

  it("never ignores the OS setting", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    renderer.setMediaFeatures({ reducedMotion: true })
    render(
      <MotionConfig reducedMotion="never">
        <div style={column}>
          <Box transition={linear} />
        </div>
      </MotionConfig>
    )
    frame(renderer, 500)
    expect(widthOf(renderer, "box")).toBeCloseTo(50, 0)
  })

  it("follows the OS setting under user, including a flip mid-animation", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <MotionConfig reducedMotion="user">
        <div style={column}>
          <Box transition={linear} />
        </div>
      </MotionConfig>
    )
    frame(renderer, 200)
    expect(widthOf(renderer, "box")).toBeCloseTo(20, 0)

    renderer.setMediaFeatures({ reducedMotion: true })
    renderer.flush()
    frame(renderer, 16)
    expect(widthOf(renderer, "box")).toBe(100)
  })

  it("starts reduced when the OS already asks for it", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    renderer.setMediaFeatures({ reducedMotion: true })
    render(
      <MotionConfig reducedMotion="user">
        <div style={column}>
          <Box transition={linear} />
        </div>
      </MotionConfig>
    )
    expect(widthOf(renderer, "box")).toBe(100)
  })

  it("lets a nested config override only what it sets", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <MotionConfig reducedMotion="always" transition={linear}>
        <MotionConfig transition={{ duration: 2, ease: "linear" }}>
          <div style={column}>
            <Box />
          </div>
        </MotionConfig>
      </MotionConfig>
    )
    expect(widthOf(renderer, "box")).toBe(100)
  })

  it("uses the config transition when the component has none", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <MotionConfig transition={{ duration: 2, ease: "linear" }}>
        <div style={column}>
          <Box />
          <motion.div
            testId="own"
            initial={{ width: 0, height: 10 }}
            animate={{ width: 100, height: 10 }}
            transition={linear}
          />
        </div>
      </MotionConfig>
    )
    frame(renderer, 1000)
    expect(widthOf(renderer, "box")).toBeCloseTo(50, 0)
    expect(widthOf(renderer, "own")).toBe(100)
  })

  it("leaves CSS transitions alone, as the DOM does", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const app = (width: number) => (
      <MotionConfig reducedMotion="always">
        <div style={column}>
          <div testId="plain" style={{ width, height: 10, transition: "width 1s linear" }} />
        </div>
      </MotionConfig>
    )
    render(app(10))
    render(app(110))
    frame(renderer, 500)
    expect(widthOf(renderer, "plain")).toBeCloseTo(60, 0)
  })

  it("reports the resolved setting from useReducedMotionConfig", () => {
    const { render, renderer } = createTestRoot()
    function Probe() {
      return <text>{useReducedMotionConfig() ? "reduced" : "full"}</text>
    }
    render(<Probe />)
    expect(renderer.getAllText()).toEqual(["full"])

    render(
      <MotionConfig reducedMotion="user">
        <Probe />
      </MotionConfig>
    )
    expect(renderer.getAllText()).toEqual(["full"])
    renderer.setMediaFeatures({ reducedMotion: true })
    renderer.flush()
    expect(renderer.getAllText()).toEqual(["reduced"])
  })

  describe("motion values", () => {
    function Bound(props: { property: "width" | "paddingLeft" | "opacity"; value: MotionValue<number> }) {
      return (
        <div style={column}>
          <motion.div
            testId="box"
            animate={{ height: 10 }}
            style={
              {
                height: 10,
                ...(props.property === "paddingLeft" ? {} : { width: 40 }),
                [props.property]: props.value,
              } as never
            }
          />
        </div>
      )
    }

    it("jumps animate() on a value bound to a size property", async () => {
      const { render, renderer } = createTestRoot()
      renderer.clockPause()
      const width = motionValue(40)
      render(
        <MotionConfig reducedMotion="always">
          <Bound property="width" value={width} />
        </MotionConfig>
      )
      const controls = animate(width, 200, linear)
      frame(renderer, 16)
      expect(widthOf(renderer, "box")).toBe(200)
      expect(width.isAnimating()).toBe(false)
      await controls.finished
    })

    it("keeps animating a value bound to padding", () => {
      const { render, renderer } = createTestRoot()
      renderer.clockPause()
      const padding = motionValue(0)
      render(
        <MotionConfig reducedMotion="always">
          <Bound property="paddingLeft" value={padding} />
        </MotionConfig>
      )
      animate(padding, 100, linear)
      frame(renderer, 500)
      expect(widthOf(renderer, "box")).toBeCloseTo(50, 0)
      expect(padding.isAnimating()).toBe(true)
    })

    it("keeps animating a value bound to opacity", () => {
      const { render, renderer } = createTestRoot()
      renderer.clockPause()
      const opacity = motionValue(0)
      render(
        <MotionConfig reducedMotion="always">
          <Bound property="opacity" value={opacity} />
        </MotionConfig>
      )
      animate(opacity, 1, linear)
      frame(renderer, 500)
      expect(opacity.get()).toBeCloseTo(0.5, 1)
    })

    it("animates again once reduced motion is off", () => {
      const { render, renderer } = createTestRoot()
      renderer.clockPause()
      const width = motionValue(40)
      render(
        <MotionConfig reducedMotion="user">
          <Bound property="width" value={width} />
        </MotionConfig>
      )
      renderer.setMediaFeatures({ reducedMotion: true })
      renderer.flush()
      animate(width, 200, linear)
      frame(renderer, 16)
      expect(widthOf(renderer, "box")).toBe(200)

      renderer.setMediaFeatures({ reducedMotion: false })
      renderer.flush()
      animate(width, 40, linear)
      frame(renderer, 500)
      expect(widthOf(renderer, "box")).toBeCloseTo(120, 0)
    })
  })
})
