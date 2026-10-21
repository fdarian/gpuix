import path from "path"
import { useState } from "react"
import { describe, expect, it } from "vitest"
import {
  animate,
  motion,
  motionValue,
  useMotionValue,
  useMotionValueEvent,
  useSpring,
  useTransform,
  useVelocity,
} from "../index.js"
import type { MotionValue } from "../index.js"
import { handleGpuixEvent } from "../reconciler/event-registry.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"
import type { Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const linear = { duration: 1, ease: "linear" } as const

/** One frame of the window, with the events it produced delivered. */
function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function sizeOf(renderer: TestRenderer, testId: string) {
  const bounds = renderer.getElementBounds(renderer.findByTestId(testId).id)
  return { width: bounds.width, height: bounds.height }
}

const row = { display: "flex", flexDirection: "column", alignItems: "flex-start" } as const

describeNative("motion values in style", () => {
  it("types a motion.div's style to accept a motion value", () => {
    const width = motionValue(40)
    const box = <motion.div animate={{ height: 20 }} style={{ width, height: 20 }} />
    expect(box.props.style.width).toBe(width)
  })

  it("drives an element's bounds from a bound value", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    let x!: MotionValue<number>
    function App() {
      x = useMotionValue(50)
      return (
        <div style={row}>
          <div testId="box" style={{ width: x, height: 20 }} />
        </div>
      )
    }
    render(<App />)
    expect(sizeOf(renderer, "box")).toEqual({ width: 50, height: 20 })

    animate(x, 150, linear)
    frame(renderer, 500)
    expect(sizeOf(renderer, "box").width).toBeCloseTo(100, 0)
    frame(renderer, 1000)
    expect(sizeOf(renderer, "box")).toEqual({ width: 150, height: 20 })

    x.set(30)
    renderer.flush()
    expect(sizeOf(renderer, "box").width).toBe(30)
  })

  it("keeps linked useTransform layers on the same instant", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    let x!: MotionValue<number>
    let width!: MotionValue<number>
    let height!: MotionValue<number>
    function App() {
      x = useMotionValue(0)
      width = useTransform(x, [0, 1], [10, 110])
      height = useTransform(x, [0, 1], [20, 220])
      return (
        <div style={row}>
          <div testId="a" style={{ width, height: 10 }} />
          <div testId="b" style={{ width: 10, height }} />
        </div>
      )
    }
    render(<App />)
    animate(x, 1, linear)
    for (const step of [100, 130, 270, 333]) {
      frame(renderer, step)
      expect(height.get() - 20).toBeCloseTo((width.get() - 10) * 2, 9)
      // Layout snaps to whole pixels, so the painted sizes agree to within one.
      const a = sizeOf(renderer, "a").width
      const b = sizeOf(renderer, "b").height
      expect(Math.abs(b - 20 - (a - 10) * 2)).toBeLessThanOrEqual(2)
    }
    frame(renderer, 1000)
    expect(sizeOf(renderer, "a").width).toBe(110)
    expect(sizeOf(renderer, "b").height).toBe(220)
  })

  it("reuses a transform whose inline ranges did not change", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const seen = new Set<MotionValue>()
    function App() {
      const x = useMotionValue(0)
      seen.add(useTransform(x, [0, 1], [10, 110], { clamp: false }))
      return <div testId="box" style={{ width: 10, height: 10 }} />
    }
    render(<App />)
    render(<App />)
    expect(seen.size).toBe(1)
  })

  it("follows a source with useSpring", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    let x!: MotionValue<number>
    function App() {
      x = useMotionValue(0)
      const spring = useSpring(x, { stiffness: 400, damping: 40 })
      return (
        <div style={row}>
          <div testId="box" style={{ width: spring, height: 10 }} />
        </div>
      )
    }
    render(<App />)
    expect(sizeOf(renderer, "box").width).toBe(0)
    x.set(100)
    for (let i = 0; i < 5; i++) frame(renderer, 16)
    const mid = sizeOf(renderer, "box").width
    expect(mid).toBeGreaterThan(1)
    expect(mid).toBeLessThan(100)
    for (let i = 0; i < 100; i++) frame(renderer, 16)
    expect(sizeOf(renderer, "box").width).toBeCloseTo(100, 0)
  })

  it("moves a spring with its own target when it is set", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    let spring!: MotionValue<number>
    function App() {
      spring = useSpring(0, { stiffness: 300, damping: 30 })
      return (
        <div style={row}>
          <div testId="box" style={{ width: spring, height: 10 }} />
        </div>
      )
    }
    render(<App />)
    spring.set(80)
    for (let i = 0; i < 100; i++) frame(renderer, 16)
    expect(sizeOf(renderer, "box").width).toBeCloseTo(80, 0)
  })

  it("runs a JS-computed transform and a velocity value", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    let x!: MotionValue<number>
    let speed!: MotionValue<number>
    function App() {
      x = useMotionValue(0)
      const doubled = useTransform(x, (latest) => latest * 2)
      const plus = useTransform(() => doubled.get() + 1)
      speed = useVelocity(x)
      return (
        <div style={row}>
          <div testId="box" style={{ width: plus, height: 10 }} />
        </div>
      )
    }
    render(<App />)
    expect(sizeOf(renderer, "box").width).toBe(1)
    animate(x, 100, linear)
    frame(renderer, 500)
    expect(x.getVelocity()).toBeCloseTo(100, 3)
    frame(renderer, 16)
    frame(renderer, 16)
    frame(renderer, 16)
    expect(speed.get()).toBeCloseTo(100, 3)
    frame(renderer, 1000)
    for (let i = 0; i < 4; i++) frame(renderer, 16)
    expect(sizeOf(renderer, "box").width).toBe(201)
    expect(speed.get()).toBe(0)
  })

  it("controls an animation: stop, complete, cancel and finished", async () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    let x!: MotionValue<number>
    function App() {
      x = useMotionValue(0)
      return (
        <div style={row}>
          <div testId="box" style={{ width: x, height: 10 }} />
        </div>
      )
    }
    render(<App />)

    const stopped = animate(x, 100, linear)
    frame(renderer, 400)
    stopped.stop()
    expect(stopped.state).toBe("idle")
    frame(renderer, 1000)
    expect(sizeOf(renderer, "box").width).toBeCloseTo(40, 0)

    const completed = animate(x, 100, linear)
    frame(renderer, 100)
    completed.complete()
    await completed.finished
    renderer.flush()
    expect(sizeOf(renderer, "box").width).toBe(100)

    const cancelled = animate(x, 0, linear)
    frame(renderer, 300)
    cancelled.cancel()
    expect(cancelled.state).toBe("idle")
    renderer.flush()
    expect(sizeOf(renderer, "box").width).toBe(100)

    let done = false
    const natural = animate(x, 20, linear)
    void natural.then(() => {
      done = true
    })
    frame(renderer, 1000)
    frame(renderer, 16)
    await natural.finished
    expect(done).toBe(true)
    expect(sizeOf(renderer, "box").width).toBe(20)
  })

  it("batches the setState of events that arrive in one frame", async () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    let x!: MotionValue<number>
    let renders = 0
    function App() {
      renders += 1
      x = useMotionValue(0)
      const double = useTransform(x, [0, 1], [0, 2])
      const [seen, setSeen] = useState({ x: 0, double: 0 })
      useMotionValueEvent(x, "change", (latest) => setSeen((prev) => ({ ...prev, x: latest })))
      useMotionValueEvent(double, "change", (latest) =>
        setSeen((prev) => ({ ...prev, double: latest }))
      )
      return <text>{`${seen.x.toFixed(2)} ${seen.double.toFixed(2)}`}</text>
    }
    render(<App />)
    expect(renderer.getAllText()).toEqual(["0.00 0.00"])
    const before = renders
    animate(x, 1, linear)
    // Delivered the way the runtime does, outside any flushSync of the test root.
    renderer.clockFastForward(500)
    renderer.flush()
    const events = renderer.drainEvents()
    expect(events.length).toBeGreaterThan(1)
    for (const event of events) handleGpuixEvent(event, renderer)
    await new Promise((resolve) => setTimeout(resolve, 50))
    expect(renderer.getAllText()).toEqual(["0.50 1.00"])
    expect(renders - before).toBe(1)
  })

  it("frees a value's node when its last element goes, and keeps its state", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const width = motionValue(40)
    function App({ show }: { show: boolean }) {
      return <div style={row}>{show ? <div testId="box" style={{ width, height: 10 }} /> : null}</div>
    }
    render(<App show />)
    expect(width.host).toBe(renderer)
    expect(renderer.getMotionValue(width.id)).toBe(40)

    render(<App show={false} />)
    expect(width.host).toBeUndefined()
    expect(() => renderer.getMotionValue(width.id)).toThrow(/unknown motion value/)
    expect(width.get()).toBe(40)

    render(<App show />)
    expect(sizeOf(renderer, "box").width).toBe(40)
  })

  it("rebinds when the style swaps one value for another", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const narrow = motionValue(20)
    const wide = motionValue(90)
    function App({ value }: { value: MotionValue<number> | number }) {
      return (
        <div style={row}>
          <div testId="box" style={{ width: value, height: 10 }} />
        </div>
      )
    }
    render(<App value={narrow} />)
    expect(sizeOf(renderer, "box").width).toBe(20)
    render(<App value={wide} />)
    expect(sizeOf(renderer, "box").width).toBe(90)
    expect(narrow.host).toBeUndefined()
    expect(wide.host).toBe(renderer)

    render(<App value={55} />)
    expect(sizeOf(renderer, "box").width).toBe(55)
    expect(wide.host).toBeUndefined()
  })

  it("keeps the other styles beside a bound value, and a set reaches the element", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const width = motionValue(30)
    function App() {
      return (
        <div style={row}>
          <div testId="box" style={{ width, height: 10, opacity: 0.5 }} />
        </div>
      )
    }
    render(<App />)
    expect(sizeOf(renderer, "box")).toEqual({ width: 30, height: 10 })
    width.set(60)
    renderer.flush()
    expect(sizeOf(renderer, "box").width).toBe(60)
  })

  describe("a CSS length string with calc()", () => {
    const parent = { display: "flex", flexDirection: "column", width: 400 } as const

    function mount(width: MotionValue<string> | MotionValue<number>) {
      const { render, renderer } = createTestRoot({ width: 800, height: 300 })
      renderer.clockPause()
      render(
        <div style={parent}>
          <div testId="box" style={{ width, height: 10 }} />
        </div>
      )
      return renderer
    }

    it("binds to style and tracks a set", () => {
      const width = motionValue("calc(100% - 20px)")
      const renderer = mount(width)
      expect(sizeOf(renderer, "box").width).toBe(380)

      width.set("calc(50% + 10px)")
      renderer.flush()
      expect(sizeOf(renderer, "box").width).toBe(210)

      width.set("2em")
      renderer.flush()
      expect(sizeOf(renderer, "box").width).toBe(32)
    })

    it("animates between two calcs as the mix of their parts", () => {
      const width = motionValue("calc(100% - 20px)")
      const renderer = mount(width)
      animate(width, "calc(50% + 10px)", linear)
      frame(renderer, 500)
      // calc(75% - 5px) of 400px.
      expect(sizeOf(renderer, "box").width).toBe(295)
      expect(width.get()).toBe("calc(75% - 5px)")
      frame(renderer, 1000)
      expect(sizeOf(renderer, "box").width).toBe(210)
      expect(width.get()).toBe("calc(50% + 10px)")
    })

    it("animates between px or a percentage and a calc", () => {
      const px = motionValue(20)
      const renderer = mount(px)
      animate(px, "calc(50% + 10px)", linear)
      frame(renderer, 500)
      // calc(25% + 15px) of 400px.
      expect(sizeOf(renderer, "box").width).toBe(115)
      expect(px.get()).toBe("calc(25% + 15px)")
      frame(renderer, 1000)
      expect(sizeOf(renderer, "box").width).toBe(210)

      const percent = motionValue("50%")
      const other = mount(percent)
      animate(percent, "calc(100% - 20px)", linear)
      frame(other, 500)
      expect(sizeOf(other, "box").width).toBe(290)
    })

    it("maps through useTransform and springs to a calc", () => {
      const { render, renderer } = createTestRoot({ width: 800, height: 300 })
      renderer.clockPause()
      let x!: MotionValue<number>
      let spring!: MotionValue<string>
      function App() {
        x = useMotionValue(0)
        const width = useTransform(x, [0, 1], ["calc(100% - 20px)", "calc(50% + 10px)"])
        spring = useSpring("0%", { stiffness: 300, damping: 30 })
        return (
          <div style={parent}>
            <div testId="mapped" style={{ width, height: 10 }} />
            <div testId="sprung" style={{ width: spring, height: 10 }} />
          </div>
        )
      }
      render(<App />)
      expect(sizeOf(renderer, "mapped").width).toBe(380)
      x.set(0.5)
      renderer.flush()
      expect(sizeOf(renderer, "mapped").width).toBe(295)

      spring.set("calc(100% - 20px)")
      for (let i = 0; i < 200; i++) frame(renderer, 16)
      expect(sizeOf(renderer, "sprung").width).toBe(380)
    })
  })

  describe("a multi-value string", () => {
    /** The padding box's insets as the inner box sees them, and the box's own size. */
    function insets(renderer: TestRenderer) {
      renderer.dispatchNativeEvents()
      const box = renderer.getElementBounds(renderer.findByTestId("box").id)
      const inner = renderer.getElementBounds(renderer.findByTestId("inner").id)
      return { x: inner.x - box.x, y: inner.y - box.y, width: box.width, height: box.height }
    }

    function mountPadded(padding: MotionValue<string>) {
      const { render, renderer } = createTestRoot({ width: 400, height: 300 })
      renderer.clockPause()
      render(
        <div style={row}>
          <div testId="box" style={{ fontSize: 16, padding }}>
            <div testId="inner" style={{ width: 20, height: 20 }} />
          </div>
        </div>
      )
      return renderer
    }

    it("binds '0.5em 0.75em' to padding as 8px above and below and 12px at the sides", () => {
      const padding = motionValue("0.5em 0.75em")
      const renderer = mountPadded(padding)
      expect(insets(renderer)).toEqual({ x: 12, y: 8, width: 44, height: 36 })
      // em stays a multiple of the font size until it lands on an element.
      expect(padding.get()).toBe("0.5em 0.75em")

      padding.set("1px 2px 3px 4px")
      renderer.flush()
      expect(insets(renderer)).toEqual({ x: 4, y: 1, width: 26, height: 24 })
    })

    it("animates padding from '0.5em 0.75em' to '1em 1.5em' with each axis on its own value", () => {
      const padding = motionValue("0.5em 0.75em")
      const renderer = mountPadded(padding)
      animate(padding, "1em 1.5em", linear)
      frame(renderer, 500)
      const middle = insets(renderer)
      expect(middle.y).toBeCloseTo(12, 0)
      expect(middle.x).toBeCloseTo(18, 0)
      expect(middle.width).toBeCloseTo(20 + 2 * 18, 0)
      expect(middle.height).toBeCloseTo(20 + 2 * 12, 0)
      expect(padding.get()).toBe("0.75em 1.125em")

      frame(renderer, 1000)
      expect(insets(renderer)).toEqual({ x: 24, y: 16, width: 68, height: 52 })
      expect(padding.get()).toBe("1em 1.5em")
    })

    it("maps through useTransform and springs to a multi-value string", () => {
      const { render, renderer } = createTestRoot({ width: 400, height: 300 })
      renderer.clockPause()
      let x!: MotionValue<number>
      let spring!: MotionValue<string>
      function App() {
        x = useMotionValue(0)
        const mapped = useTransform(x, [0, 1], ["0px 0px", "16px 24px"])
        spring = useSpring("0px 0px", { stiffness: 300, damping: 30 })
        return (
          <div style={row}>
            <div testId="box" style={{ padding: mapped }}>
              <div testId="inner" style={{ width: 20, height: 20 }} />
            </div>
            <div testId="other" style={{ padding: spring }}>
              <div testId="inside" style={{ width: 20, height: 20 }} />
            </div>
          </div>
        )
      }
      render(<App />)
      x.set(0.5)
      renderer.flush()
      expect(insets(renderer)).toMatchObject({ x: 12, y: 8 })

      spring.set("16px 24px")
      for (let i = 0; i < 200; i++) frame(renderer, 16)
      const other = renderer.getElementBounds(renderer.findByTestId("other").id)
      const inside = renderer.getElementBounds(renderer.findByTestId("inside").id)
      expect({ x: inside.x - other.x, y: inside.y - other.y }).toEqual({ x: 24, y: 16 })
    })

    it("fills a one-value string out to the shorthand's sides when it animates to two", () => {
      const padding = motionValue("8px")
      const renderer = mountPadded(padding)
      expect(insets(renderer)).toMatchObject({ x: 8, y: 8 })
      animate(padding, "8px 16px", linear)
      frame(renderer, 500)
      expect(insets(renderer).y).toBeCloseTo(8, 0)
      expect(insets(renderer).x).toBeCloseTo(12, 0)
      frame(renderer, 1000)
      expect(insets(renderer)).toMatchObject({ x: 16, y: 8 })
      expect(padding.get()).toBe("8px 16px")
    })

    it("jumps at the halfway point between strings no shorthand could fill out alike", () => {
      const { render, renderer } = createTestRoot({ width: 400, height: 300 })
      renderer.clockPause()
      const list = motionValue("1px 2px")
      render(<div testId="box" style={{ padding: list }} />)
      animate(list, "1px 2px 3px 4px 5px", linear)
      frame(renderer, 400)
      expect(list.get()).toBe("1px 2px")
      frame(renderer, 400)
      expect(list.get()).toBe("1px 2px 3px 4px 5px")
    })

    it("binds to gap as the row gap and the column gap", () => {
      const { render, renderer } = createTestRoot({ width: 400, height: 300 })
      renderer.clockPause()
      const gap = motionValue("4px 9px")
      render(
        <div testId="grid" style={{ display: "flex", flexWrap: "wrap", width: 40, gap }}>
          <div testId="a" style={{ width: 20, height: 10 }} />
          <div testId="b" style={{ width: 20, height: 10 }} />
          <div testId="c" style={{ width: 20, height: 10 }} />
        </div>
      )
      // Two 20px items and a 9px column gap overflow 40px, so the second wraps.
      const at = (id: string) => renderer.getElementBounds(renderer.findByTestId(id).id)
      expect(at("b").y - at("a").y).toBe(10 + 4)
      gap.set("4px 0px")
      renderer.flush()
      expect(at("b").x - at("a").x).toBe(20)
      expect(at("b").y).toBe(at("a").y)
    })

    it("paints borderColor 'red blue' with the top and bottom red and the sides blue", () => {
      const { render, renderer } = createTestRoot({ width: 200, height: 200 })
      renderer.clockPause()
      const file = path.join(SHOTS_DIR, "motion-value-border-color.png")
      const color = motionValue("#000000 #000000")
      render(
        <div
          style={{ width: 80, height: 80, borderStyle: "solid", borderWidth: 20, backgroundColor: "#ffffff", borderColor: color }}
        />
      )
      const sample = () => {
        renderer.dispatchNativeEvents()
        renderer.captureScreenshot(file)
        return readLogicalPixels(file, renderer)
      }
      const top = (pixels: Pixels) => pixels.at(60, 10).slice(0, 3)
      const left = (pixels: Pixels) => pixels.at(10, 60).slice(0, 3)
      expect(top(sample())).toEqual([0, 0, 0])

      animate(color, "red blue", linear)
      frame(renderer, 1000)
      const done = sample()
      expect(top(done)).toEqual([255, 0, 0])
      expect(left(done)).toEqual([0, 0, 255])
      expect(done.at(60, 70).slice(0, 3)).toEqual([255, 0, 0])
      expect(done.at(70, 60).slice(0, 3)).toEqual([0, 0, 255])
      expect(color.get()).toMatch(/^rgba\(255\.000, 0\.000, 0\.000, 1\.00000\) rgba\(0\.000, 0\.000, 255\.000, 1\.00000\)$/)
    })

    it("paints borderRadius '10px / 20px' animating to '30px / 40px' through a 20px by 30px corner", () => {
      const { render, renderer } = createTestRoot({ width: 280, height: 220 })
      renderer.clockPause()
      const MARGIN = 40
      const radius = motionValue("10px / 20px")
      render(
        <div style={{ width: 280, height: 220, padding: MARGIN, backgroundColor: "#ffffff" }}>
          <div style={{ width: 200, height: 140, backgroundColor: "#ff0000", borderRadius: radius }} />
        </div>
      )
      const file = path.join(SHOTS_DIR, "motion-value-radius.png")
      const painted = (pixels: Pixels, x: number, y: number) => {
        const [r, g, b] = pixels.at(MARGIN + x, MARGIN + y)
        if (r > 200 && g < 60 && b < 60) return true
        if (r > 240 && g > 240 && b > 240) return false
        throw new Error(`(${x}, ${y}) is neither the fill nor the page: ${[r, g, b]}`)
      }
      const shot = () => {
        renderer.dispatchNativeEvents()
        renderer.captureScreenshot(file)
        return readLogicalPixels(file, renderer)
      }

      animate(radius, "30px / 40px", linear)
      frame(renderer, 500)
      // Halfway the corner is an ellipse of 20px across and 30px down: (1, 13) and
      // (4, 8) are outside it but inside a 20px circle.
      const middle = shot()
      expect(painted(middle, 8, 12)).toBe(true)
      expect(painted(middle, 1, 13)).toBe(false)
      expect(painted(middle, 4, 8)).toBe(false)
      expect(painted(middle, 20, 2)).toBe(true)
      expect(painted(middle, 199 - 8, 139 - 12)).toBe(true)
      expect(painted(middle, 199 - 1, 139 - 13)).toBe(false)
      expect(radius.get()).toBe("20px / 30px")

      frame(renderer, 1000)
      const done = shot()
      expect(painted(done, 12, 16)).toBe(true)
      expect(painted(done, 3, 8)).toBe(false)
    })
  })

  it("disposes every node when the root unmounts", () => {
    const { render, renderer, root } = createTestRoot()
    renderer.clockPause()
    const ids: number[] = []
    const outside = motionValue(10)
    function App() {
      const x = useMotionValue(0)
      const width = useTransform(x, [0, 1], [10, 110])
      const spring = useSpring(x)
      const speed = useVelocity(x)
      const double = useTransform(() => x.get() * 2)
      useMotionValueEvent(width, "change", () => {})
      ids.push(x.id, width.id, spring.id, speed.id, double.id)
      return (
        <div style={row}>
          <div testId="box" style={{ width, height: outside }} />
        </div>
      )
    }
    render(<App />)
    for (const id of new Set(ids)) expect(() => renderer.getMotionValue(id)).not.toThrow()
    expect(renderer.getMotionValue(outside.id)).toBe(10)

    // Not the test root's unmount: it disposes the window these reads go through.
    root.unmount()
    for (const id of new Set(ids)) {
      expect(() => renderer.getMotionValue(id)).toThrow(/unknown motion value/)
    }
    expect(() => renderer.getMotionValue(outside.id)).toThrow(/unknown motion value/)
    expect(outside.get()).toBe(10)
    renderer.dispose()
  })
})
