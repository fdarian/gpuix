/// `element.getAnimations()` for the animations of `motion` components, and the commands on them, against Motion 14.1.0 in
/// headless Chrome. Motion hands some of the values it animates to the browser's Web Animations (an `opacity` or
/// `backgroundColor`, a colour in `oklch()`) and runs the rest in JavaScript, where nothing lists them.
/// `fixtures/motion-waapi-oracle.json` is what Chrome lists after every step of 278 cases (regenerate it with
/// `fixtures/motion-waapi-oracle.py`, which says how): ~140 are `animate(el, keyframes, options)`, ~45 a `motion.div`,
/// ~95 command the `Animation` that `getAnimations()` listed (`pause()`, `currentTime = `, `finish()`, `cancel()`,
/// `updateTiming()`, ...), and 25 use the controls Motion's `animate()` returns. Each is replayed here on a paused clock
/// as the `motion` component that makes the same animation: `animate(el, kf, opts)` is a `motion.div` that mounts with
/// `animate={kf}` `transition={opts}` over the element's own style, and a second call is a render with the new props.
///
/// What is compared at every sample: how many animations are listed, and for each its class, `id`, play state, rate,
/// that its start time and current time agree, a current time within the lag of Chrome's frames, the timing
/// (`getTiming()`) and the keyframes (`getKeyframes()`); for the commanded cases also what the element shows (its pixel,
/// against Chrome's computed `opacity` and `backgroundColor`, within what the value did over the lag of Chrome's frames),
/// how often Motion told its caller the animation is complete (`onMotionComplete` here), and the events and promises
/// of the animation. The cases a gpuix component cannot be made to run are listed by name with the reason in
/// `NOT_EXPRESSIBLE` and are not replayed. Of the controls cases (`ctl-*`) the ones whose control Motion passes through to
/// the animation (`pause`, `play`, `time`, `speed`, `complete`, `cancel`) are replayed as that command on the
/// `Animation`.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { createElement, type ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { document, motion, type PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface Keyframe {
  offset: number | null
  easing: string
  props: Record<string, string>
}
interface Listed {
  ctor: string
  id: string
  playState: string
  pending: boolean
  currentTime: number | null
  playbackRate: number
  startTimeRelative: number | null
  timing: Record<string, unknown>
  keyframes: Keyframe[]
}
interface Noted {
  t: number
  what: string
}
interface Sample {
  label: string
  t: number
  calls: number
  n: number
  anims: Listed[]
  computed: Record<string, string>
  completions: Noted[]
  events: Noted[]
}
type Call =
  | { kind: "animate"; t: number; keyframes: Record<string, unknown>; options: Record<string, unknown> | null }
  | { kind: "render"; t: number; props: Record<string, unknown> }
  | { kind: "unmount"; t: number }
  | { kind: "track"; t: number; name: string; index: number }
  | { kind: "command"; t: number; target: string; op: string; arg: unknown }
  | { kind: "control"; t: number; op: string; arg: unknown }
interface Case {
  id: string
  group: string
  kind: "vanilla" | "react"
  tag: string | null
  style: string
  calls: Call[]
  samples: Sample[]
}

const ORACLE: { cases: Case[] } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/motion-waapi-oracle.json"),
    "utf8"
  )
)

const FUNCTION_EASE = "Motion's JS easings (circOut, backOut, anticipate, circInOut) and functions are not gpuix eases"
const NO_TRANSFORM_KEYS = "a gpuix motion has no transform string, filter or clipPath key"
const SPRING_FIELDS = "gpuix refuses the spring fields (stiffness, damping) on a tween: they need type spring"
const DAMPING_ZERO = "gpuix refuses a spring with damping 0"
const DEFAULT_CURVE =
  "no transition at all: Motion gives the browser cubic-bezier(0.25, 0.1, 0.35, 1) where gpuix runs its own default, ease-out"
const STOP =
  "stop() commits a value that Motion samples from a JavaScript replica of the animation at wall-clock time, then cancels the Animation: a motion component has no such replica to sample"
const REVIVE =
  "play() or the time setter on the Animation Motion cancelled restarts it in Chrome, where gpuix's is gone with its track, as a cancelled CSS animation is"
const BEFORE_FIRST_FRAME =
  "the control acts on the Animation Motion makes before any frame, which gpuix lists from the first frame that is drawn"
