// Runs Motion's own `animateTarget` (motion-dom 14.1.0: what a `motion` component calls for its `animate` prop, a variant or a
// gesture) on a stub visual element, on a clock the script steps, and samples the value of the one key each case animates.
// The cases are the ones that decide which transition Motion plays: none at all, only `delay` or a repeat field, `default`,
// per-key entries beside other keys, `type` without a duration, more than two keyframes, a `scale` that ends at 0, and the
// transition of a target (a variant or a gesture) against the component's.
// Usage: MOTION_DOM=<dir of motion-dom/dist/es> bun motion-default-oracle.mjs > motion-default-oracle.json
//
// A case is `key`, `from` and `to` (a number, or keyframes), the `transition` of the component, and `target` for the
// transition the animated target carries (Motion uses it in place of the component's). `values` are the key's value at
// `times` (seconds after the start); `duration` is the second the animation ended at (the first sample with the value at rest
// and no animation on it).
import { pathToFileURL } from "node:url"
import path from "node:path"

const root = process.env.MOTION_DOM
if (!root) throw new Error("set MOTION_DOM to the motion-dom/dist/es directory (14.1.0)")

let clock = 0
let frames = []
Object.defineProperty(globalThis, "performance", { value: { now: () => clock }, configurable: true, writable: true })
globalThis.requestAnimationFrame = (callback) => frames.push(callback)
globalThis.cancelAnimationFrame = () => {}
// The stub element is no HTMLElement, so Motion keeps the animations of opacity in JavaScript, where the script can step them.
globalThis.HTMLElement = class {}
globalThis.SVGElement = class {}
const { animateTarget, motionValue } = await import(pathToFileURL(path.join(root, "index.mjs")).href)

const settle = () => new Promise((done) => setTimeout(done, 0))
const frame = async () => {
  await settle()
  const due = frames
  frames = []
  for (const callback of due) callback(clock)
  await settle()
}

const revive = (value) => (value === "Infinity" ? Infinity : value)
const reviveTransition = (transition) => {
  if (!transition || typeof transition !== "object") return transition
  const out = {}
  for (const [key, value] of Object.entries(transition)) {
    out[key] = key === "repeat" ? revive(value) : value && typeof value === "object" && !Array.isArray(value) ? reviveTransition(value) : value
  }
  return out
}

const grid = (to, step) => Array.from({ length: Math.round(to / step) + 1 }, (_, index) => Number((index * step).toFixed(6)))
const SHORT = grid(1.2, 0.05)
const LONG = grid(2.4, 0.1)

const row = (key, from, to, transition, extra = {}) => ({ key, from, to, transition, times: SHORT, ...extra })

