// Runs Motion's own `animate` on a motion value (motion 14.1.0: what `animate(mv, ...)` returns, a
// GroupAnimationWithThen over a JSAnimation) through sequences of playback controls, on a clock the
// script steps, and records what the controls and the value read after each step.
// Usage: MOTION_ROOT=<node_modules holding motion> bun motion-controls-oracle.mjs > motion-controls-oracle.json
//
// A row is a start (`from`, `to`, `transition`) at time 0 and `steps`. A step sets the clock to `at`
// milliseconds, applies its `ops` in order, runs one frame and reads. Ops: ["pause"], ["play"], ["stop"],
// ["cancel"], ["complete"], ["seek", seconds], ["speed", rate], ["animate", to, transition] (another animation of
// the same value; its controls are appended), ["valueStop"] and ["valueJump", to] (the value's own stop() and jump()),
// ["attach", "pausing" | "plain"] (attachTimeline of a hand-made timeline whose observe() pauses the animation or not
// and returns a cleanup that is counted), ["progress", p] (what framer-motion's scroll attach-animation does to the
// observed animation: `anim.time = anim.iterationDuration * p`) and ["detach"] (the function attachTimeline
// returned). A row with `early` also records what the controls read synchronously after animate(), before any
// frame. "Infinity" stands for Infinity, as JSON has none.
import { pathToFileURL } from "node:url"
import path from "node:path"

const root = process.env.MOTION_ROOT
if (!root) throw new Error("set MOTION_ROOT to the node_modules directory that holds motion")

let clock = 0
let frames = []
Object.defineProperty(globalThis, "performance", { value: { now: () => clock }, configurable: true, writable: true })
globalThis.requestAnimationFrame = (callback) => frames.push(callback)
globalThis.cancelAnimationFrame = () => {}
const { animate, motionValue } = await import(pathToFileURL(path.join(root, "motion/dist/es/index.mjs")).href)

const settle = () => new Promise((done) => setTimeout(done, 0))

const revive = (value) => (value === "Infinity" ? Infinity : value)
const options = (transition) => {
  const out = { ...transition }
  if ("repeat" in out) out.repeat = revive(out.repeat)
  return out
}
const encode = (number) => (number === Infinity ? "Infinity" : number === -Infinity ? "-Infinity" : number)

// The legs of the fluid-hover demo's cursor (docs/fluid-hover/demos.tsx): three round trips, each faster.
const CURSOR_LEGS = [2, 2, 0.8, 0.8, 0.3, 0.3]
const CURSOR_PASS = CURSOR_LEGS.reduce((a, b) => a + b, 0)
const CURSOR_TIMES = CURSOR_LEGS.reduce((acc, leg) => [...acc, acc[acc.length - 1] + leg / CURSOR_PASS], [0])
const CURSOR = {
  from: 2,
  to: [2, 210, 2, 210, 2, 210, 2],
  transition: { duration: CURSOR_PASS, times: CURSOR_TIMES, ease: "easeInOut", repeat: "Infinity" },
}
const LINEAR = { duration: 1, ease: "linear" }
const at = (...times) => times.map((time) => ({ at: time }))
const grid = (to, step) => Array.from({ length: Math.round(to / step) + 1 }, (_, index) => ({ at: index * step }))
const withOps = (time, ...ops) => ({ at: time, ops })

