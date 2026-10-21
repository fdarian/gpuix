import { createEffect, createSignal, onCleanup } from "solid-js"
import { describe, expect, it } from "bun:test"
import { animate, createMotionValue, motionValue } from "../index.js"
import type { AnimationPlaybackControlsWithThen, MotionValue } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function widthOf(renderer: TestRenderer, testId: string): number {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!.width
}

const row = { display: "flex", flexDirection: "column", alignItems: "flex-start" } as const

/** Whether a promise has resolved, after the microtasks and one timer. */
async function resolved(promise: PromiseLike<unknown>): Promise<boolean> {
  return Promise.race([
    Promise.resolve(promise).then(() => true),
    new Promise<boolean>((done) => setTimeout(() => done(false), 5)),
  ])
}

function mount(initial = 0) {
  const app = createTestRoot()
  app.renderer.clockPause()
  let x!: MotionValue<number>
  app.render(() => {
    x = createMotionValue(initial)
    return (
      <div style={row}>
        <div testId="box" style={{ width: x, height: 10 }} />
      </div>
    )
  })
  return { renderer: app.renderer, root: app.root, x, width: () => widthOf(app.renderer, "box") }
}

// The fluid-hover demo's cursor: one animation of three round trips, each faster than the last,
// paused and played in place by an effect that follows `running`.
const TOP = 2
const BOTTOM = 102
const KEYFRAMES = [TOP, BOTTOM, TOP, BOTTOM, TOP, BOTTOM, TOP]
const LEGS = [2, 2, 0.8, 0.8, 0.3, 0.3]
const PASS = LEGS.reduce((a, b) => a + b, 0)
const TIMES = LEGS.reduce<number[]>((acc, leg) => [...acc, acc[acc.length - 1] + leg / PASS], [0])

