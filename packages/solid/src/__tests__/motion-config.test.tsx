import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import {
  MotionConfig,
  animate,
  createReducedMotionConfig,
  motion,
  motionValue,
} from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const
const columnStyle = { display: "flex", flexDirection: "column", alignItems: "flex-start" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function widthOf(renderer: TestRenderer, testId: string): number {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!.width
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

describe.skipIf(!hasNativeTestRenderer)("Solid MotionConfig", () => {
  it("animates without a config, as MotionConfig defaults to never", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={columnStyle}>
        <Box transition={linear} />
      </div>
    ))
    frame(app.renderer, 500)
    expect(widthOf(app.renderer, "box")).toBeCloseTo(50, 0)
  })

  it("jumps position and size under reducedMotion always, and completes", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const completions: string[] = []
    app.render(() => (
      <MotionConfig reducedMotion="always">
        <div style={columnStyle}>
          <Box transition={linear} onMotionComplete={() => completions.push("done")} />
        </div>
      </MotionConfig>
    ))
    expect(widthOf(app.renderer, "box")).toBe(100)
    frame(app.renderer, 16)
    expect(completions).toEqual(["done"])
  })

  it("still animates opacity", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const completions: string[] = []
    app.render(() => (
      <MotionConfig reducedMotion="always">
        <div style={columnStyle}>
          <motion.div
            testId="box"
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: 100, opacity: 1 }}
            transition={linear}
            onMotionComplete={() => completions.push("done")}
          />
        </div>
      </MotionConfig>
    ))
    expect(widthOf(app.renderer, "box")).toBe(100)
    frame(app.renderer, 500)
    expect(completions).toEqual([])
    frame(app.renderer, 600)
    expect(completions).toEqual(["done"])
  })

  it("never ignores the OS setting", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.renderer.setMediaFeatures({ reducedMotion: true })
    app.render(() => (
      <MotionConfig reducedMotion="never">
        <div style={columnStyle}>
          <Box transition={linear} />
        </div>
      </MotionConfig>
    ))
    frame(app.renderer, 500)
    expect(widthOf(app.renderer, "box")).toBeCloseTo(50, 0)
  })

  it("follows the OS setting under user, including a flip mid-animation", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <MotionConfig reducedMotion="user">
        <div style={columnStyle}>
          <Box transition={linear} />
        </div>
      </MotionConfig>
    ))
    frame(app.renderer, 200)
    expect(widthOf(app.renderer, "box")).toBeCloseTo(20, 0)

    app.renderer.setMediaFeatures({ reducedMotion: true })
    app.renderer.flush()
    frame(app.renderer, 16)
    expect(widthOf(app.renderer, "box")).toBe(100)
  })

  it("starts reduced when the OS already asks for it", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.renderer.setMediaFeatures({ reducedMotion: true })
    app.render(() => (
      <MotionConfig reducedMotion="user">
        <div style={columnStyle}>
          <Box transition={linear} />
        </div>
      </MotionConfig>
    ))
    expect(widthOf(app.renderer, "box")).toBe(100)
  })

  it("follows a reactive reducedMotion prop", () => {
    const [setting, setSetting] = createSignal<"never" | "always">("never")
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <MotionConfig reducedMotion={setting()}>
        <div style={columnStyle}>
          <Box transition={linear} />
        </div>
      </MotionConfig>
    ))
    frame(app.renderer, 200)
    expect(widthOf(app.renderer, "box")).toBeCloseTo(20, 0)
    app.flushSync(() => setSetting("always"))
    frame(app.renderer, 16)
    expect(widthOf(app.renderer, "box")).toBe(100)
  })

  it("lets a nested config override only what it sets", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <MotionConfig reducedMotion="always" transition={linear}>
        <MotionConfig transition={{ duration: 2, ease: "linear" }}>
          <div style={columnStyle}>
            <Box />
          </div>
        </MotionConfig>
      </MotionConfig>
    ))
    expect(widthOf(app.renderer, "box")).toBe(100)
  })

  it("uses the config transition when the component has none", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <MotionConfig transition={{ duration: 2, ease: "linear" }}>
        <div style={columnStyle}>
          <Box />
          <motion.div
            testId="own"
            initial={{ width: 0, height: 10 }}
            animate={{ width: 100, height: 10 }}
            transition={linear}
          />
        </div>
      </MotionConfig>
    ))
    frame(app.renderer, 1000)
    expect(widthOf(app.renderer, "box")).toBeCloseTo(50, 0)
    expect(widthOf(app.renderer, "own")).toBe(100)
  })

  it("leaves CSS transitions alone, as the DOM does", () => {
    const [width, setWidth] = createSignal(10)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <MotionConfig reducedMotion="always">
        <div style={columnStyle}>
          <div testId="plain" style={{ width: width(), height: 10, transition: "width 1s linear" }} />
        </div>
      </MotionConfig>
    ))
    app.flushSync(() => setWidth(110))
    frame(app.renderer, 500)
    expect(widthOf(app.renderer, "plain")).toBeCloseTo(60, 0)
  })

  it("reports the resolved setting from createReducedMotionConfig", () => {
    const app = createTestRoot()
    function Probe() {
      const reduced = createReducedMotionConfig()
      return <text>{reduced() ? "reduced" : "full"}</text>
    }
    app.render(() => (
      <MotionConfig reducedMotion="user">
        <Probe />
      </MotionConfig>
    ))
    expect(app.renderer.getAllText()).toEqual(["full"])
    app.renderer.setMediaFeatures({ reducedMotion: true })
    app.renderer.flush()
    expect(app.renderer.getAllText()).toEqual(["reduced"])
  })

  describe("motion values", () => {
    it("jumps animate() on a value bound to a size property", async () => {
      const app = createTestRoot()
      app.renderer.clockPause()
      const width = motionValue(40)
      app.render(() => (
        <MotionConfig reducedMotion="always">
          <div style={columnStyle}>
            <motion.div testId="box" animate={{ height: 10 }} style={{ width, height: 10 }} />
          </div>
        </MotionConfig>
      ))
      const controls = animate(width, 200, linear)
      frame(app.renderer, 16)
      expect(widthOf(app.renderer, "box")).toBe(200)
      expect(width.isAnimating()).toBe(false)
      await controls.finished
    })

    it("keeps animating a value bound to opacity", () => {
      const app = createTestRoot()
      app.renderer.clockPause()
      const opacity = motionValue(0)
      app.render(() => (
        <MotionConfig reducedMotion="always">
          <motion.div testId="box" animate={{ height: 10 }} style={{ opacity, width: 40, height: 10 }} />
        </MotionConfig>
      ))
      animate(opacity, 1, linear)
      frame(app.renderer, 500)
      expect(opacity.get()).toBeCloseTo(0.5, 1)
    })

    it("animates again once reduced motion is off", () => {
      const app = createTestRoot()
      app.renderer.clockPause()
      const width = motionValue(40)
      app.render(() => (
        <MotionConfig reducedMotion="user">
          <div style={columnStyle}>
            <motion.div testId="box" animate={{ height: 10 }} style={{ width, height: 10 }} />
          </div>
        </MotionConfig>
      ))
      app.renderer.setMediaFeatures({ reducedMotion: true })
      app.renderer.flush()
      animate(width, 200, linear)
      frame(app.renderer, 16)
      expect(widthOf(app.renderer, "box")).toBe(200)

      app.renderer.setMediaFeatures({ reducedMotion: false })
      app.renderer.flush()
      animate(width, 40, linear)
      frame(app.renderer, 500)
      expect(widthOf(app.renderer, "box")).toBeCloseTo(120, 0)
    })
  })
})