const ZERO_DURATION = "a duration of 0: Motion lists it for one frame, and not reliably"
const NO_PROP = "gpuix motion components have no such prop, so the condition does not arise"

/** The cases not replayed, and why. */
const NOT_EXPRESSIBLE = new Map<string, string>([
  ["opacity-default", DEFAULT_CURVE],
  ["r-opacity-default-transition", DEFAULT_CURVE],
  ["opacity-tween-circOut", FUNCTION_EASE],
  ["opacity-tween-backOut", FUNCTION_EASE],
  ["opacity-tween-anticipate", FUNCTION_EASE],
  ["opacity-tween-fn", FUNCTION_EASE],
  ["ease-circInOut", FUNCTION_EASE],
  ["ease-steps", "Motion reads steps(4) as no easing at all (linear) where gpuix runs the CSS steps()"],
  ["opacity-tween-zero-duration", ZERO_DURATION],
  ["creation-zero-duration", ZERO_DURATION],
  ["opacity-spring-stiff-only", SPRING_FIELDS],
  ["opacity-spring-damp-only", SPRING_FIELDS],
  ["r-opacity-stiffness-only", SPRING_FIELDS],
  ["damping-set-tween", SPRING_FIELDS],
  ["damping0-tween", SPRING_FIELDS],
  ["opacity-spring-visualDuration", "gpuix springs have no visualDuration"],
  ["opacity-spring-damping0", DAMPING_ZERO],
  ["opacity-spring-damping0-notype", DAMPING_ZERO],
  ["r-damping0", DAMPING_ZERO],
  ["type-spring-damping0-oklch", DAMPING_ZERO],
  ["opacity-spring-keyframes3", "gpuix refuses a spring over more than two keyframes"],
  ["opacity-inertia", "gpuix has no inertia"],
  ["delay-negative", "gpuix refuses a negative delay"],
  ["autoplay-false", "gpuix has no autoplay option"],
  ["css-var", "a gpuix motion has no custom property key"],
  ["transform-str", NO_TRANSFORM_KEYS],
  ["transform-str-from-none", NO_TRANSFORM_KEYS],
  ["transform-str-keyframes", NO_TRANSFORM_KEYS],
  ["transform-str-spring", NO_TRANSFORM_KEYS],
  ["transform-rotate-str", NO_TRANSFORM_KEYS],
  ["filter", NO_TRANSFORM_KEYS],
  ["filter-from-none", NO_TRANSFORM_KEYS],
  ["filter-brightness", NO_TRANSFORM_KEYS],
  ["clipPath", NO_TRANSFORM_KEYS],
  ["multi-opacity-filter-clip", NO_TRANSFORM_KEYS],
  ["svg-transform-str", NO_TRANSFORM_KEYS],
  ["r-filter", NO_TRANSFORM_KEYS],
  ["r-transform-str", NO_TRANSFORM_KEYS],
  ["r-transformTemplate-transform-str", NO_TRANSFORM_KEYS],
  ["r-onUpdate", NO_PROP],
  ["r-transformTemplate-opacity", NO_PROP],
  ["type-keyframes", "gpuix has no transition type keyframes"],
  ["borderColor-oklch", "gpuix animates borderColor as its four sides, which are not one animation here yet"],
])

/** The cases replayed except for the text of some value in their keyframes, and why. */
const VALUES_DIFFER = new Map<string, string>([
  ["color-mix", "gpuix has the colour a color-mix() computes to (color(srgb 0.5 0 0.5)) where Chrome keeps the function"],
  ["svg-fill-oklch", "Motion reads the start of an SVG fill as black (oklch(0 0 0)); gpuix starts from the fill the shape has"],
])
for (const [id, reason] of [
  ["ctl-stop", STOP],
  ["ctl-stop-in-delay", STOP],
  ["ctl-spring-stop", STOP],
  ["ctl-bg-stop", STOP],
  ["ctl-stop-then-play", STOP],
  ["ctl-time-set-after-finish", REVIVE],
  ["ctl-play-after-finish", REVIVE],
  ["wa-cancel-play", REVIVE],
  ["ctl-pause-before-resolve", BEFORE_FIRST_FRAME],
]) {
  NOT_EXPRESSIBLE.set(id, reason)
}