describe.skipIf(!hasNativeTestRenderer)("Solid playback controls of animate()", () => {
  it("holds the fluid-hover cursor mid-loop and plays it on from the held time", () => {
    const app = createTestRoot()
    const renderer = app.renderer
    renderer.clockPause()
    let controls!: AnimationPlaybackControlsWithThen
    let value!: MotionValue<number>
    const [running, setRunning] = createSignal(true)
    app.render(() => {
      const y = createMotionValue(TOP)
      let started: AnimationPlaybackControlsWithThen | undefined
      createEffect(() => {
        if (!running()) {
          started?.pause()
          return
        }
        if (started) {
          started.play()
          return
        }
        started = animate(y, KEYFRAMES, { duration: PASS, times: TIMES, ease: "easeInOut", repeat: Infinity })
        controls = started
        value = y
      })
      onCleanup(() => started?.stop())
      return (
        <div style={row}>
          <div testId="cursor" style={{ width: y, height: 10 }} />
        </div>
      )
    })
    const sync = (next: boolean) => setRunning(next)
    const cursor = () => widthOf(renderer, "cursor")

    // A twin held at 3 s by a seek, the second leg half way down, to read what 3 s and 3.5 s are.
    const twin = motionValue(TOP)
    twin.bind(renderer)
    const reference = animate(twin, KEYFRAMES, { duration: PASS, times: TIMES, ease: "easeInOut", repeat: Infinity })
    reference.pause()
    const at = (seconds: number) => {
      reference.time = seconds
      frame(renderer, 0)
      return twin.get()
    }
    const held = at(3)
    const later = at(3.5)
    const lap = at(3 + PASS)
    expect(held).toBeGreaterThan(TOP + 20)
    expect(held).toBeLessThan(BOTTOM - 20)
    expect(later).toBeLessThan(held)
    expect(lap).toBeCloseTo(held, 6)

    frame(renderer, 3000)
    expect(controls.state).toBe("running")
    expect(controls.time).toBeCloseTo(3, 6)
    expect(value.get()).toBeCloseTo(held, 6)
    expect(cursor()).toBeCloseTo(held, 1)

    sync(false)
    expect(controls.state).toBe("paused")
    for (const ms of [16, 1000, 4000, 60000]) {
      frame(renderer, ms)
      expect(value.get()).toBeCloseTo(held, 6)
      expect(cursor()).toBeCloseTo(held, 1)
      expect(controls.time).toBeCloseTo(3, 6)
      expect(value.getVelocity()).toBe(0)
      expect(value.isAnimating()).toBe(true)
    }

    sync(true)
    expect(controls.state).toBe("running")
    frame(renderer, 500)
    expect(controls.time).toBeCloseTo(3.5, 6)
    expect(value.get()).toBeCloseTo(later, 6)
    expect(cursor()).toBeCloseTo(later, 1)
    frame(renderer, PASS * 1000 - 500)
    expect(controls.time).toBeCloseTo(3 + PASS, 5)
    expect(value.get()).toBeCloseTo(held, 5)

    app.root.unmount()
    expect(controls.state).toBe("idle")
    twin.destroy()
  })

  it("pauses and plays an animation in place", () => {
    const { renderer, x, width } = mount()
    const controls = animate(x, 100, linear)
    expect(controls.state).toBe("running")
    frame(renderer, 400)
    controls.pause()
    expect(controls.state).toBe("paused")
    frame(renderer, 5000)
    expect(width()).toBe(40)
    expect(x.get()).toBeCloseTo(40, 6)
    expect(controls.time).toBeCloseTo(0.4, 6)

    controls.play()
    expect(controls.state).toBe("running")
    frame(renderer, 100)
    expect(width()).toBe(50)
    frame(renderer, 500)
    expect(width()).toBe(100)
    expect(controls.state).toBe("finished")
  })

  it("reads and sets the time", () => {
    const { renderer, x, width } = mount()
    const controls = animate(x, 100, linear)
    frame(renderer, 400)
    expect(controls.time).toBeCloseTo(0.4, 6)

    controls.time = 0.9
    expect(controls.time).toBeCloseTo(0.9, 6)
    frame(renderer, 0)
    expect(width()).toBe(90)
    frame(renderer, 50)
    expect(width()).toBe(95)

    controls.pause()
    controls.time = 0.2
    expect(controls.state).toBe("paused")
    frame(renderer, 1000)
    expect(width()).toBe(20)
    expect(controls.time).toBeCloseTo(0.2, 6)

    // Past the end while held it stays held there, and a running one finishes.
    controls.time = 5
    frame(renderer, 16)
    expect(width()).toBe(100)
    expect(controls.state).toBe("paused")

    const running = animate(x, 0, linear)
    frame(renderer, 100)
    running.time = 5
    frame(renderer, 16)
    expect(width()).toBe(0)
    expect(running.state).toBe("finished")
  })

  it("keeps the time as set in an animation with a delay", () => {
    const { renderer, x, width } = mount()
    const controls = animate(x, 100, { ...linear, delay: 0.5 })
    frame(renderer, 100)
    expect(controls.time).toBe(0)
    controls.time = 0.4
    // Motion counts the delay in the time it is given; here it reads back as set.
    expect(controls.time).toBeCloseTo(0.4, 6)
    frame(renderer, 0)
    expect(width()).toBe(40)
  })

  it("runs at a speed, and backwards to the first keyframe", async () => {
    const { renderer, x, width } = mount()
    const controls = animate(x, 100, linear)
    controls.speed = 2
    expect(controls.speed).toBe(2)
    frame(renderer, 250)
    expect(width()).toBe(50)
    expect(controls.time).toBeCloseTo(0.5, 6)

    controls.speed = -1
    frame(renderer, 200)
    expect(width()).toBe(30)
    expect(controls.time).toBeCloseTo(0.3, 6)
    expect(controls.state).toBe("running")
    expect(await resolved(controls.finished)).toBe(false)

    frame(renderer, 1000)
    expect(width()).toBe(0)
    expect(controls.state).toBe("finished")
    expect(await resolved(controls.finished)).toBe(true)
  })

  it("holds at speed 0 and runs again at another", () => {
    const { renderer, x, width } = mount()
    const controls = animate(x, 100, linear)
    frame(renderer, 300)
    controls.speed = 0
    expect(controls.state).toBe("running")
    frame(renderer, 3000)
    expect(width()).toBe(30)
    // Motion needs play() as well; here a speed other than 0 runs it again.
    controls.speed = 1
    frame(renderer, 200)
    expect(width()).toBe(50)
  })

  it("reads the duration of one iteration, and with the delay", () => {
    const { x } = mount()
    const tween = animate(x, 100, { duration: 2, delay: 0.25, repeat: 3 })
    expect(tween.duration).toBe(2)
    expect(tween.iterationDuration).toBe(2.25)
    const spring = animate(x, 100, { type: "spring", stiffness: 300, damping: 30 })
    expect(spring.duration).toBeGreaterThan(0.1)
    expect(spring.duration).toBeLessThan(20)
    expect(spring.iterationDuration).toBe(spring.duration)
  })

  it("throws for the duration of an animation whose value is not bound yet", () => {
    const loose = motionValue(0)
    const controls = animate(loose, 100, linear)
    expect(() => controls.duration).toThrow(/not known until/)
    expect(() => controls.iterationDuration).toThrow(/not known until/)
    expect(controls.state).toBe("running")
    expect(controls.speed).toBe(1)
    expect(controls.time).toBe(0)
    controls.stop()
    loose.destroy()
  })

  it("goes from running to paused to finished to idle", () => {
    const { renderer, x } = mount()
    const controls = animate(x, 100, linear)
    const states = [controls.state]
    controls.pause()
    states.push(controls.state)
    controls.play()
    states.push(controls.state)
    frame(renderer, 1000)
    states.push(controls.state)
    controls.play()
    states.push(controls.state)
    frame(renderer, 200)
    controls.stop()
    states.push(controls.state)
    expect(states).toEqual(["running", "paused", "running", "finished", "running", "idle"])
  })

  it("resolves finished and then on a finish and on complete()", async () => {
    const { renderer, x, width } = mount()
    const natural = animate(x, 100, linear)
    let thenCalls = 0
    void natural.then(() => {
      thenCalls += 1
    })
    frame(renderer, 500)
    expect(await resolved(natural.finished)).toBe(false)
    frame(renderer, 600)
    expect(await resolved(natural.finished)).toBe(true)
    expect(thenCalls).toBe(1)

    const completed = animate(x, 0, linear)
    frame(renderer, 100)
    expect(await resolved(completed.finished)).toBe(false)
    completed.complete()
    expect(completed.state).toBe("finished")
    expect(await resolved(completed.finished)).toBe(true)
    frame(renderer, 0)
    expect(width()).toBe(0)
  })

  it("arms finished again when play() starts a finished animation", async () => {
    const { renderer, x } = mount()
    const controls = animate(x, 100, linear)
    frame(renderer, 1100)
    const first = controls.finished
    expect(await resolved(first)).toBe(true)

    controls.play()
    expect(controls.finished).not.toBe(first)
    expect(await resolved(controls.finished)).toBe(false)
    frame(renderer, 1100)
    expect(await resolved(controls.finished)).toBe(true)
  })

  it("leaves finished and then pending after stop(), cancel() and a replacement", async () => {
    const { renderer, x } = mount()
    const stopped = animate(x, 100, linear)
    frame(renderer, 300)
    stopped.stop()

    const cancelled = animate(x, 100, linear)
    frame(renderer, 300)
    cancelled.cancel()

    const replaced = animate(x, 100, linear)
    frame(renderer, 300)
    const replacement = animate(x, 0, linear)
    frame(renderer, 5000)

    for (const controls of [stopped, cancelled, replaced]) {
      expect(controls.state).toBe("idle")
      expect(await resolved(controls.finished)).toBe(false)
    }
    expect(await resolved(replacement.finished)).toBe(true)
  })

  it("keeps the value on stop() and ends the controls for good", () => {
    const { renderer, x, width } = mount()
    const controls = animate(x, 100, linear)
    frame(renderer, 400)
    controls.stop()
    expect(controls.state).toBe("idle")
    expect(x.isAnimating()).toBe(false)
    frame(renderer, 2000)
    expect(width()).toBe(40)

    controls.play()
    controls.time = 0.9
    controls.speed = 3
    controls.complete()
    frame(renderer, 2000)
    expect(controls.state).toBe("idle")
    expect(width()).toBe(40)
    expect(x.get()).toBeCloseTo(40, 6)
  })

  it("takes the value back to the first keyframe on cancel(), and runs again on play()", () => {
    const { renderer, x, width } = mount(10)
    const controls = animate(x, 110, linear)
    frame(renderer, 400)
    expect(width()).toBe(50)
    controls.cancel()
    expect(controls.state).toBe("idle")
    expect(x.isAnimating()).toBe(false)
    frame(renderer, 100)
    expect(width()).toBe(10)

    controls.play()
    expect(controls.state).toBe("running")
    expect(controls.time).toBeCloseTo(0, 6)
    frame(renderer, 250)
    expect(width()).toBe(35)
    frame(renderer, 1000)
    expect(width()).toBe(110)
    expect(controls.state).toBe("finished")

    // A cancel that found it finished leaves the value on its end, and play() starts from the first keyframe.
    controls.cancel()
    frame(renderer, 100)
    expect(width()).toBe(110)
    controls.play()
    frame(renderer, 500)
    expect(width()).toBe(60)
  })

  it("does not run a cancelled animation again once another animation took the value", () => {
    const { renderer, x, width } = mount()
    const first = animate(x, 100, linear)
    frame(renderer, 400)
    first.cancel()
    const second = animate(x, 20, linear)
    first.play()
    expect(first.state).toBe("idle")
    frame(renderer, 1000)
    expect(width()).toBe(20)
    expect(second.state).toBe("finished")
  })

  it("applies controls used before the value is bound when the animation starts", () => {
    const loose = motionValue(0)
    const controls = animate(loose, 100, linear)
    controls.speed = 2
    controls.time = 0.25
    const app = createTestRoot()
    app.renderer.clockPause()
    loose.bind(app.renderer)
    app.renderer.flush()
    expect(controls.speed).toBe(2)
    expect(controls.time).toBeCloseTo(0.25, 6)
    frame(app.renderer, 100)
    expect(loose.get()).toBeCloseTo(45, 6)
    controls.stop()
    loose.destroy()
  })

  describe("frames", () => {
    /** Frames a window draws on its own, the clock held: a running animation asks for each next one. */
    function framesDrawn(renderer: TestRenderer, display: number): number {
      const before = renderer.getPaintedFrameCount()
      for (let i = 0; i < display; i++) renderer.drawFrame()
      return renderer.getPaintedFrameCount() - before
    }

    it("asks for no frame while paused, and the frame after play() moves the element", () => {
      const { renderer, x, width } = mount()
      renderer.setInlineDraws(false)
      const controls = animate(x, 100, linear)
      frame(renderer, 400)
      for (let i = 0; i < 3; i++) renderer.drawFrame()
      expect(framesDrawn(renderer, 5), "a running animation asks for each frame").toBe(5)

      controls.pause()
      for (let i = 0; i < 3; i++) renderer.drawFrame()
      expect(framesDrawn(renderer, 5), "a paused one asks for none").toBe(0)
      expect(controls.state).toBe("paused")
      expect(x.isAnimating()).toBe(true)

      controls.play()
      renderer.clockFastForward(100)
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(width()).toBe(50)
      expect(framesDrawn(renderer, 5)).toBe(5)

      controls.speed = 0
      for (let i = 0; i < 3; i++) renderer.drawFrame()
      expect(framesDrawn(renderer, 5), "a speed of 0 holds it, and asks for none").toBe(0)
      expect(controls.state).toBe("running")
      controls.speed = 1
      renderer.clockFastForward(100)
      renderer.drawFrame()
      expect(width()).toBe(60)
    })
  })

  describe("when the root unmounts", () => {
    it("keeps a paused animation's value until the animation is stopped", () => {
      const { renderer, root, x } = mount()
      const controls = animate(x, 100, linear)
      frame(renderer, 400)
      controls.pause()

      // Not the test root's unmount: it disposes the window these reads go through.
      root.unmount()
      expect(renderer.getMotionValuePlayback(x.id)?.state).toBe("paused")
      expect(renderer.getMotionValue(x.id)).toBeCloseTo(40, 6)
      controls.play()
      frame(renderer, 100)
      expect(renderer.getMotionValue(x.id)).toBeCloseTo(50, 6)
      controls.pause()

      controls.stop()
      expect(() => renderer.getMotionValue(x.id)).toThrow(/unknown motion value/)
      expect(() => frame(renderer, 1000)).not.toThrow()
      renderer.dispose()
    })

    it("frees the node when the component stops the animation in onCleanup", () => {
      const app = createTestRoot()
      app.renderer.clockPause()
      let id = -1
      app.render(() => {
        const y = createMotionValue(0)
        id = y.id
        const controls = animate(y, 100, linear)
        controls.pause()
        onCleanup(() => controls.stop())
        return (
          <div style={row}>
            <div testId="box" style={{ width: y, height: 10 }} />
          </div>
        )
      })
      expect(app.renderer.getMotionValuePlayback(id)?.state).toBe("paused")

      app.root.unmount()
      expect(() => app.renderer.getMotionValue(id)).toThrow(/unknown motion value/)
      app.renderer.dispose()
    })
  })
})