const CASES = {
  "x: no transition": row("x", 0, 100, undefined),
  "x: an empty transition": row("x", 0, 100, {}),
  "x: a large move": row("x", 0, 400, undefined),
  "y: no transition": row("y", 0, -80, undefined),
  "translateX: no transition": row("translateX", 10, 90, undefined),
  "scale: no transition": row("scale", 1, 2, undefined),
  "scale: to 0": row("scale", 1, 0, undefined),
  "scale: from 0": row("scale", 0, 1, undefined),
  "scaleX: no transition": row("scaleX", 1, 0.5, undefined),
  "scaleY: to 0": row("scaleY", 1, 0, undefined),
  "width: no transition": row("width", 0, 100, undefined),
  "height: no transition": row("height", 20, 120, undefined),
  "opacity: no transition": row("opacity", 0, 1, undefined),
  "opacity: down": row("opacity", 1, 0.2, undefined),
  "x: three keyframes": row("x", 0, [0, 100, 40], undefined, { times: LONG }),
  "x: three keyframes starting from null": row("x", 10, [null, 100, 40], undefined, { times: LONG }),
  "scale: three keyframes": row("scale", 1, [1, 2, 0], undefined, { times: LONG }),
  "width: three keyframes": row("width", 0, [0, 100, 50], undefined, { times: LONG }),
  "width: four keyframes": row("width", 0, [0, 100, 50, 80], undefined, { times: LONG }),
  "opacity: three keyframes": row("opacity", 0, [0, 1, 0.5], undefined, { times: LONG }),
  "x: two keyframes": row("x", 0, [20, 100], undefined),
  "width: two keyframes": row("width", 0, [20, 100], undefined),
  "x: delay only": row("x", 0, 100, { delay: 0.3 }),
  "width: delay only": row("width", 0, 100, { delay: 0.3 }),
  "opacity: delay only": row("opacity", 0, 1, { delay: 0.2 }),
  "scale: delay only": row("scale", 1, 2, { delay: 0.2 }),
  "x: three keyframes with a delay": row("x", 0, [0, 100, 40], { delay: 0.2 }, { times: LONG }),
  "width: repeat only": row("width", 0, 100, { repeat: 2 }, { times: LONG }),
  "width: repeat reverse": row("width", 0, 100, { repeat: 2, repeatType: "reverse" }, { times: LONG }),
  "width: repeat mirror with a repeat delay": row("width", 0, 100, { repeat: 1, repeatType: "mirror", repeatDelay: 0.2 }, { times: LONG }),
  "x: repeat only": row("x", 0, 100, { repeat: 2 }, { times: LONG }),
  "x: delay and repeat": row("x", 0, 100, { delay: 0.2, repeat: 1, repeatType: "reverse" }, { times: LONG }),
  "x: duration only": row("x", 0, 100, { duration: 0.5 }),
  "scale: duration only": row("scale", 1, 2, { duration: 0.5 }),
  "width: duration only": row("width", 0, 100, { duration: 0.5 }),
  "x: ease only": row("x", 0, 100, { ease: "linear" }),
  "x: type tween": row("x", 0, 100, { type: "tween" }),
  "width: type tween": row("width", 0, 100, { type: "tween" }),
  "width: type tween with a delay": row("width", 0, 100, { type: "tween", delay: 0.2 }),
  "x: type spring": row("x", 0, 100, { type: "spring" }, { times: LONG }),
  "width: type spring": row("width", 0, 100, { type: "spring" }, { times: LONG }),
  "opacity: type spring": row("opacity", 0, 1, { type: "spring" }, { times: LONG }),
  "x: type spring with a delay": row("x", 0, 100, { type: "spring", delay: 0.2 }, { times: LONG }),
  "x: type spring with a bounce": row("x", 0, 100, { type: "spring", bounce: 0.5 }, { times: LONG }),
  "x: type spring with a duration": row("x", 0, 100, { type: "spring", duration: 0.5 }, { times: LONG }),
  "x: type spring stiffness only": row("x", 0, 100, { type: "spring", stiffness: 300 }, { times: LONG }),
  "x: type spring damping only": row("x", 0, 100, { type: "spring", damping: 40 }, { times: LONG }),
  "scale: type spring": row("scale", 1, 2, { type: "spring" }, { times: LONG }),
  "x: default only a delay": row("x", 0, 100, { default: { delay: 0.2 } }),
  "width: default only a delay": row("width", 0, 100, { default: { delay: 0.2 } }),
  "x: default with a duration": row("x", 0, 100, { default: { duration: 0.5 } }),
  "width: default with a duration": row("width", 0, 100, { default: { duration: 0.5 } }),
  "x: default and a delay at the top": row("x", 0, 100, { delay: 0.4, default: { duration: 0.5 } }),
  "width: default and a delay at the top": row("width", 0, 100, { delay: 0.4, default: { duration: 0.5 } }),
  "x: default with a delay and a delay at the top": row("x", 0, 100, { delay: 0.4, default: { delay: 0.1 } }),
  "x: default empty": row("x", 0, 100, { default: {} }),
  "width: default empty": row("width", 0, 100, { default: {} }),
  "x: default spring": row("x", 0, 100, { default: { type: "spring" } }, { times: LONG }),
  "x: its own entry with a duration": row("x", 0, 100, { x: { duration: 0.5 } }),
  "x: its own entry with only a delay": row("x", 0, 100, { x: { delay: 0.2 } }),
  "width: an entry of x beside it": row("width", 0, 100, { x: { delay: 0.2 } }),
  "width: an entry of x with a delay at the top": row("width", 0, 100, { delay: 0.2, x: { duration: 1 } }),
  "x: an entry of width beside it": row("x", 0, 100, { width: { duration: 0.5 } }),
  "scale: an entry of x beside it": row("scale", 1, 2, { x: { duration: 0.5 } }),
  "opacity: an entry of x beside it": row("opacity", 0, 1, { x: { duration: 0.5 } }),
  "x: an entry of x and a default": row("x", 0, 100, { default: { duration: 1 }, x: { delay: 0.2 } }),
  "width: an entry of x and a default": row("width", 0, 100, { default: { duration: 1 }, x: { delay: 0.2 } }),
  "x: its own entry is an empty object": row("x", 0, 100, { x: {} }),
  "width: an entry of x that is empty": row("width", 0, 100, { x: {} }),
  "x: its own entry with a repeat": row("x", 0, 100, { x: { repeat: 1, repeatType: "reverse" } }, { times: LONG }),
  "x: top level type with a delay": row("x", 0, 100, { type: "spring", stiffness: 200, damping: 20, delay: 0.1 }, { times: LONG }),
  "x: a variant transition with only a delay": row("x", 0, 100, undefined, { target: { delay: 0.3 } }),
  "width: a variant transition with only a delay": row("width", 0, 100, undefined, { target: { delay: 0.3 } }),
  "x: a variant transition replaces the component's": row("x", 0, 100, { duration: 1, ease: "linear" }, { target: { delay: 0.3 } }),
  "width: a variant transition replaces the component's": row("width", 0, 100, { type: "spring" }, { target: { delay: 0.3 }, times: LONG }),
  "x: a variant transition with a duration": row("x", 0, 100, { delay: 0.2 }, { target: { duration: 0.4 } }),
  "x: a variant transition with an entry of another key": row("x", 0, 100, { type: "spring" }, { target: { width: { duration: 1 } } }),
  "opacity: a variant transition with a default": row("opacity", 0, 1, { duration: 1 }, { target: { default: { delay: 0.3 } } }),
  "x: a variant transition that is empty": row("x", 0, 100, { duration: 1 }, { target: {} }),
}

