import { readFileSync } from "node:fs"
import { describe, expect, it } from "vitest"
import { animate, motionValue } from "../host.js"
import type { AnimationPlaybackControlsWithThen, MotionCurve } from "../host.js"
import { TestRenderer, hasNativeTestRenderer } from "../testing.js"

/**
 * The playback controls of an animation on the real addon, against Motion 14.1.0 itself: the same
 * sequence of controls on the same clock, read after every step. The oracle is
 * fixtures/motion-controls-oracle.mjs; regenerate the JSON with it.
 */

type Op =
  | ["pause" | "play" | "stop" | "cancel" | "complete" | "valueStop" | "detach"]
  | ["seek" | "speed" | "progress" | "valueJump", number]
  | ["attach", "pausing" | "plain"]
  | ["animate", unknown, Record<string, unknown>]
  | ["controls", number, "pause" | "play" | "stop" | "cancel" | "complete"]

type Observation = {
  at: number
  value: number
  animating: boolean
  controls: Array<{
    time: number | "Infinity"
    speed: number
    duration: number | "Infinity"
    iterationDuration: number | "Infinity"
    state: string
    /** Milliseconds after the animation was started, `"origin"` for 0 on the clock, `null` for none. */
    startTime: number | "origin" | null
    finished: boolean
  }>
  timelines?: Array<{ observed: number; cleaned: number }>
}

type Row = {
  from: number
  to: number | number[]
  transition: Record<string, unknown>
  steps: Array<{ at: number; ops?: Op[] }>
  early?: { duration: number | "Infinity"; iterationDuration: number | "Infinity" }
  observations: Observation[]
}

const oracle: Record<string, Row> = JSON.parse(
  readFileSync(new URL("./fixtures/motion-controls-oracle.json", import.meta.url), "utf8")
)

const number = (value: number | "Infinity"): number => (value === "Infinity" ? Infinity : value)

const curve = (transition: Record<string, unknown>): MotionCurve =>
  ({ ...transition, ...(transition.repeat === "Infinity" ? { repeat: Infinity } : {}) }) as MotionCurve

const settle = () => new Promise<void>((done) => setTimeout(done, 0))

/** What a sequence of controls reads on the addon, in the shape of the oracle's observations. */
async function runNative(row: Row): Promise<{ early?: Row["early"]; observations: Observation[] }> {
  const renderer = new TestRenderer({ width: 400, height: 300 })
  renderer.clockPause()
  // Far from the clock's origin, which a start time of 0 means.
  const started = renderer.clockSet(5000)
  renderer.flush()
  const value = motionValue(row.from)
  value.bind(renderer)
  const controls: AnimationPlaybackControlsWithThen[] = []
  const finished: boolean[] = []
  const timelines: Array<{ observed: number; cleaned: number; animation?: AnimationPlaybackControlsWithThen }> = []
  const detachers: Array<() => void> = []
  const start = (to: unknown, transition: Record<string, unknown>) => {
    const index = controls.length
    const created = value.animate(to as number, curve(transition))
    controls.push(created)
    finished.push(false)
    void created.finished.then(() => {
      finished[index] = true
    })
  }
  start(row.to, row.transition)
  const early = row.early
    ? { duration: controls[0].duration, iterationDuration: controls[0].iterationDuration }
    : undefined

  const observations: Observation[] = []
  let now = 0
  for (const step of row.steps) {
    renderer.clockFastForward(step.at - now)
    now = step.at
    for (const op of step.ops ?? []) {
      const current = controls[controls.length - 1]
      switch (op[0]) {
        case "pause":
        case "play":
        case "stop":
        case "cancel":
        case "complete":
          current[op[0]]()
          break
        case "seek":
          current.time = op[1]
          break
        case "speed":
          current.speed = op[1]
          break
        case "animate":
          start(op[1], op[2])
          break
        case "controls":
          controls[op[1]][op[2]]()
          break
        case "valueStop":
          value.stop()
          break
        case "valueJump":
          value.jump(op[1])
          break
        case "attach": {
          const line: (typeof timelines)[number] = { observed: 0, cleaned: 0 }
          timelines.push(line)
          detachers.push(
            current.attachTimeline({
              observe(animation) {
                line.observed += 1
                line.animation = animation
                if (op[1] === "pausing") animation.pause()
                return () => {
                  line.cleaned += 1
                }
              },
            })
          )
          break
        }
        case "progress": {
          const animation = timelines[timelines.length - 1].animation
          if (!animation) throw new Error("no timeline observed an animation")
          animation.time = animation.iterationDuration * op[1]
          break
        }
        case "detach":
          detachers[detachers.length - 1]()
          break
      }
    }
    renderer.flush()
    renderer.dispatchNativeEvents()
    await settle()
    observations.push({
      at: step.at,
      value: value.get(),
      animating: value.isAnimating(),
      controls: controls.map((control, index) => ({
        time: control.time,
        speed: control.speed,
        duration: control.duration,
        iterationDuration: control.iterationDuration,
        state: control.state,
        startTime: control.startTime === null ? null : control.startTime === 0 ? "origin" : control.startTime - started,
        finished: finished[index],
      })),
      ...(timelines.length > 0
        ? { timelines: timelines.map((line) => ({ observed: line.observed, cleaned: line.cleaned })) }
        : {}),
    })
  }
  value.destroy()
  return { early, observations }
}