const BOX = { width: 100, height: 50, color: "rgb(10, 20, 30)", backgroundColor: "rgb(200, 200, 200)" }

/** `"background-color: #ff0000; opacity: 1"` as a style. */
function styleOf(text: string): Record<string, unknown> {
  const style: Record<string, unknown> = {}
  for (const declaration of text.split(";")) {
    const colon = declaration.indexOf(":")
    if (colon < 0) continue
    const name = declaration.slice(0, colon).trim().replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())
    style[name] = declaration.slice(colon + 1).trim()
  }
  return style
}

/** The options of Motion's `animate()` as a gpuix `transition`: `repeat: "Infinity"` is `Infinity`, `autoplay` is Motion's. */
function transitionOf(options: Record<string, unknown> | null): Record<string, unknown> | undefined {
  if (options === null) return undefined
  const transition: Record<string, unknown> = {}
  for (const key of Object.keys(options)) {
    if (key === "autoplay") continue
    transition[key] = options[key] === "Infinity" ? Infinity : options[key]
  }
  return transition
}

/** The props of a `motion.div` the oracle rendered, with its numbers and Infinity as gpuix takes them. */
function reviveProps(props: Record<string, unknown>): Record<string, unknown> {
  const revived: Record<string, unknown> = { ...props }
  if (props.transition) revived.transition = transitionOf(props.transition as Record<string, unknown>)
  return revived
}

/** A frame of the window at the clock as it is, after the clock moved by `ms`: the only way time passes. */
const frame = (renderer: ReturnType<typeof createTestRoot>["renderer"], ms: number) => {
  renderer.clockFastForward(ms)
  renderer.drawFrame()
  renderer.dispatchNativeEvents()
}

/** Chrome's frames run 16.7 ms apart, and its animations start at one of them: its current times lag the clock by up to that and the jitter of a loaded page. */
const LAG = 60

/** The easing of a `linear()` as pairs of its output and input: Chrome writes the inputs it works out with noise past the thirteenth digit. */
function stops(easing: string): [number, number][] | null {
  if (!easing.startsWith("linear(")) return null
  return [...easing.matchAll(/(-?[\d.e-]+) (-?[\d.e-]+)%/g)].map((stop) => [Number(stop[1]), Number(stop[2])])
}

function expectSameEasing(got: unknown, want: unknown, where: string): void {
  const gotStops = typeof got === "string" ? stops(got) : null
  const wantStops = typeof want === "string" ? stops(want) : null
  if (gotStops && wantStops) {
    expect(gotStops.length, `${where}: points of the linear()`).toBe(wantStops.length)
    gotStops.forEach((stop, index) => {
      expect(stop[0], `${where}: output ${index}`).toBe(wantStops[index][0])
      expect(stop[1], `${where}: input ${index}`).toBeCloseTo(wantStops[index][1], 9)
    })
  } else {
    expect(got, where).toEqual(want)
  }
}

function describeKeyframes(animation: Animation): Keyframe[] {
  return animation.effect!.getKeyframes().map((frame) => {
    const props: Record<string, string> = {}
    for (const key of Object.keys(frame)) {
      if (key !== "offset" && key !== "computedOffset" && key !== "easing" && key !== "composite") {
        props[key] = String(frame[key as keyof typeof frame])
      }
    }
    return { offset: frame.offset, easing: frame.easing, props }
  })
}

/**
 * Whether Chrome's animation is about to end at the sample: it is within a frame of its end, which gpuix may have passed. Its
 * frames lag by up to `LAG` and a case samples a little after the end it was waiting for.
 */
function endsSoon(animation: Listed): boolean {
  const timing = animation.timing as { delay: number; endDelay: number; duration: number; iterations: number | string }
  if (animation.currentTime === null || animation.playbackRate === 0 || timing.iterations === "Infinity") return false
  const end = timing.delay + timing.duration * Number(timing.iterations) + timing.endDelay
  const remaining = animation.playbackRate > 0 ? (end - animation.currentTime) / animation.playbackRate : animation.currentTime / -animation.playbackRate
  return Math.abs(remaining) < 30
}

/** The groups whose cases command the animations: what the element shows is compared, and Motion's completion. */
const COMMANDED = new Set(["waapi", "controls"])