const sampleCase = async (case_) => {
  const base = clock + 100_000
  clock = base
  const values = new Map()
  const props = { transition: reviveTransition(case_.transition) }
  const element = {
    props,
    latestValues: {},
    shouldReduceMotion: false,
    shouldSkipAnimations: false,
    getDefaultTransition: () => props.transition,
    getValue: (key, initial) => {
      if (key === "willChange") return undefined
      if (!values.has(key)) values.set(key, motionValue(initial ?? null))
      return values.get(key)
    },
  }
  const value = element.getValue(case_.key, case_.from)
  value.set(case_.from)
  const target = { [case_.key]: case_.to }
  if (case_.target !== undefined) target.transition = reviveTransition(case_.target)
  await settle()
  const animations = animateTarget(element, target)
  let finished = false
  Promise.all(animations).then(() => {
    finished = true
  })
  const out = []
  let end = null
  let previous = 0
  for (const time of case_.times) {
    for (let at = previous; at < time; at += 0.016) {
      clock = base + at * 1000
      await frame()
    }
    clock = base + time * 1000
    await frame()
    previous = time
    out.push(value.get())
    if (end === null && finished) end = time
  }
  value.stop()
  return { values: out, end }
}

const out = {}
for (const [name, case_] of Object.entries(CASES)) {
  const { values, end } = await sampleCase(case_)
  out[name] = { ...case_, values, end }
}
console.log(JSON.stringify(out, null, 1).replace(/\[[^\[\]{}]*\]/g, (list) => list.replace(/\s+/g, " ")))
