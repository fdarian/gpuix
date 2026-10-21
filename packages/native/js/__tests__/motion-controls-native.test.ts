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
  | ["pause" | "play" | "stop" | "cancel" | "complete"]
  | ["seek" | "speed", number]
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
    finished: boolean
  }>
}

type Row = {
  from: number
  to: number | number[]
  transition: Record<string, unknown>
  steps: Array<{ at: number; ops?: Op[] }>
  observations: Observation[]
}

const oracle: Record<string, Row> = JSON.parse(
  readFileSync(new URL("./fixtures/motion-controls-oracle.json", import.meta.url), "utf8")
)

const number = (value: number | "Infinity"): number => (value === "Infinity" ? Infinity : value)

const curve = (transition: Record<string, unknown>): MotionCurve =>
  ({ ...transition, ...(transition.repeat === "Infinity" ? { repeat: Infinity } : {}) }) as MotionCurve

/** Seconds from the end of the delay to the end of the last repeat. */
const totalOf = (transition: Record<string, unknown>): number => {
  if (transition.repeat === "Infinity") return Infinity
  const duration = (transition.duration as number | undefined) ?? 0.3
  const repeat = (transition.repeat as number | undefined) ?? 0
  return duration * (repeat + 1) + ((transition.repeatDelay as number | undefined) ?? 0) * repeat
}

const settle = () => new Promise<void>((done) => setTimeout(done, 0))

/** What a sequence of controls reads on the addon, in the shape of the oracle's observations. */
async function runNative(row: Row): Promise<Observation[]> {
  const renderer = new TestRenderer({ width: 400, height: 300 })
  renderer.clockPause()
  renderer.flush()
  const value = motionValue(row.from)
  value.bind(renderer)
  const controls: AnimationPlaybackControlsWithThen[] = []
  const finished: boolean[] = []
  const start = (to: unknown, transition: Record<string, unknown>) => {
    const index = controls.length
    const started = value.animate(to as number, curve(transition))
    controls.push(started)
    finished.push(false)
    void started.finished.then(() => {
      finished[index] = true
    })
  }
  start(row.to, row.transition)

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
        finished: finished[index],
      })),
    })
  }
  value.destroy()
  return observations
}

/**
 * Rows where Motion's `isAnimating()` goes stale: it is set when the value starts animating and cleared when an
 * animation finishes or the value's own `stop()` runs, so it stays true after the controls' `stop()` and
 * `cancel()` and stays false while an animation that finished is seeked or played again. The addon reads whether
 * the current animation is in effect, running or held. Checked as that, and Motion must really differ.
 */
const STALE_IS_ANIMATING = new Set([
  "stop while running",
  "stop while paused",
  "cancel while running",
  "cancel then play at speed 2",
  "cancel after the finish then play",
  "cancel keyframes",
  "seek while finished",
  "play after the finish",
  "play after the finish of a repeat",
  "complete while running",
  "speed -1 after a finish and play",
])

/**
 * Where the addon deliberately reads differently, by row: the step, the field, and what the addon reads.
 * Every entry is checked from both sides: the addon must read exactly this, and Motion must read
 * something else.
 */
const DEVIATIONS: Record<string, { why: string; differs: Array<[step: number, field: string, native: unknown]> }> = {
  "seek in a delayed animation": {
    why: "The time setter reads back as set. Motion counts the delay in the time it is given, so seeking to 0.4 of a 0.5 delay is still inside the delay and `time` reads 0; here it is 0.4 into the animation.",
    differs: [
      [2, "value", 40],
      [2, "c0.time", 0.4],
      [3, "value", 50],
      [3, "c0.time", 0.5],
      [4, "value", 90],
      [4, "c0.time", 0.9],
      [5, "value", 100],
      [5, "c0.time", 1],
      [5, "c0.state", "finished"],
      [5, "c0.finished", true],
      [5, "animating", false],
    ],
  },
  "speed 0 then 1": {
    why: "A speed other than 0 after 0 runs the animation again. Motion needs `play()` as well: its clock was held with the speed.",
    differs: [
      [5, "value", 50],
      [5, "c0.time", 0.5],
      [6, "value", 70],
      [6, "c0.time", 0.7],
      [7, "value", 100],
      [7, "c0.time", 1],
      [7, "c0.state", "finished"],
      [7, "c0.finished", true],
      [7, "animating", false],
      [8, "value", 100],
      [8, "c0.time", 1],
      [8, "c0.state", "finished"],
      [8, "c0.finished", true],
      [8, "animating", false],
    ],
  },
}

