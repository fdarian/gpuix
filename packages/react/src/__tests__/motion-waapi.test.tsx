/// `element.getAnimations()` for the animations of `motion` components, against Motion 14.1.0 in headless Chrome.
/// Motion hands some of the values it animates to the browser's Web Animations (an `opacity` or `backgroundColor`,
/// a colour in `oklch()`) and runs the rest in JavaScript, where nothing lists them. `fixtures/motion-waapi-oracle.json`
/// is what Chrome lists after every step of 185 cases (regenerate it with `fixtures/motion-waapi-oracle.py`, which says
/// how): ~140 are `animate(el, keyframes, options)` and ~45 are a `motion.div`. Each is replayed here on a paused clock
/// as the `motion` component that makes the same animation: `animate(el, kf, opts)` is a `motion.div` that mounts with
/// `animate={kf}` `transition={opts}` over the element's own style, and a second call is a render with the new props.
///
/// What is compared at every sample: how many animations are listed, and for each its class, `id`, play state, rate,
/// that its start time and current time agree, a current time within the lag of Chrome's frames, the timing
/// (`getTiming()`) and the keyframes (`getKeyframes()`). The cases a gpuix component cannot be made to run are listed
/// by name with the reason in `NOT_EXPRESSIBLE` and are not replayed; the controls cases (`ctl-*`) wait for the
/// commands on these animations (round 15b, S2d2).

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { createElement, type ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { document, motion, type PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

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
interface Sample {
  label: string
  t: number
  calls: number
  n: number
  anims: Listed[]
}
type Call =
  | { kind: "animate"; t: number; keyframes: Record<string, unknown>; options: Record<string, unknown> | null }
  | { kind: "render"; t: number; props: Record<string, unknown> }
  | { kind: "unmount"; t: number }
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
const CONTROLS = "controls on the animation (pause, play, seek, rate, stop, cancel, complete): round 15b, S2d2"
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
for (const scenario of ORACLE.cases) {
  if (scenario.group === "controls") NOT_EXPRESSIBLE.set(scenario.id, CONTROLS)
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

/** What a `motion` component runs for a call of the oracle's page. */
function elementFor(scenario: Case, call: Call, ref: (node: PublicInstance | null) => void): ReactElement {
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
      } as never)
    )
  }
  if (call.kind === "animate") {
    return createElement(motion.div, {
      ref,
      testId: "box",
      style: { ...BOX, ...inline },
      animate: call.keyframes,
      transition: transitionOf(call.options),
    } as never)
  }
  if (call.kind === "render") {
    return createElement(motion.div, { ref, testId: "box", ...reviveProps(call.props), style: BOX } as never)
  }
  throw new Error(`no element for a ${call.kind}`)
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
  let now = 0
  const advanceTo = (t: number) => {
    if (t <= now) return
    frame(root.renderer, t - now)
    now = t
  }
  let applied = 0
  /** Calls were made and no frame has been drawn since. */
  let undrawn = false
  try {
    const apply = (upTo: number) => {
      while (applied < upTo) {
        const call = scenario.calls[applied]
        applied++
        // A later call is a render with the new props over the same element; a call to unmount is the element going.
        root.render(call.kind === "unmount" ? createElement("div") : elementFor(scenario, call, ref))
        // Chrome styles the element when it is asked for its animations; a read of its box is how gpuix is made to.
        if (call.kind !== "unmount" && holder.current) root.renderer.getElementBounds(holder.current.id)
      }
      undrawn = true
    }
    for (const sample of scenario.samples) {
      const where = `${scenario.id} at ${sample.label} (${sample.t} ms)`
      const synchronous = /^(sync|microtask|timeout0)/.test(sample.label)
      if (applied < sample.calls) {
        // A later call happens at the time of the sample or just before it.
        if (applied > 0) advanceTo(sample.t)
        apply(sample.calls)
      }
      if (!synchronous) {
        if (undrawn) frame(root.renderer, 0)
        undrawn = false
        advanceTo(sample.t)
      }
      if (sample.label === "final") continue
      const element = holder.current
      expect(element, `${where}: element`).not.toBeNull()
      const got = element!.getAnimations()
      expect(
        got.map((animation) => animation.constructor.name),
        `${where}: listed (${got.length}, Chrome lists ${sample.n})`
      ).toEqual(sample.anims.map((animation) => animation.ctor))
      sample.anims.forEach((want, index) => {
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
        expect(animation.startTime! - animation.timeline!.currentTime!, `${here}: startTime`).toBeCloseTo(-animation.currentTime!, 3)
        const timing = animation.effect!.getTiming()
        for (const key of Object.keys(want.timing)) {
          const value = timing[key as keyof typeof timing]
          expectSameEasing(value === Infinity ? "Infinity" : value, want.timing[key], `${here}: timing.${key}`)
        }
        const keyframes = describeKeyframes(animation)
        expect(keyframes.length, `${here}: keyframes`).toBe(want.keyframes.length)
        // The values of an animation that took over from another start where that one was.
        const exact = sample.calls === 1 && !VALUES_DIFFER.has(scenario.id)
        keyframes.forEach((keyframe, position) => {
          const wantFrame = want.keyframes[position]
          expect(keyframe.offset, `${here}: keyframe ${position} offset`).toBe(wantFrame.offset)
          expect(keyframe.easing, `${here}: keyframe ${position} easing`).toBe(wantFrame.easing)
          expect(Object.keys(keyframe.props), `${here}: keyframe ${position} properties`).toEqual(Object.keys(wantFrame.props))
          if (exact) expect(keyframe.props, `${here}: keyframe ${position} values`).toEqual(wantFrame.props)
        })
      })
    }
  } finally {
    root.unmount()
  }
}

describeNative("Motion's animations in getAnimations() against Chrome", () => {
  it("is Chrome's list of cases, with every one replayed or named below", () => {
    expect(ORACLE.cases.length).toBe(185)
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
