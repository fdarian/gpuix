// Samples Motion's own keyframes animation (motion-dom's JSAnimation, what `animate` on a motion value runs) at
// fixed times, for the repeat, repeatType, repeatDelay, times and ease-list cases of `motion-repeat.test.tsx`.
// Usage: MOTION_DOM=<dir of motion-dom/dist/es> bun motion-repeat-oracle.mjs > motion-repeat-oracle.json
import { pathToFileURL } from "node:url"
import path from "node:path"

const root = process.env.MOTION_DOM
if (!root) throw new Error("set MOTION_DOM to the motion-dom/dist/es directory")
const { JSAnimation } = await import(pathToFileURL(path.join(root, "index.mjs")).href)

const grid = (to, step) => Array.from({ length: Math.round(to / step) + 1 }, (_, index) => index * step)
const CASES = {
  "loop forever": { keyframes: [0, 100], transition: { duration: 1, ease: "linear", repeat: "Infinity" }, times: [0.25, 0.75, 1, 1.25, 2, 2.5, 3.75, 10.5, 1000.25] },
  "reverse forever with an ease": { keyframes: [0, 100], transition: { duration: 1, ease: "easeIn", repeat: "Infinity", repeatType: "reverse" }, times: [0.25, 0.75, 1, 1.25, 1.75, 2, 2.25, 3.5, 1000.25] },
  "mirror forever with an ease": { keyframes: [0, 100], transition: { duration: 1, ease: "easeIn", repeat: "Infinity", repeatType: "mirror" }, times: [0.25, 0.75, 1, 1.25, 1.75, 2, 2.25, 3.5, 1000.25] },
  "loop with a repeat delay": { keyframes: [0, 100], transition: { duration: 1, ease: "easeOut", repeat: 2, repeatDelay: 0.5, repeatType: "loop" }, times: grid(4.5, 0.25) },
  "reverse with a repeat delay": { keyframes: [0, 100], transition: { duration: 1, ease: "easeOut", repeat: 2, repeatDelay: 0.5, repeatType: "reverse" }, times: grid(4.5, 0.25) },
  "mirror with a repeat delay": { keyframes: [0, 100], transition: { duration: 1, ease: "easeOut", repeat: 2, repeatDelay: 0.5, repeatType: "mirror" }, times: grid(4.5, 0.25) },
  "loop through three keyframes": { keyframes: [0, 100, 50], transition: { duration: 2, times: [0, 0.2, 1], ease: ["easeIn", "easeOut"], repeat: 2, repeatType: "loop" }, times: grid(6.5, 0.25) },
  "reverse through three keyframes": { keyframes: [0, 100, 50], transition: { duration: 2, times: [0, 0.2, 1], ease: ["easeIn", "easeOut"], repeat: 2, repeatType: "reverse" }, times: grid(6.5, 0.25) },
  "mirror through three keyframes": { keyframes: [0, 100, 50], transition: { duration: 2, times: [0, 0.2, 1], ease: ["easeIn", "easeOut"], repeat: 3, repeatType: "mirror" }, times: grid(8.5, 0.25) },
  "a delay once before reverse": { keyframes: [0, 100], transition: { duration: 1, ease: "linear", delay: 0.5, repeat: 1, repeatDelay: 0.25, repeatType: "reverse" }, times: grid(3.5, 0.125) },
  "ends: reverse once": { keyframes: [0, 100], transition: { duration: 1, ease: "linear", repeat: 1, repeatType: "reverse" }, times: [1.5, 2, 2.5, 5] },
  "ends: reverse twice": { keyframes: [0, 100], transition: { duration: 1, ease: "linear", repeat: 2, repeatType: "reverse" }, times: [2.5, 3, 3.5, 5] },
  "ends: mirror once": { keyframes: [0, 100], transition: { duration: 1, ease: "linear", repeat: 1, repeatType: "mirror" }, times: [1.5, 2, 2.5, 5] },
  "ends: mirror three times": { keyframes: [0, 100, 50], transition: { duration: 1, ease: "linear", repeat: 3, repeatType: "mirror" }, times: [3.5, 4, 4.5, 8] },
}

const sample = (case_) => {
  const { duration, delay = 0, repeatDelay = 0, repeat = 0, ...rest } = case_.transition
  const animation = new JSAnimation({
    ...rest,
    keyframes: case_.keyframes,
    duration: duration * 1000,
    delay: delay * 1000,
    repeatDelay: repeatDelay * 1000,
    repeat: repeat === "Infinity" ? Infinity : repeat,
    autoplay: false,
  })
  const values = case_.times.map((time) => animation.sample(time * 1000).value)
  animation.stop()
  return values
}

const out = {}
for (const [name, case_] of Object.entries(CASES)) out[name] = { ...case_, values: sample(case_) }
console.log(JSON.stringify(out, null, 1).replace(/\[[^\[\]{}]*\]/g, (list) => list.replace(/\s+/g, " ")))