describe.skipIf(!hasNativeTestRenderer)("playback controls against Motion", () => {
  for (const [name, row] of Object.entries(oracle)) {
    it(name, async () => {
      const native = await runNative(row)
      // Motion solves a cubic bezier with 12 bisections, which is good to about 2e-4 of the range.
      const tolerance = row.transition.ease === "linear" ? 1e-3 : 0.1
      const spring = name.includes("spring") ? 0.06 : 1e-9

      if (row.early) {
        expect(native.early?.duration, `${name} duration before a frame`).toBeCloseTo(number(row.early.duration), spring === 1e-9 ? 9 : 1)
        expect(native.early?.iterationDuration, `${name} iterationDuration before a frame`).toBeCloseTo(
          number(row.early.iterationDuration),
          spring === 1e-9 ? 9 : 1
        )
      }

      row.observations.forEach((want, step) => {
        const got = native.observations[step]
        const where = (field: string) => `${name} @${row.steps[step].at}ms (step ${step}) ${field}`
        expect(Math.abs(got.value - want.value), where("value")).toBeLessThanOrEqual(tolerance)
        expect(got.animating, where("animating")).toBe(want.animating)
        want.controls.forEach((control, index) => {
          const read = got.controls[index]
          const field = (key: string) => where(`c${index}.${key}`)
          expect(read.time, field("time")).toBeCloseTo(number(control.time), 9)
          expect(read.speed, field("speed")).toBe(control.speed)
          expect(read.duration, field("duration")).toBeCloseTo(number(control.duration), spring === 1e-9 ? 9 : 1)
          expect(read.iterationDuration, field("iterationDuration")).toBeCloseTo(
            number(control.iterationDuration),
            spring === 1e-9 ? 9 : 1
          )
          expect(read.state, field("state")).toBe(control.state)
          expect(read.finished, field("finished")).toBe(control.finished)
          if (typeof control.startTime === "number" && typeof read.startTime === "number") {
            expect(read.startTime, field("startTime")).toBeCloseTo(control.startTime, 6)
          } else {
            expect(read.startTime, field("startTime")).toBe(control.startTime)
          }
        })
        expect(got.timelines, where("timelines")).toEqual(want.timelines)
      })
    })
  }
})