describe.skipIf(!hasNativeTestRenderer)("playback controls against Motion", () => {
  for (const [name, row] of Object.entries(oracle)) {
    it(name, async () => {
      const native = await runNative(row)
      const totals = [row.transition, ...row.steps.flatMap((step) => (step.ops ?? []).flatMap((op) => (op[0] === "animate" ? [op[2]] : [])))].map(totalOf)
      const declared = DEVIATIONS[name]
      // Motion solves a cubic bezier with 12 bisections, which is good to about 2e-4 of the range.
      const tolerance = row.transition.ease === "linear" ? 1e-3 : 0.1
      const seen: Array<[number, string, unknown]> = []

      const same = (field: string, step: number, actual: unknown, wanted: unknown, tolerance = 0) => {
        const equal =
          typeof actual === "number" && typeof wanted === "number"
            ? Math.abs(actual - wanted) <= tolerance || actual === wanted
            : actual === wanted
        if (equal) return
        const deviation = declared?.differs.find(([at, where]) => at === step && where === field)
        if (deviation) {
          seen.push([step, field, actual])
          const message = `${name} @${step} ${field} (deviates: ${declared.why})`
          if (typeof actual === "number" && typeof deviation[2] === "number") {
            expect(actual, message).toBeCloseTo(deviation[2], 9)
          } else {
            expect(actual, message).toEqual(deviation[2])
          }
          return
        }
        expect(actual, `${name} @${row.steps[step].at}ms (step ${step}) ${field}`).toEqual(wanted)
      }

      row.observations.forEach((want, step) => {
        const got = native[step]
        same("value", step, got.value, want.value, tolerance)
        if (STALE_IS_ANIMATING.has(name)) {
          const states = got.controls[got.controls.length - 1].state
          same("animating", step, got.animating, states === "running" || states === "paused")
        } else {
          same("animating", step, got.animating, want.animating)
        }
        want.controls.forEach((control, index) => {
          const read = got.controls[index]
          const where = `c${index}`
          // Motion's time keeps counting past the end by the frame it noticed it on, and a seek
          // past the end leaves it there; the addon stops at the end of the last repeat.
          same(`${where}.time`, step, read.time, Math.min(number(control.time), totals[index]), 1e-9)
          same(`${where}.speed`, step, read.speed, control.speed)
          same(`${where}.duration`, step, read.duration, number(control.duration), name === "the duration of a spring" ? 0.06 : 1e-9)
          same(`${where}.iterationDuration`, step, read.iterationDuration, number(control.iterationDuration), name === "the duration of a spring" ? 0.06 : 1e-9)
          same(`${where}.state`, step, read.state, control.state)
          same(`${where}.finished`, step, read.finished, control.finished)
        })
      })

      if (STALE_IS_ANIMATING.has(name)) {
        const differs = row.observations.some((want) => {
          const state = want.controls[want.controls.length - 1].state
          return want.animating !== (state === "running" || state === "paused")
        })
        expect(differs, `${name}: Motion's isAnimating() is not stale here`).toBe(true)
      }

      // A deviation that no longer happens is a stale entry.
      for (const [step, field] of declared?.differs ?? []) {
        expect(
          seen.some(([at, where]) => at === step && where === field),
          `${name}: the entry for step ${step} ${field} no longer deviates`
        ).toBe(true)
      }
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
    expect(() => controls.duration).toThrow(/not known/)
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