/** What a `motion` component runs for a call of the oracle's page. `box` is the style of the element. */
function elementFor(
  scenario: Case,
  call: Call,
  ref: (node: PublicInstance | null) => void,
  box: Record<string, unknown>,
  onComplete: () => void
): ReactElement {
  const inline = styleOf(scenario.style)
  if (scenario.tag === "svg-rect") {
    // An SVG shape animates attributes: its own style is the attributes the oracle's page set.
    const attributes: Record<string, unknown> = { width: 50, height: 50, fill: "red" }
    for (const key of Object.keys(inline)) attributes[key] = inline[key]
    const animate = call.kind === "animate" ? call.keyframes : {}
    return createElement(
      "svg",
      { width: 100, height: 100 },
      createElement(motion.rect, {
        ref,
        testId: "box",
        ...attributes,
        animate,
        transition: call.kind === "animate" ? transitionOf(call.options) : undefined,
        onMotionComplete: onComplete,
      } as never)
    )
  }
  if (call.kind === "animate") {
    return createElement(motion.div, {
      ref,
      testId: "box",
      style: { ...box, ...inline },
      animate: call.keyframes,
      transition: transitionOf(call.options),
      onMotionComplete: onComplete,
    } as never)
  }
  if (call.kind === "render") {
    const props = reviveProps(call.props)
    // The page counts `onAnimationComplete`, which is the completion of the animation.
    delete props.onAnimationComplete
    return createElement(motion.div, { ref, testId: "box", ...props, style: box, onMotionComplete: onComplete } as never)
  }
  throw new Error(`no element for a ${call.kind}`)
}

type Pixel = [number, number, number]

let shots = 0

/** The colour at the middle of the element on the screen. */
function pixelOf(renderer: ReturnType<typeof createTestRoot>["renderer"], element: PublicInstance): Pixel {
  const file = `${SHOTS_DIR}/motion-waapi-${process.pid}-${shots++}.png`
  // What a command changed shows in the frame that is drawn after it.
  renderer.drawFrame()
  renderer.captureScreenshot(file)
  const pixels = readLogicalPixels(file, renderer)
  fs.unlinkSync(file)
  const bounds = renderer.getElementBounds(element.id)!
  return pixels.at(Math.round(bounds.x + bounds.width / 2), Math.round(bounds.y + bounds.height / 2)).slice(0, 3) as Pixel
}

/** What Chrome's computed `opacity` and `backgroundColor` make of a black box, or of the colour it has, over white. */
function shownBy(computed: Record<string, string>): Pixel {
  const alpha = computed.opacity === undefined ? 1 : Number(computed.opacity)
  const parsed = computed.backgroundColor === undefined ? null : /rgba?\(([^)]*)\)/.exec(computed.backgroundColor)
  const color = parsed ? parsed[1].split(",").map(Number) : [0, 0, 0]
  return [0, 1, 2].map((channel) => alpha * color[channel] + (1 - alpha) * 255) as Pixel
}

/** Within the colours the element showed over the lag of Chrome's frames, give or take the rounding of a pixel. */
const PIXEL_SLACK = 6

/** The calls that put the picture somewhere else at once, where the ones that start, stop or slow it do not. */
const JUMPS = new Set(["currentTime", "startTime", "finish", "cancel", "updateTiming", "complete", "time"])

/**
 * How fast the picture of the case changes in Chrome, in colour steps a millisecond, between samples that no jump of it
 * separates. A pause that Chrome holds a frame after gpuix does differs by that much of it.
 */
function fastest(scenario: Case): number {
  let rate = 0
  scenario.samples.forEach((sample, index) => {
    const before = scenario.samples[index - 1]
    if (!before || sample.t - before.t < 10) return
    const between = scenario.calls.slice(before.calls, sample.calls)
    if (between.some((call) => !(call.kind === "track" || (call.kind === "command" && !JUMPS.has(call.op)) || (call.kind === "control" && !JUMPS.has(call.op))))) return
    const [now, then] = [shownBy(sample.computed), shownBy(before.computed)]
    for (let channel = 0; channel < 3; channel++) rate = Math.max(rate, Math.abs(now[channel] - then[channel]) / (sample.t - before.t))
  })
  return rate
}

/** A frame of Chrome's. */
const FRAME = 17

/**
 * A `backgroundColor` between its two ends: gpuix blends the colours of a motion in Lab where Motion hands them to the
 * browser, which blends in sRGB, so the colour between the ends is not the same.
 */
