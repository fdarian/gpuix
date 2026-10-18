import fs from "fs"
import os from "os"
import path from "path"
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
import { readLogicalPixels } from "./pixels.js"
import type { Pixels } from "./pixels.js"

const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-motion-values-"))

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
  describe("a CSS length string with calc()", () => {
    const parent = { display: "flex", flexDirection: "column", width: 400 } as const

    function mount(width: MotionValue<string> | MotionValue<number>) {
      const app = createTestRoot({ width: 800, height: 300 })
      app.renderer.clockPause()
      app.render(() => (
        <div style={parent}>
          <div testId="box" style={{ width, height: 10 }} />
        </div>
      ))
      return app.renderer
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

    it("maps through createTransform and springs to a calc", () => {
      const app = createTestRoot({ width: 800, height: 300 })
      app.renderer.clockPause()
      let x!: MotionValue<number>
      let spring!: MotionValue<string>
      app.render(() => {
        x = createMotionValue(0)
        const width = createTransform(x, [0, 1], ["calc(100% - 20px)", "calc(50% + 10px)"])
        spring = createSpring("0%", { stiffness: 300, damping: 30 })
        return (
          <div style={parent}>
            <div testId="mapped" style={{ width, height: 10 }} />
            <div testId="sprung" style={{ width: spring, height: 10 }} />
          </div>
        )
      })
      expect(sizeOf(app.renderer, "mapped").width).toBe(380)
      x.set(0.5)
      app.renderer.flush()
      expect(sizeOf(app.renderer, "mapped").width).toBe(295)

      spring.set("calc(100% - 20px)")
      for (let i = 0; i < 200; i++) frame(app.renderer, 16)
      expect(sizeOf(app.renderer, "sprung").width).toBe(380)
    })
  })

  describe("a multi-value string", () => {
    /** The padding box's insets as the inner box sees them, and the box's own size. */
    function insets(renderer: TestRenderer, box = "box", inner = "inner") {
      renderer.dispatchNativeEvents()
      const outer = renderer.getElementBounds(renderer.findByTestId(box)!.id)!
      const inside = renderer.getElementBounds(renderer.findByTestId(inner)!.id)!
      return { x: inside.x - outer.x, y: inside.y - outer.y, width: outer.width, height: outer.height }
    }

    function mountPadded(padding: MotionValue<string>) {
      const app = createTestRoot({ width: 400, height: 300 })
      app.renderer.clockPause()
      app.render(() => (
        <div style={row}>
          <div testId="box" style={{ fontSize: 16, padding }}>
            <div testId="inner" style={{ width: 20, height: 20 }} />
          </div>
        </div>
      ))
      return app.renderer
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

    it("maps through createTransform and springs to a multi-value string", () => {
      const app = createTestRoot({ width: 400, height: 300 })
      app.renderer.clockPause()
      let x!: MotionValue<number>
      let spring!: MotionValue<string>
      app.render(() => {
        x = createMotionValue(0)
        const mapped = createTransform(x, [0, 1], ["0px 0px", "16px 24px"])
        spring = createSpring("0px 0px", { stiffness: 300, damping: 30 })
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
      })
      x.set(0.5)
      app.renderer.flush()
      expect(insets(app.renderer)).toMatchObject({ x: 12, y: 8 })

      spring.set("16px 24px")
      for (let i = 0; i < 200; i++) frame(app.renderer, 16)
      expect(insets(app.renderer, "other", "inside")).toMatchObject({ x: 24, y: 16 })
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
      const app = createTestRoot({ width: 400, height: 300 })
      app.renderer.clockPause()
      const list = motionValue("1px 2px")
      app.render(() => <div testId="box" style={{ padding: list }} />)
      animate(list, "1px 2px 3px 4px 5px", linear)
      frame(app.renderer, 400)
      expect(list.get()).toBe("1px 2px")
      frame(app.renderer, 400)
      expect(list.get()).toBe("1px 2px 3px 4px 5px")
    })

    it("binds to gap as the row gap and the column gap", () => {
      const app = createTestRoot({ width: 400, height: 300 })
      app.renderer.clockPause()
      const gap = motionValue("4px 9px")
      app.render(() => (
        <div testId="grid" style={{ display: "flex", flexWrap: "wrap", width: 40, gap }}>
          <div testId="a" style={{ width: 20, height: 10 }} />
          <div testId="b" style={{ width: 20, height: 10 }} />
          <div testId="c" style={{ width: 20, height: 10 }} />
        </div>
      ))
      const at = (id: string) => app.renderer.getElementBounds(app.renderer.findByTestId(id)!.id)!
      expect(at("b").y - at("a").y).toBe(10 + 4)
      gap.set("4px 0px")
      app.renderer.flush()
      expect(at("b").x - at("a").x).toBe(20)
      expect(at("b").y).toBe(at("a").y)
    })

    it("paints borderColor 'red blue' with the top and bottom red and the sides blue", () => {
      const app = createTestRoot({ width: 200, height: 200 })
      const renderer = app.renderer
      renderer.clockPause()
      const file = path.join(shots, "motion-value-border-color.png")
      const color = motionValue("#000000 #000000")
      app.render(() => (
        <div
          style={{ width: 80, height: 80, borderStyle: "solid", borderWidth: 20, backgroundColor: "#ffffff", borderColor: color }}
        />
      ))
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
      const app = createTestRoot({ width: 280, height: 220 })
      const renderer = app.renderer
      renderer.clockPause()
      const MARGIN = 40
      const radius = motionValue("10px / 20px")
      app.render(() => (
        <div style={{ width: 280, height: 220, padding: MARGIN, backgroundColor: "#ffffff" }}>
          <div style={{ width: 200, height: 140, backgroundColor: "#ff0000", borderRadius: radius }} />
        </div>
      ))
      const file = path.join(shots, "motion-value-radius.png")
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
})
