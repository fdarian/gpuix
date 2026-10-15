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
    await stopped.finished
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
    await cancelled.finished
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

  it("disposes every node when the root unmounts", () => {
    const { render, renderer, unmount } = createTestRoot()
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

    unmount()
    for (const id of new Set(ids)) {
      expect(() => renderer.getMotionValue(id)).toThrow(/unknown motion value/)
    }
    expect(() => renderer.getMotionValue(outside.id)).toThrow(/unknown motion value/)
    expect(outside.get()).toBe(10)
  })
})