function isBlended(computed: Record<string, string>): boolean {
  return computed.backgroundColor !== undefined && !/^rgb\((255, 0, 0|0, 0, 255)\)$/.test(computed.backgroundColor)
}

/** The names of what happened to the animations the case holds, in the order they did. */
type Log = Noted[]

/** The entries of `want` that are certainly in by `at`, and those that may be, given the lag of Chrome's frames. */
function certainly(want: Noted[], at: number): string[] {
  return want.filter((entry) => entry.t < at - LAG).map((entry) => entry.what)
}
function possibly(want: Noted[], at: number): string[] {
  return want.filter((entry) => entry.t >= at - LAG && entry.t <= at + LAG).map((entry) => entry.what)
}

/** `inside` holds all of `needed`, and nothing but those and `allowed`. */
function expectBetween(inside: string[], needed: string[], allowed: string[], where: string): void {
  const rest = [...inside]
  for (const name of needed) {
    const at = rest.indexOf(name)
    expect(at, `${where}: ${name} is missing from [${inside.join(" | ")}]`).toBeGreaterThanOrEqual(0)
    rest.splice(at, 1)
  }
  for (const name of allowed) {
    const at = rest.indexOf(name)
    if (at >= 0) rest.splice(at, 1)
  }
  expect(rest, `${where}: more happened than Chrome's [${needed.concat(allowed).join(" | ")}]`).toEqual([])
}