const ROWS = {
  "fluid-hover loop paused mid-loop and played again": {
    ...CURSOR,
    steps: [
      ...at(0, 1000, 3500),
      withOps(3500, ["pause"]),
      ...at(6000, 9000),
      withOps(9000, ["play"]),
      ...at(9500, 12000, 15700, 16000, 25000, 40000),
    ],
  },
  "fluid-hover loop paused in a turnaround": {
    ...CURSOR,
    steps: [...at(0), withOps(4000, ["pause"]), ...at(5000), withOps(5000, ["play"]), ...at(5100, 6000, 6200 + 1000)],
  },
  "pause and play twice": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0, 200),
      withOps(200, ["pause"]),
      withOps(500, ["pause"]),
      withOps(700, ["play"]),
      ...at(800),
      withOps(800, ["play"]),
      withOps(900, ["pause"]),
      withOps(1500, ["play"]),
      ...at(1600, 1800, 2000, 2500),
    ],
  },
  "pause during the delay": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 200), withOps(200, ["pause"]), ...at(1000), withOps(1000, ["play"]), ...at(1200, 1500, 1700, 2000, 2200)],
  },
  "pause after the delay": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 800), withOps(800, ["pause"]), ...at(2000), withOps(2000, ["play"]), ...at(2300, 2500, 3000)],
  },
  "seek while running": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 200), withOps(200, ["seek", 0.7]), ...at(300, 500, 600), ...at(1200)],
  },
  "seek while paused": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0, 200),
      withOps(200, ["pause"]),
      withOps(400, ["seek", 0.6]),
      ...at(600),
      withOps(700, ["seek", 0.1]),
      ...at(800),
      withOps(800, ["play"]),
      ...at(900, 1500, 1800),
    ],
  },
  "seek while finished": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1200), withOps(1500, ["seek", 0.4]), ...at(1600, 2500), withOps(2500, ["play"]), ...at(2700, 3200, 4000)],
  },
  "seek past the end while running": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 200), withOps(200, ["seek", 5]), ...at(300, 1000)],
  },
  "seek past the end while paused": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 200), withOps(200, ["pause"]), withOps(300, ["seek", 5]), ...at(400), withOps(500, ["play"]), ...at(600, 1000)],
  },
  "seek into a repeat iteration": {
    from: 0,
    to: [0, 100, 50],
    transition: { duration: 2, ease: "linear", times: [0, 0.5, 1], repeat: 2, repeatType: "reverse" },
    steps: [...at(0, 500), withOps(500, ["seek", 3.5]), ...at(600, 1500, 2500), withOps(2500, ["seek", 5.9]), ...at(2600, 2900, 4000)],
  },
  "seek back to zero": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 600), withOps(600, ["seek", 0]), ...at(700, 1000)],
  },
  "seek in a delayed animation": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 100), withOps(100, ["seek", 0.4]), ...at(200, 600, 700, 1500, 1700)],
  },
  "speed 2": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [withOps(0, ["speed", 2]), ...at(100, 250, 400, 499, 500, 600)],
  },
  "speed 2 set midway": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 250), withOps(250, ["speed", 2]), ...at(300, 500, 600, 900)],
  },
  "speed 0.5": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [withOps(0, ["speed", 0.5]), ...at(500, 1000, 1500, 1999, 2000, 2500)],
  },
  "speed 0 holds, running": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["speed", 0]), ...at(500, 2000)],
  },
  "speed 0 then 1": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["speed", 0]), ...at(2000), withOps(2000, ["speed", 1]), ...at(2100, 2300, 2700, 3000)],
  },
  "speed 0 then play": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["speed", 0]), ...at(2000), withOps(2000, ["speed", 1], ["play"]), ...at(2100, 2300, 2700, 3000)],
  },
  "speed -1 from the middle": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 600), withOps(600, ["speed", -1]), ...at(700, 800, 1000, 1199, 1200, 1300, 2000)],
  },
  "speed -1 through repeats": {
    from: 0,
    to: [0, 100],
    transition: { duration: 1, ease: "linear", repeat: 3 },
    steps: [...at(0, 2500), withOps(2500, ["speed", -1]), ...at(2600, 3000, 3500, 4500, 5000, 5500)],
  },
  "speed -1 after a finish and play": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1500), withOps(1500, ["speed", -1]), ...at(1600), withOps(1700, ["play"]), ...at(1800, 2000, 2500, 2700, 3000)],
  },
  "speed 2 while paused": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 200), withOps(200, ["pause"]), withOps(300, ["speed", 2]), ...at(500), withOps(500, ["play"]), ...at(600, 800, 1000)],
  },
  "stop while running": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["stop"]), ...at(500, 2000), withOps(2000, ["play"]), ...at(2500)],
  },
  "stop while paused": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["pause"]), withOps(500, ["stop"]), ...at(600, 2000)],
  },
  "stop after the finish": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1200), withOps(1500, ["stop"]), ...at(1600)],
  },
  "cancel while running": {
    from: 10,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["cancel"]), ...at(500, 2000), withOps(2000, ["play"]), ...at(2100)],
  },
  "cancel then play at speed 2": {
    from: 10,
    to: 100,
    transition: LINEAR,
    steps: [withOps(0, ["speed", 2]), ...at(200), withOps(200, ["cancel"]), ...at(300), withOps(300, ["play"]), ...at(400, 600, 900)],
  },
  "cancel after the finish then play": {
    from: 10,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1200), withOps(1500, ["cancel"]), withOps(1600, ["play"]), ...at(1700, 2300)],
  },
  "cancel keyframes": {
    from: 0,
    to: [20, 60, 30],
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["cancel"]), ...at(500, 2000)],
  },
  "cancel after the finish": {
    from: 10,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1200), withOps(1500, ["cancel"]), ...at(1600, 2000)],
  },
  "complete while running": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["complete"]), ...at(500, 2000), withOps(2000, ["play"]), ...at(2100, 2600, 3200)],
  },
  "complete while paused": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["pause"]), withOps(500, ["complete"]), ...at(600, 2000)],
  },
  "complete a loop forever": {
    from: 2,
    to: [2, 210, 2],
    transition: { duration: 1, ease: "linear", repeat: "Infinity" },
    steps: [...at(0, 2300), withOps(2300, ["complete"]), ...at(2400, 5000)],
  },
  "complete a reversed animation": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 600), withOps(600, ["speed", -1]), withOps(700, ["complete"]), ...at(800, 2000)],
  },
  "complete after the finish": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1500), withOps(1500, ["complete"]), ...at(1600)],
  },
  "replacement": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0, 400),
      withOps(400, ["animate", 0, { duration: 2, ease: "linear" }]),
      ...at(500, 1000),
      ...at(1100, 2400, 3000),
    ],
  },
  "replacement after a pause": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0, 400),
      withOps(400, ["pause"]),
      withOps(900, ["animate", 0, { duration: 1, ease: "linear" }]),
      ...at(1000, 1500, 2000),
      withOps(2000, ["controls", 0, "play"]),
      ...at(2100),
    ],
  },
  "replacement after the finish": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1200), withOps(1500, ["animate", 50, { duration: 1, ease: "linear" }]), ...at(1600, 2600)],
  },
  "play after the finish": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1200), withOps(1500, ["play"]), ...at(1600, 2000, 2500, 3000)],
  },
  "play after the finish of a repeat": {
    from: 0,
    to: [0, 100],
    transition: { duration: 1, ease: "linear", repeat: 1, repeatType: "reverse" },
    steps: [...at(0, 2200), withOps(2500, ["play"]), ...at(2600, 3500, 4500, 5000)],
  },
  "play while running": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["play"]), ...at(600, 1000, 1200)],
  },
  "duration of a tween, a delay and repeats": {
    from: 0,
    to: 100,
    transition: { duration: 2, ease: "linear", delay: 0.5, repeat: 3, repeatDelay: 0.25 },
    steps: [...at(0, 600), withOps(600, ["seek", 100]), ...at(700, 700)],
  },
  "the time of a delayed animation": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: grid(2000, 250),
  },
  "the time of repeats with a repeat delay": {
    from: 0,
    to: [0, 100],
    transition: { duration: 1, ease: "linear", repeat: 2, repeatDelay: 0.5 },
    steps: grid(5000, 500),
  },
  "the duration of a spring": {
    from: 0,
    to: 100,
    transition: { type: "spring", stiffness: 200, damping: 12 },
    steps: at(0, 100),
  },
  "seek past the delay of a delayed animation": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 100), withOps(100, ["seek", 0.8]), ...at(200, 500, 700, 1500)],
  },
  "seek into the delay while running past it": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 800), withOps(800, ["seek", 0.2]), ...at(900, 1000, 1200, 1700, 2000)],
  },
  "seek into the delay while paused": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [
      ...at(0, 100),
      withOps(100, ["pause"]),
      withOps(200, ["seek", 0.3]),
      ...at(300),
      withOps(400, ["play"]),
      ...at(500, 800, 1000, 1500),
    ],
  },
  "seek past the delay while paused": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [
      ...at(0, 100),
      withOps(100, ["pause"]),
      withOps(200, ["seek", 0.9]),
      ...at(300),
      withOps(400, ["play"]),
      ...at(500, 800, 1000, 1500),
    ],
  },
  "seek to the end of the delay": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 100), withOps(100, ["seek", 0.5]), ...at(200, 500, 1000)],
  },
  "seek into the delay of a finished animation": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 1800), withOps(2000, ["seek", 0.2]), ...at(2100, 2600, 3000, 3500)],
  },
  "seek past the end of the repeats": {
    from: 0,
    to: [0, 100],
    transition: { duration: 1, ease: "linear", repeat: 2, repeatDelay: 0.5 },
    steps: [...at(0, 200), withOps(200, ["seek", 10]), ...at(300, 1000), withOps(1000, ["play"]), ...at(1100, 2000)],
  },
  "seek past the end of a delayed animation": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 100), withOps(100, ["seek", 7]), ...at(200, 1000), withOps(1000, ["speed", -1]), ...at(1100, 1500, 3000)],
  },
  "seek past the end then play": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 200), withOps(200, ["pause"]), withOps(300, ["seek", 5]), ...at(400), withOps(500, ["play"]), ...at(600, 700)],
  },
  "seek past the end while reversed": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 200), withOps(200, ["speed", -1]), withOps(300, ["seek", 5]), ...at(400, 700, 1500, 2000)],
  },
  "speed 0 before the first frame": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [withOps(0, ["speed", 0]), ...at(0, 500), withOps(500, ["speed", 1]), ...at(600), withOps(700, ["play"]), ...at(800, 1200, 1700)],
  },
  "speed 0 then 2 then play": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["speed", 0]), ...at(1000), withOps(1000, ["speed", 2]), ...at(1100), withOps(1200, ["play"]), ...at(1300, 1500, 1700)],
  },
  "speed 0 then pause then speed 1": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0, 400),
      withOps(400, ["speed", 0]),
      withOps(500, ["pause"]),
      withOps(600, ["speed", 1]),
      ...at(700),
      withOps(800, ["play"]),
      ...at(900, 1200, 1500),
    ],
  },
  "speed 0 then seek then speed 1": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["speed", 0]), withOps(500, ["seek", 0.7]), ...at(600), withOps(700, ["speed", 1]), ...at(800, 1000), withOps(1000, ["play"]), ...at(1100, 1300, 1600)],
  },
  "speed 0 then play without a speed": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["speed", 0]), ...at(1000), withOps(1000, ["play"]), ...at(1100, 2000)],
  },
  "speed 0 then -1 then play": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 600), withOps(600, ["speed", 0]), ...at(900), withOps(900, ["speed", -1]), ...at(1000), withOps(1100, ["play"]), ...at(1200, 1500, 1800)],
  },
  "speed 0 in a delayed animation": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [...at(0, 200), withOps(200, ["speed", 0]), ...at(900), withOps(900, ["speed", 1]), ...at(1000), withOps(1000, ["play"]), ...at(1100, 1400, 1700, 2000)],
  },
  "value stop while running": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["valueStop"]), ...at(500, 2000), withOps(2000, ["play"]), ...at(2100, 2600)],
  },
  "value stop while paused": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["pause"]), withOps(500, ["valueStop"]), ...at(600, 2000)],
  },
  "value stop after the finish": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 1200), withOps(1500, ["valueStop"]), ...at(1600)],
  },
  "value jump while running": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["valueJump", 7]), ...at(500, 2000), withOps(2000, ["play"]), ...at(2100)],
  },
  "value jump while paused": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["pause"]), withOps(500, ["valueJump", 7]), ...at(600, 2000)],
  },
  "controls stop then a new animation": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["stop"]), ...at(500), withOps(500, ["animate", 0, { duration: 1, ease: "linear" }]), ...at(600, 1600)],
  },
  "controls stop then value stop": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["stop"]), ...at(500), withOps(600, ["valueStop"]), ...at(700)],
  },
  "controls cancel then value jump": {
    from: 10,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["cancel"]), ...at(500), withOps(600, ["valueJump", 3]), ...at(700, 800)],
  },
  "complete then play": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 400), withOps(400, ["complete"]), ...at(500), withOps(600, ["play"]), ...at(700, 1000)],
  },
  "the duration of a tween read before a frame": {
    early: true,
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: at(0, 1500),
  },
  "the duration of a delayed, repeated animation read before a frame": {
    early: true,
    from: 0,
    to: 100,
    transition: { duration: 2, ease: "linear", delay: 0.5, repeat: 3, repeatDelay: 0.25 },
    steps: at(0, 600),
  },
  "the duration of keyframes with times read before a frame": {
    early: true,
    from: 5,
    to: [5, 80, 20],
    transition: { duration: 3, times: [0, 0.4, 1], ease: "easeInOut", delay: 1 },
    steps: at(0, 1500),
  },
  "the duration of the default tween read before a frame": {
    early: true,
    from: 0,
    to: 100,
    transition: {},
    steps: at(0, 400),
  },
  "the duration of a spring read before a frame": {
    early: true,
    from: 0,
    to: 100,
    transition: { type: "spring", stiffness: 200, damping: 12 },
    steps: at(0, 100),
  },
  "the duration of a spring with a delay read before a frame": {
    early: true,
    from: 0,
    to: 100,
    transition: { type: "spring", stiffness: 300, damping: 30, mass: 2, delay: 0.25 },
    steps: at(0, 100),
  },
  "the duration of an infinite loop read before a frame": {
    early: true,
    from: 2,
    to: [2, 210, 2],
    transition: { duration: 1.5, ease: "linear", repeat: "Infinity" },
    steps: at(0, 100),
  },
  "the duration of a mirrored repeat read before a frame": {
    early: true,
    from: 0,
    to: [0, 100],
    transition: { duration: 0.5, ease: "linear", repeat: 2, repeatType: "mirror" },
    steps: at(0, 100),
  },
  "a timeline scrubs a paused animation": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0),
      withOps(0, ["attach", "pausing"]),
      ...at(100),
      withOps(200, ["progress", 0.5]),
      withOps(1000, ["progress", 0.25]),
      withOps(1100, ["progress", 1]),
      ...at(1200),
      withOps(1300, ["progress", 0]),
      ...at(1400, 3000),
    ],
  },
  "a timeline scrubs a delayed animation": {
    from: 0,
    to: 100,
    transition: { ...LINEAR, delay: 0.5 },
    steps: [
      withOps(0, ["attach", "pausing"], ["progress", 0.2]),
      ...at(100),
      withOps(300, ["progress", 0.5]),
      withOps(400, ["progress", 1]),
      withOps(500, ["progress", 0.75]),
      ...at(600, 2000),
    ],
  },
  "a timeline scrubs repeats": {
    from: 0,
    to: [0, 100],
    transition: { duration: 1, ease: "linear", repeat: 2, repeatType: "reverse" },
    steps: [
      ...at(0),
      withOps(0, ["attach", "pausing"]),
      withOps(100, ["progress", 0.5]),
      withOps(200, ["progress", 1]),
      withOps(300, ["progress", 0.1]),
      ...at(400, 2000),
    ],
  },
  "a timeline attached before the first frame": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [withOps(0, ["attach", "pausing"], ["progress", 0.3]), ...at(200, 1500), withOps(1500, ["progress", 0.8]), ...at(1600)],
  },
  "a timeline that does not pause stops the clock": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0, 200), withOps(200, ["attach", "plain"]), ...at(300, 800), withOps(900, ["progress", 0.4]), ...at(1000, 2500)],
  },
  "play after a timeline gives the clock back": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0),
      withOps(0, ["attach", "pausing"]),
      withOps(100, ["progress", 0.2]),
      ...at(200),
      withOps(300, ["play"]),
      ...at(400, 700, 1200, 1500),
    ],
  },
  "stop after a timeline cleans it up": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0),
      withOps(0, ["attach", "pausing"]),
      withOps(100, ["progress", 0.4]),
      ...at(200),
      withOps(300, ["stop"]),
      ...at(400, 1500),
    ],
  },
  "the returned function of a timeline stops the animation": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0),
      withOps(0, ["attach", "pausing"]),
      withOps(100, ["progress", 0.4]),
      ...at(200),
      withOps(300, ["detach"]),
      ...at(400, 1500),
    ],
  },
  "cancel after a timeline": {
    from: 10,
    to: 100,
    transition: LINEAR,
    steps: [...at(0), withOps(0, ["attach", "pausing"]), withOps(100, ["progress", 0.6]), ...at(200), withOps(300, ["cancel"]), ...at(400, 1500)],
  },
  "complete after a timeline": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [...at(0), withOps(0, ["attach", "pausing"]), withOps(100, ["progress", 0.6]), ...at(200), withOps(300, ["complete"]), ...at(400, 1500)],
  },
  "a timeline on a replaced animation": {
    from: 0,
    to: 100,
    transition: LINEAR,
    steps: [
      ...at(0),
      withOps(0, ["attach", "pausing"]),
      withOps(100, ["progress", 0.5]),
      ...at(200),
      withOps(300, ["animate", 0, { duration: 1, ease: "linear" }]),
      ...at(400, 900, 1500),
    ],
  },
}