describe.skipIf(!hasNativeTestRenderer)("playback controls on the addon", () => {
  const linear = { duration: 1, ease: "linear" } as const

  const mount = () => {
    const renderer = new TestRenderer({ width: 400, height: 300 })
    renderer.clockPause()
    renderer.flush()
    return renderer
  }

  const frame = (renderer: TestRenderer, ms: number) => {
    renderer.clockFastForward(ms)
    renderer.flush()
    renderer.dispatchNativeEvents()
  }

  it("takes a running animation over with the value's own stop(), which clears isAnimating() at once", () => {
    // The sequence of a list that scrolls to the keyboard row: stop the travel if there is one, then travel again.
    const renderer = mount()
    const x = motionValue(0)
    x.bind(renderer)
    x.animate(100, linear)
    frame(renderer, 400)
    expect(x.isAnimating()).toBe(true)
    x.stop()
    expect(x.isAnimating()).toBe(false)
    expect(x.get()).toBeCloseTo(40, 5)
    const next = x.animate(0, linear)
    expect(x.isAnimating()).toBe(true)
    frame(renderer, 500)
    expect(x.get()).toBeCloseTo(20, 5)
    next.stop()
    expect(x.isAnimating()).toBe(true)
  })

  it("runs Motion's default curve for a value with no key: 0.3 s, 0.8 s for more than two keyframes", () => {
    const renderer = mount()
    const x = motionValue(0)
    x.bind(renderer)
    const plain = x.animate(100)
    expect(plain.duration).toBeCloseTo(0.3, 5)
    frame(renderer, 150)
    // cubic-bezier(0.25, 0.1, 0.35, 1) is at 0.5 of the time a little under three quarters of the way.
    expect(x.get()).toBeGreaterThan(70)
    expect(x.get()).toBeLessThan(80)
    frame(renderer, 200)
    expect(x.get()).toBe(100)
    const keyframes = x.animate([0, 50, 0])
    expect(keyframes.duration).toBeCloseTo(0.8, 5)
    const delayed = x.animate(10, { delay: 0.2 })
    expect(delayed.duration).toBeCloseTo(0.3, 5)
    expect(x.animate(10, { repeat: 1 }).iterationDuration).toBeCloseTo(0.3, 5)
  })

  it("holds a replaced animation's controls inert", async () => {
    const renderer = mount()
    const x = motionValue(0)
    x.bind(renderer)
    const first = x.animate(100, linear)
    frame(renderer, 400)
    const second = x.animate(0, { duration: 2, ease: "linear" })
    first.pause()
    first.speed = 4
    first.time = 0.9
    first.complete()
    first.play()
    expect(first.state).toBe("idle")
    expect(first.speed).toBe(1)
    expect(first.time).toBeCloseTo(0.4, 5)
    frame(renderer, 1000)
    expect(x.get()).toBeCloseTo(20, 5)
    expect(second.state).toBe("running")
    expect(second.time).toBeCloseTo(1, 5)
  })

  it("applies controls used before the value is bound when the animation starts", () => {
    const renderer = mount()
    const x = motionValue(0)
    const controls = x.animate(100, linear)
    controls.speed = 2
    controls.time = 0.25
    controls.pause()
    expect(controls.state).toBe("paused")
    expect(controls.speed).toBe(2)
    expect(controls.time).toBe(0.25)
    expect(controls.duration).toBe(1)
    expect(controls.startTime).toBeNull()
    x.bind(renderer)
    expect(x.get()).toBeCloseTo(25, 5)
    frame(renderer, 5000)
    expect(x.get()).toBeCloseTo(25, 5)
    expect(controls.duration).toBe(1)
    controls.play()
    frame(renderer, 250)
    expect(x.get()).toBeCloseTo(75, 5)
  })

  it("ends a controls' complete() before it is bound on the last keyframe", async () => {
    const renderer = mount()
    const x = motionValue(0)
    const controls = x.animate([10, 20, 30], linear)
    controls.complete()
    expect(controls.state).toBe("finished")
    x.bind(renderer)
    await controls.finished
    expect(x.get()).toBe(30)
    expect(controls.state).toBe("finished")
  })

  it("asks for no frame while held, and keeps the value for the elements that bind it", () => {
    const renderer = mount()
    renderer.applyBatch(
      JSON.stringify([
        ["createElement", 1, "div"],
        ["setRoot", 1],
        ["setStyle", 1, { display: "flex" }],
        ["createElement", 2, "div"],
        ["appendChild", 1, 2],
        ["setStyle", 2, { height: 20, width: 5 }],
      ])
    )
    const x = motionValue(10)
    x.bind(renderer)
    renderer.applyBatch(JSON.stringify([["setCustomProp", 2, "motionValues", { width: x.id }]]))
    renderer.flush()
    const controls = x.animate(110, linear)
    frame(renderer, 500)
    expect(renderer.getElementBounds(2)?.width).toBeCloseTo(60, 0)
    controls.pause()
    frame(renderer, 3000)
    expect(renderer.getElementBounds(2)?.width).toBeCloseTo(60, 0)
    expect(x.isAnimating()).toBe(true)
    expect(x.getVelocity()).toBe(0)
    controls.time = 0.9
    frame(renderer, 16)
    expect(renderer.getElementBounds(2)?.width).toBeCloseTo(100, 0)
    controls.play()
    frame(renderer, 200)
    expect(renderer.getElementBounds(2)?.width).toBe(110)
    expect(x.isAnimating()).toBe(false)
  })

  it("reports the velocity at the pace of the animation, and 0 while held", () => {
    const renderer = mount()
    const x = motionValue(0)
    x.bind(renderer)
    const controls = x.animate(100, linear)
    controls.speed = 2
    frame(renderer, 100)
    expect(x.getVelocity()).toBeCloseTo(200, 5)
    controls.speed = -1
    expect(x.getVelocity()).toBeCloseTo(-100, 5)
    controls.pause()
    expect(x.getVelocity()).toBe(0)
  })

  it("completes at the start when a reversed animation gets there, and a listener hears it once", async () => {
    const renderer = mount()
    const x = motionValue(0)
    x.bind(renderer)
    const log: string[] = []
    x.on("animationStart", () => log.push("start"))
    x.on("animationComplete", () => log.push("complete"))
    const controls = x.animate(100, linear)
    frame(renderer, 600)
    controls.speed = -1
    frame(renderer, 599)
    expect(controls.state).toBe("running")
    frame(renderer, 1)
    await settle()
    expect(controls.state).toBe("finished")
    expect(x.get()).toBe(0)
    expect(log).toEqual(["start", "complete"])
    await controls.finished
  })

  it("a new animation after a pause starts from the held value", () => {
    const renderer = mount()
    const x = motionValue(0)
    x.bind(renderer)
    const first = x.animate(100, linear)
    frame(renderer, 400)
    first.pause()
    frame(renderer, 1000)
    x.animate(0, linear)
    expect(x.get()).toBeCloseTo(40, 5)
    expect(x.getVelocity()).toBe(0)
    frame(renderer, 500)
    expect(x.get()).toBeCloseTo(20, 5)
  })

  it("destroys the temporary value of animate(from, to) when the run ends any way", async () => {
    const renderer = mount()
    const seen: number[] = []
    const finishing = animate(0, 10, { host: renderer, ...linear, onUpdate: (v) => seen.push(v) })
    frame(renderer, 500)
    finishing.pause()
    frame(renderer, 500)
    expect(finishing.state).toBe("paused")
    finishing.play()
    frame(renderer, 600)
    await finishing.finished
    expect(finishing.state).toBe("finished")
    expect(seen.at(-1)).toBe(10)

    const stopped = animate(0, 10, { host: renderer, ...linear })
    frame(renderer, 100)
    stopped.stop()
    await settle()
    expect(stopped.state).toBe("idle")
    const cancelled = animate(0, 10, { host: renderer, ...linear })
    cancelled.cancel()
    await settle()
    expect(cancelled.state).toBe("idle")
  })
})