async function replay(scenario: Case): Promise<void> {
  const root = createTestRoot()
  root.renderer.clockPause()
  // Time passes in frames only, as in a browser, and a read forces a layout and no more.
  root.renderer.setInlineDraws(false)
  const holder: { current: PublicInstance | null } = { current: null }
  const ref = (node: PublicInstance | null) => {
    if (node !== null) holder.current = node
  }
  const measured = COMMANDED.has(scenario.group)
  const box = measured ? { ...BOX, backgroundColor: "rgb(0, 0, 0)" } : BOX
  const completions: Noted[] = []
  const events: Log = []
  const tracked: Record<string, Animation> = {}
  let now = 0
  /** The time of what is dispatched while a frame is drawn. */
  let stamp = 0
  const advanceTo = (t: number) => {
    if (t <= now) return
    stamp = t
    frame(root.renderer, t - now)
    now = t
  }
  const noted = (what: string) => events.push({ t: stamp, what })
  const onComplete = () => completions.push({ t: stamp, what: "complete" })
  let applied = 0
  /** Calls were made and no frame has been drawn since. */
  let undrawn = false
  /** When the last call was made. */
  let lastCall = 0
  const slack = PIXEL_SLACK + fastest(scenario) * FRAME
  const wrap = (element: ReactElement): ReactElement =>
    measured ? createElement("div", { style: { width: 300, height: 200, backgroundColor: "#ffffff" } }, element) : element
  const watch = (name: string, animation: Animation) => {
    animation.addEventListener("finish", () => noted(`${name}:finish`))
    animation.addEventListener("cancel", () => noted(`${name}:cancel`))
    animation.finished.then(
      () => noted(`${name}:finished`),
      (error: Error) => noted(`${name}:finished rejected ${error.name}`)
    )
  }
  const command = (name: string, animation: Animation, op: string, arg: unknown) => {
    try {
      switch (op) {
        case "pause":
        case "play":
        case "finish":
        case "cancel":
        case "reverse":
          animation[op]()
          break
        case "updatePlaybackRate":
          animation.updatePlaybackRate(arg as number)
          break
        case "updateTiming":
          animation.effect!.updateTiming(arg as OptionalEffectTiming)
          break
        case "currentTime":
        case "playbackRate":
          animation[op] = arg as number
          break
        case "startTime":
          animation.startTime = arg === null ? null : animation.timeline!.currentTime! + (arg as { relative: number }).relative
          break
        default:
          throw new Error(`no command ${op}`)
      }
    } catch (error) {
      noted(`${name}:${op} threw ${(error as Error).name}`)
    }
    if (op === "pause" || op === "play" || op === "reverse" || op === "updatePlaybackRate") {
      animation.ready.then(
        () => noted(`${name}:ready after ${op}`),
        () => noted(`${name}:ready rejected after ${op}`)
      )
    }
  }
  /** The command of Motion's control, which Motion passes on to its animations. */
  const control = (op: string, arg: unknown) => {
    for (const animation of holder.current!.getAnimations()) {
      switch (op) {
        case "pause":
        case "play":
          animation[op]()
          break
        case "complete":
          animation.finish()
          break
        case "cancel":
          animation.cancel()
          break
        case "time":
          animation.currentTime = (arg as number) * 1000
          break
        case "speed":
          animation.playbackRate = arg as number
          break
        default:
          throw new Error(`the control ${op} is not a command of the animation`)
      }
    }
  }
  try {
    const apply = (upTo: number) => {
      while (applied < upTo) {
        const call = scenario.calls[applied]
        applied++
        lastCall = now
        undrawn = true
        switch (call.kind) {
          case "track":
            tracked[call.name] = holder.current!.getAnimations()[call.index]
            expect(tracked[call.name], `${scenario.id}: the animation to track`).toBeDefined()
            watch(call.name, tracked[call.name])
            break
          case "command":
            command(call.target, tracked[call.target], call.op, call.arg)
            break
          case "control":
            control(call.op, call.arg)
            break
          default:
            // A later call is a render with the new props over the same element; a call to unmount is the element going.
            root.render(call.kind === "unmount" ? createElement("div") : wrap(elementFor(scenario, call, ref, box, onComplete)))
            // Chrome styles the element when it is asked for its animations; a read of its box is how gpuix is made to.
            if (call.kind !== "unmount" && holder.current) root.renderer.getElementBounds(holder.current.id)
        }
      }
    }
    for (const sample of scenario.samples) {
      const where = `${scenario.id} at ${sample.label} (${sample.t} ms)`
      const synchronous = /^(sync|microtask|timeout0)/.test(sample.label)
      if (applied < sample.calls) {
        // The frame after the calls so far is drawn before time moves on to the next.
        if (undrawn && applied > 0) {
          frame(root.renderer, 0)
          undrawn = false
        }
        // A later call happens at the time of the sample or just before it.
        if (applied > 0) advanceTo(sample.t)
        apply(sample.calls)
      }
      let before: Pixel | null = null
      if (!synchronous) {
        if (undrawn) frame(root.renderer, 0)
        undrawn = false
        const from = Math.max(sample.t - LAG, lastCall)
        if (measured && holder.current && from >= now && from < sample.t) {
          advanceTo(from)
          before = pixelOf(root.renderer, holder.current)
        }
        advanceTo(sample.t)
      }
      if (sample.label === "final") continue
      // The promises of the animations settle in the microtasks that follow what was done.
      await new Promise((resolve) => setImmediate(resolve))
      const element = holder.current
      expect(element, `${where}: element`).not.toBeNull()
      const got = element!.getAnimations()
      const expected = got.length === sample.anims.length ? sample.anims : sample.anims.filter((animation) => !endsSoon(animation))
      expect(
        got.map((animation) => animation.constructor.name),
        `${where}: listed (${got.length}, Chrome lists ${sample.n})`
      ).toEqual(expected.map((animation) => animation.ctor))
      expected.forEach((want, index) => {
        const animation = got[index]
        const here = `${where}, animation ${index}`
        expect(animation.id, `${here}: id`).toBe(want.id)
        expect(animation.playState, `${here}: playState`).toBe(want.playState)
        expect(animation.pending, `${here}: pending`).toBe(want.pending)
        expect(animation.playbackRate, `${here}: playbackRate`).toBe(want.playbackRate)
        if (want.currentTime !== null) {
          // The oracle's start is a frame after the call, and ours is the frame that drew it.
          expect(Math.abs(animation.currentTime! - want.currentTime), `${here}: currentTime ${animation.currentTime} against ${want.currentTime}`).toBeLessThanOrEqual(LAG)
        }
        if (want.startTimeRelative === null) {
          expect(animation.startTime, `${here}: startTime`).toBeNull()
        } else if (animation.playbackRate !== 0) {
          expect(
            (animation.timeline!.currentTime! - animation.startTime!) * animation.playbackRate,
            `${here}: startTime agrees with currentTime`
          ).toBeCloseTo(animation.currentTime!, 3)
        }
        const timing = animation.effect!.getTiming()
        for (const key of Object.keys(want.timing)) {
          const value = timing[key as keyof typeof timing]
          expectSameEasing(value === Infinity ? "Infinity" : value, want.timing[key], `${here}: timing.${key}`)
        }
        const keyframes = describeKeyframes(animation)
        expect(keyframes.length, `${here}: keyframes`).toBe(want.keyframes.length)
        // The values of an animation that took over from another start where that one was.
        const renders = scenario.calls.slice(0, sample.calls).filter((call) => call.kind === "animate" || call.kind === "render").length
        const exact = renders === 1 && !VALUES_DIFFER.has(scenario.id)
        keyframes.forEach((keyframe, position) => {
          const wantFrame = want.keyframes[position]
          expect(keyframe.offset, `${here}: keyframe ${position} offset`).toBe(wantFrame.offset)
          expect(keyframe.easing, `${here}: keyframe ${position} easing`).toBe(wantFrame.easing)
          expect(Object.keys(keyframe.props), `${here}: keyframe ${position} properties`).toEqual(Object.keys(wantFrame.props))
          if (exact) expect(keyframe.props, `${here}: keyframe ${position} values`).toEqual(wantFrame.props)
        })
      })
      if (!measured) continue
      // Chrome's frame is up to the lag behind, and the colours it computed are what the element showed then. An element
      // that is gone computes to nothing.
      const gone = Object.values(sample.computed).some((value) => value === "") || isBlended(sample.computed)
      const after = gone ? null : pixelOf(root.renderer, element!)
      const wanted = after ? shownBy(sample.computed) : []
      wanted.forEach((channel, index) => {
        const shown = [after![index], ...(before ? [before[index]] : [])]
        expect(
          channel,
          `${where}: pixel channel ${index} is ${shown.join(" or ")}, Chrome computed ${JSON.stringify(sample.computed)}`
        ).toBeGreaterThanOrEqual(Math.min(...shown) - slack)
        expect(channel, `${where}: pixel channel ${index}`).toBeLessThanOrEqual(Math.max(...shown) + slack)
      })
      // What happens just after a sample that was taken inside a call is in the samples after it.
      const everything = scenario.samples[scenario.samples.length - 1]
      expectBetween(
        completions.map((entry) => entry.what),
        certainly(everything.completions, sample.t).map(() => "complete"),
        possibly(everything.completions, sample.t).map(() => "complete"),
        `${where}: Motion's completion`
      )
      if (Object.keys(tracked).length > 0) {
        expectBetween(
          events.map((entry) => entry.what),
          certainly(everything.events, sample.t),
          possibly(everything.events, sample.t),
          `${where}: events and promises`
        )
      }
    }
  } finally {
    root.unmount()
  }
}

