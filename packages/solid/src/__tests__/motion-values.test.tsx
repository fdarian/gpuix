import { createEffect, createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import {
  animate,
  createMotionValue,
  createMotionValueEvent,
  createSpring,
  createTransform,
  createVelocity,
  isMotionValue,
  motion,
  motionValue,
} from "../index.js"
import type { MotionValue } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const

/** One frame of the window, with the events it produced delivered. */
function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function sizeOf(renderer: TestRenderer, testId: string) {
  const bounds = renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
  return { width: bounds.width, height: bounds.height }
}

const row = { display: "flex", flexDirection: "column", alignItems: "flex-start" } as const

describe("Solid motion value exports", () => {
  it("exports the framework-free core and the create primitives", () => {
    expect(isMotionValue(motionValue(1))).toBe(true)
    expect(isMotionValue(1)).toBe(false)
    expect(typeof animate).toBe("function")
    for (const create of [
      createMotionValue,
      createTransform,
      createSpring,
      createVelocity,
      createMotionValueEvent,
    ]) {
      expect(typeof create).toBe("function")
    }
  })

  it("types host and motion.div style to accept a motion value", () => {
    const width = motionValue(40)
    const plain = <div style={{ width, height: 20 }} />
    const animated = <motion.div animate={{ height: 20 }} style={{ width, height: 20 }} />
    expect(plain).toBeDefined()
    expect(animated).toBeDefined()
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid motion values in style", () => {
  it("drives an element's bounds from a bound value", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let x!: MotionValue<number>
    app.render(() => {
      x = createMotionValue(50)
      return (
        <div style={row}>
          <div testId="box" style={{ width: x, height: 20 }} />
        </div>
      )
    })
    expect(sizeOf(app.renderer, "box")).toEqual({ width: 50, height: 20 })

    animate(x, 150, linear)
    frame(app.renderer, 500)
    expect(sizeOf(app.renderer, "box").width).toBeCloseTo(100, 0)
    frame(app.renderer, 1000)
    expect(sizeOf(app.renderer, "box")).toEqual({ width: 150, height: 20 })

    x.set(30)
    app.renderer.flush()
    expect(sizeOf(app.renderer, "box").width).toBe(30)
  })

  it("keeps linked createTransform layers on the same instant", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let x!: MotionValue<number>
    let width!: MotionValue<number>
    let height!: MotionValue<number>
    app.render(() => {
      x = createMotionValue(0)
      width = createTransform(x, [0, 1], [10, 110])
      height = createTransform(x, [0, 1], [20, 220])
      return (
        <div style={row}>
          <div testId="a" style={{ width, height: 10 }} />
          <div testId="b" style={{ width: 10, height }} />
        </div>
      )
    })
    animate(x, 1, linear)
    for (const step of [100, 130, 270, 333]) {
      frame(app.renderer, step)
      expect(height.get() - 20).toBeCloseTo((width.get() - 10) * 2, 9)
      // Layout snaps to whole pixels, so the painted sizes agree to within one.
      const a = sizeOf(app.renderer, "a").width
      const b = sizeOf(app.renderer, "b").height
      expect(Math.abs(b - 20 - (a - 10) * 2)).toBeLessThanOrEqual(2)
    }
    frame(app.renderer, 1000)
    expect(sizeOf(app.renderer, "a").width).toBe(110)
    expect(sizeOf(app.renderer, "b").height).toBe(220)
  })

  it("follows a source with createSpring", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let x!: MotionValue<number>
    app.render(() => {
      x = createMotionValue(0)
      const spring = createSpring(x, { stiffness: 400, damping: 40 })
      return (
        <div style={row}>
          <div testId="box" style={{ width: spring, height: 10 }} />
        </div>
      )
    })
    expect(sizeOf(app.renderer, "box").width).toBe(0)
    x.set(100)
    for (let i = 0; i < 5; i++) frame(app.renderer, 16)
    const mid = sizeOf(app.renderer, "box").width
    expect(mid).toBeGreaterThan(1)
    expect(mid).toBeLessThan(100)
    for (let i = 0; i < 100; i++) frame(app.renderer, 16)
    expect(sizeOf(app.renderer, "box").width).toBeCloseTo(100, 0)
  })

  it("moves a spring with its own target when it is set", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let spring!: MotionValue<number>
    app.render(() => {
      spring = createSpring(0, { stiffness: 300, damping: 30 })
      return (
        <div style={row}>
          <div testId="box" style={{ width: spring, height: 10 }} />
        </div>
      )
    })
    spring.set(80)
    for (let i = 0; i < 100; i++) frame(app.renderer, 16)
    expect(sizeOf(app.renderer, "box").width).toBeCloseTo(80, 0)
  })

  it("runs JS-computed transforms and a velocity value", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let x!: MotionValue<number>
    let speed!: MotionValue<number>
    app.render(() => {
      x = createMotionValue(0)
      const doubled = createTransform(x, (latest) => latest * 2)
      const plus = createTransform(() => doubled.get() + 1)
      speed = createVelocity(x)
      return (
        <div style={row}>
          <div testId="box" style={{ width: plus, height: 10 }} />
        </div>
      )
    })
    expect(sizeOf(app.renderer, "box").width).toBe(1)
    animate(x, 100, linear)
    frame(app.renderer, 500)
    expect(x.getVelocity()).toBeCloseTo(100, 3)
    frame(app.renderer, 16)
    frame(app.renderer, 16)
    frame(app.renderer, 16)
    expect(speed.get()).toBeCloseTo(100, 3)
    frame(app.renderer, 1000)
    for (let i = 0; i < 4; i++) frame(app.renderer, 16)
    expect(sizeOf(app.renderer, "box").width).toBe(201)
    expect(speed.get()).toBe(0)
  })

  it("combines several values in a function-form createTransform", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let sum!: MotionValue<number>
    let a!: MotionValue<number>
    app.render(() => {
      a = createMotionValue(10)
      const b = createMotionValue(5)
      sum = createTransform([a, b], ([left, right]) => left + right)
      return <div testId="box" style={{ width: sum, height: 10 }} />
    })
    expect(sum.get()).toBe(15)
    a.set(20)
    frame(app.renderer, 16)
    frame(app.renderer, 16)
    expect(sum.get()).toBe(25)
  })

  it("controls an animation: stop, complete, cancel and finished", async () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let x!: MotionValue<number>
    app.render(() => {
      x = createMotionValue(0)
      return (
        <div style={row}>
          <div testId="box" style={{ width: x, height: 10 }} />
        </div>
      )
    })
    const renderer = app.renderer

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

  it("delivers createMotionValueEvent in a batch and stops on dispose", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    let x!: MotionValue<number>
    const runs: string[] = []
    app.render(() => {
      x = createMotionValue(0)
      const double = createTransform(x, [0, 1], [0, 2])
      const [seenX, setSeenX] = createSignal(0)
      const [seenDouble, setSeenDouble] = createSignal(0)
      createMotionValueEvent(x, "change", (latest) => {
        setSeenX(latest)
        setSeenDouble(latest * 2)
      })
      createEffect(() => runs.push(`${seenX().toFixed(2)} ${seenDouble().toFixed(2)}`))
      return <text>{`${seenX().toFixed(2)} ${seenDouble().toFixed(2)}`}</text>
    })
    expect(app.renderer.getAllText()).toEqual(["0.00 0.00"])
    runs.length = 0

    animate(x, 1, linear)
    frame(app.renderer, 500)
    expect(app.renderer.getAllText()).toEqual(["0.50 1.00"])
    // Both writes of a delivery land in one effect run.
    expect(runs).toEqual(["0.50 1.00"])

    // Not app.unmount(): it disposes the window the write goes through.
    app.root.unmount()
    app.renderer.flush()
    runs.length = 0
    x.set(0.9)
    expect(runs).toEqual([])
    app.renderer.dispose()
  })

  it("frees a value's node when its last element goes, and keeps its state", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const [show, setShow] = createSignal(true)
    const width = motionValue(40)
    app.render(() => (
      <div style={row}>{show() ? <div testId="box" style={{ width, height: 10 }} /> : null}</div>
    ))
    expect(width.host).toBe(app.renderer)
    expect(app.renderer.getMotionValue(width.id)).toBe(40)

    app.flushSync(() => setShow(false))
    expect(width.host).toBeUndefined()
    expect(() => app.renderer.getMotionValue(width.id)).toThrow(/unknown motion value/)
    expect(width.get()).toBe(40)

    app.flushSync(() => setShow(true))
    expect(sizeOf(app.renderer, "box").width).toBe(40)
    expect(width.host).toBe(app.renderer)
  })

  it("rebinds when the style swaps one value for another", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const narrow = motionValue(20)
    const wide = motionValue(90)
    const [value, setValue] = createSignal<MotionValue<number> | number>(narrow)
    app.render(() => (
      <div style={row}>
        <div testId="box" style={{ width: value(), height: 10 }} />
      </div>
    ))
    expect(sizeOf(app.renderer, "box").width).toBe(20)
    app.flushSync(() => setValue(wide))
    expect(sizeOf(app.renderer, "box").width).toBe(90)
    expect(narrow.host).toBeUndefined()
    expect(wide.host).toBe(app.renderer)

    app.flushSync(() => setValue(55))
    expect(sizeOf(app.renderer, "box").width).toBe(55)
    expect(wide.host).toBeUndefined()
  })

  it("keeps a shared value bound when an element mounts again before the flush", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const width = motionValue(30)
    const [show, setShow] = createSignal(true)
    const box = <div testId="box" style={{ width, height: 10 }} />
    app.render(() => <div style={row}>{show() ? box : null}</div>)
    expect(sizeOf(app.renderer, "box").width).toBe(30)

    app.root.flushSync(() => {
      setShow(false)
      setShow(true)
    })
    app.renderer.flush()
    expect(width.host).toBe(app.renderer)
    width.set(60)
    app.renderer.flush()
    expect(sizeOf(app.renderer, "box").width).toBe(60)
  })

  it("keeps the other styles beside a bound value, and a set reaches the element", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const width = motionValue(30)
    app.render(() => (
      <div style={row}>
        <div testId="box" style={{ width, height: 10, opacity: 0.5 }} />
      </div>
    ))
    expect(sizeOf(app.renderer, "box")).toEqual({ width: 30, height: 10 })
    width.set(60)
    app.renderer.flush()
    expect(sizeOf(app.renderer, "box").width).toBe(60)
  })

  it("disposes every node when the root unmounts", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    const ids: number[] = []
    const outside = motionValue(10)
    app.render(() => {
      const x = createMotionValue(0)
      const width = createTransform(x, [0, 1], [10, 110])
      const spring = createSpring(x)
      const speed = createVelocity(x)
      const double = createTransform(() => x.get() * 2)
      createMotionValueEvent(width, "change", () => {})
      ids.push(x.id, width.id, spring.id, speed.id, double.id)
      return (
        <div style={row}>
          <div testId="box" style={{ width, height: outside }} />
        </div>
      )
    })
    for (const id of new Set(ids)) expect(() => app.renderer.getMotionValue(id)).not.toThrow()
    expect(app.renderer.getMotionValue(outside.id)).toBe(10)

    // Not app.unmount(): it disposes the window these reads go through.
    app.root.unmount()
    app.renderer.flush()
    for (const id of new Set(ids)) {
      expect(() => app.renderer.getMotionValue(id)).toThrow(/unknown motion value/)
    }
    expect(() => app.renderer.getMotionValue(outside.id)).toThrow(/unknown motion value/)
    expect(outside.get()).toBe(10)
    app.renderer.dispose()
  })
})