const read = async (controls, flags, mv, timelines) => {
  await settle()
  const observed = controls.map((control, index) => ({
    time: encode(control.time),
    speed: control.speed,
    duration: encode(control.duration),
    iterationDuration: encode(control.iterationDuration),
    state: control.state,
    // 0 is Motion's absolute zero, which it sets where it seeks an animation that finished.
    startTime: control.startTime === null ? null : control.startTime === 0 ? "origin" : control.startTime - base,
    finished: flags[index],
  }))
  const out = { value: mv.get(), animating: mv.isAnimating(), controls: observed }
  if (timelines.length > 0) out.timelines = timelines.map((line) => ({ observed: line.observed, cleaned: line.cleaned }))
  return out
}

const frame = async () => {
  await settle()
  const due = frames
  frames = []
  for (const callback of due) callback(clock)
  await settle()
}

// The clock only moves forward across rows: Motion's frame loop keeps its own timestamps.
let base = 0

const run = async (row) => {
  base = clock + 10_000
  clock = base
  const mv = motionValue(row.from)
  const controls = []
  const flags = []
  const timelines = []
  const detachers = []
  const start = (to, transition) => {
    const control = animate(mv, to, options(transition))
    const index = controls.length
    controls.push(control)
    flags.push(false)
    control.finished.then(() => {
      flags[index] = true
    })
  }
  await settle()
  start(row.to, row.transition)
  const early = row.early
    ? { duration: encode(controls[0].duration), iterationDuration: encode(controls[0].iterationDuration) }
    : undefined
  const observations = []
  for (const step of row.steps) {
    clock = base + step.at
    await settle()
    for (const op of step.ops ?? []) {
      const control = controls.at(-1)
      switch (op[0]) {
        case "pause":
        case "play":
        case "stop":
        case "cancel":
        case "complete":
          control[op[0]]()
          break
        case "seek":
          control.time = op[1]
          break
        case "speed":
          control.speed = op[1]
          break
        case "animate":
          start(op[1], op[2])
          break
        case "controls":
          controls[op[1]][op[2]]()
          break
        case "valueStop":
          mv.stop()
          break
        case "valueJump":
          mv.jump(op[1])
          break
        case "attach": {
          const line = { observed: 0, cleaned: 0, animation: undefined }
          timelines.push(line)
          detachers.push(
            control.attachTimeline({
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
          const animation = timelines.at(-1).animation
          animation.time = animation.iterationDuration * op[1]
          break
        }
        case "detach":
          detachers.at(-1)()
          break
        default:
          throw new Error(`unknown op ${op[0]}`)
      }
    }
    await frame()
    observations.push({ at: step.at, ...(await read(controls, flags, mv, timelines)) })
  }
  // Let the frame loop wind down before the next row.
  for (const control of controls) control.stop()
  for (let round = 0; round < 5 && frames.length > 0; round++) await frame()
  return { early, observations }
}

const out = {}
for (const [name, row] of Object.entries(ROWS)) {
  const result = await run(row)
  out[name] = { from: row.from, to: row.to, transition: row.transition, steps: row.steps, early: result.early, observations: result.observations }
}
console.log(JSON.stringify(out, null, 1).replace(/\[[^\[\]{}]*\]/g, (list) => list.replace(/\s+/g, " ")))