describeNative("Motion's animations in getAnimations() against Chrome", () => {
  it("is Chrome's list of cases, with every one replayed or named below", () => {
    expect(ORACLE.cases.length).toBe(278)
    for (const name of NOT_EXPRESSIBLE.keys()) {
      expect(
        ORACLE.cases.some((scenario) => scenario.id === name),
        name
      ).toBe(true)
    }
  })

  for (const scenario of ORACLE.cases) {
    if (NOT_EXPRESSIBLE.has(scenario.id)) continue
    it(scenario.id, { timeout: 60_000 }, async () => {
      await replay(scenario)
    })
  }

  it("lists the same animations from the document as from the element", () => {
    const root = createTestRoot()
    root.renderer.clockPause()
    root.renderer.setInlineDraws(false)
    const holder: { current: PublicInstance | null } = { current: null }
    root.render(
      createElement(motion.div, {
        ref: (node: PublicInstance | null) => {
          holder.current = node
        },
        testId: "box",
        style: BOX,
        animate: { opacity: 0.5, backgroundColor: "#0000ff", width: 200 },
        transition: { duration: 1, ease: "linear" },
      } as never)
    )
    expect(holder.current!.getAnimations()).toEqual([])
    frame(root.renderer, 0)
    frame(root.renderer, 250)
    const own = holder.current!.getAnimations()
    expect(own.map((animation) => animation.effect!.getKeyframes()[0].computedOffset)).toEqual([0, 0])
    expect(own.length).toBe(2)
    expect(holder.current!.getAnimations({ subtree: true })).toEqual(own)
    expect(document.getAnimations()).toEqual(own)
    root.unmount()
  })
})
