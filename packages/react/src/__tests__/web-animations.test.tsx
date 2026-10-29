/// `element.getAnimations()` against headless Chrome: `fixtures/web-animations-oracle.html` runs each case in Chrome
/// and `fixtures/web-animations-oracle.json` is what it reports after every step (see the page for the step
/// vocabulary). Each case is replayed here on the test clock, paused, so a step takes no time; where Chrome waits for
/// frames (`settle`) a frame without time passing is drawn and the events it queued are delivered. What a step shows
/// is compared as Chrome shows it: the width and height of the element, and for every animation the first
/// `getAnimations()` returned its class, name, play state, current time, rate and, for the cases that play again after
/// a finish, which `finished` promise it holds. The cases the oracle cannot say are below it.
///
/// `apiCases` are the same format with the rest of the Web Animations playback interface: `cancel()`, `finish()`,
/// `reverse()`, `updatePlaybackRate()`, `ready`, `pending`, `startTime`, `timeline`, `onfinish`, `oncancel`, and the
/// events and promises Chrome makes of them, in its order. They are replayed on a window that draws only where Chrome
/// has a frame: the page reads in a step force a layout and no more, and `settle` draws three frames, 16 ms apart,
/// delivering what each queued. Chrome's frames take real time, so its `settle` puts a running animation back to the
/// time it had when the frames began; so does this one.
///
/// `treeCases` are trees of elements: what `getAnimations()`, `getAnimations({ subtree: true })` and
/// `document.getAnimations()` list after each step, in Chrome's order. The three that need `::before` or `::after`
/// are skipped, gpuix has no pseudo-elements.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { createElement, type ReactElement } from "react"
import { describe, expect, it } from "vitest"
import {
  Animation,
  CSSAnimation,
  CSSTransition,
  defineKeyframes,
  document,
  type HostAnimationEvent,
  type HostTransitionEvent,
  type PublicInstance,
} from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Step = [string, ...unknown[]]
interface AnimationRecord {
  kind: string
  name: string
  playState: string
  currentTime: number | null
  playbackRate: number
  finishedGeneration: number
}
interface Snapshot {
  step: Step
  width: number | null
  height: number | null
  animations: AnimationRecord[]
  list?: [string, string][]
  finished?: string[][]
  same?: boolean
}
interface Case {
  name: string
  style: Record<string, unknown>
  change: Record<string, unknown> | null
  steps: Step[]
  records: Snapshot[]
}
interface ApiAnimationRecord extends AnimationRecord {
  pending: boolean
  readyGeneration: number
  hasStartTime: boolean
  startTimeAge: number | null
  timelineIsDocument: boolean
  effectTarget: string
}
interface Sample {
  t: number
  localTime: number | null
  progress: number | null
  currentIteration: number | null
  endTime: number | string
  activeDuration: number | string
  width: number
}
interface ApiSnapshot {
  step: Step
  width: number | null
  height: number | null
  animations: ApiAnimationRecord[]
  events: string[]
  threw?: string
  list?: [string, string][]
  listIndex?: number[]
  finished?: string[][]
  ready?: string[][]
  keyframes?: unknown
  timing?: unknown
  computedTiming?: unknown
  samples?: Sample[]
}
/** `[id, style, change, children, className]`: a change is applied after the first layout, a class stands for a pseudo-element. */
type TreeNode = [string, Record<string, unknown>, Record<string, unknown> | null, TreeNode[], string | null]
interface TreeSnapshot {
  step: Step
  own: Record<string, string[]>
  subtree: Record<string, string[]>
  document: string[]
}
interface TreeCase {
  name: string
  tree: TreeNode
  steps: Step[]
  records: TreeSnapshot[]
}
interface ApiCase {
  name: string
  style: Record<string, unknown>
  change: Record<string, unknown> | null
  steps: Step[]
  records: ApiSnapshot[]
}
interface EffectCase {
  name: string
  tag: string
  attrs: Record<string, unknown>
  style: Record<string, unknown>
  keyframesName: string | null
  keyframes: string | null
  change: Record<string, unknown> | null
  attrChange: Record<string, unknown> | null
  update: Record<string, unknown> | null
  animations: { kind: string; name: string; keyframes: unknown; timing: unknown }[]
  updated: unknown[] | null
}
const ORACLE: {
  keyframes: Record<string, Record<string, Record<string, number>>>
  richKeyframes: Record<string, string>
  base: Record<string, string>
  cases: Case[]
  apiCases: ApiCase[]
  treeCases: TreeCase[]
  effectCases: EffectCase[]
} = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/web-animations-oracle.json"),
    "utf8"
  )
)

for (const [name, frames] of Object.entries(ORACLE.keyframes)) defineKeyframes(name, frames)

/** `0%{width:100px}50%{width:150px;animation-timing-function:ease-in}` as the frames `defineKeyframes` takes. */
function framesOf(body: string): Record<string, Record<string, string>> {
  const frames: Record<string, Record<string, string>> = {}
  for (const block of body.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const declarations = Object.fromEntries(
      block[2]
        .split(";")
        .filter((declaration) => declaration.trim() !== "")
        .map((declaration) => {
          const colon = declaration.indexOf(":")
          const name = declaration.slice(0, colon).trim().replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())
          return [name, declaration.slice(colon + 1).trim()]
        })
    )
    // Two blocks of one selector are one keyframe, the later declaration winning.
    frames[block[1].trim()] = { ...frames[block[1].trim()], ...declarations }
  }
  return frames
}

/** `animation-composition` is not supported, and the cases that set it have no replay (round 15b, S2b). */
const COMPOSITION = /composition/i

/** Whether the case sets `animation-composition` on the animation or in the keyframes it names. */
const composes = (scenario: { style: Record<string, unknown> }): boolean =>
  COMPOSITION.test(JSON.stringify(scenario.style)) ||
  Object.entries(ORACLE.richKeyframes).some(
    ([name, body]) => COMPOSITION.test(body) && String(scenario.style.animation ?? "").split(/\s+/).includes(name)
  )

for (const [name, body] of Object.entries(ORACLE.richKeyframes)) {
  if (!COMPOSITION.test(body)) defineKeyframes(name, framesOf(body))
}
/** `"2.5"` as the number a style or a keyframe takes for a property of numbers; text is left as it is. */
const numeric = <T extends Record<string, unknown>>(record: T): T =>
  Object.fromEntries(
    Object.entries(record).map(([key, value]) => [
      key,
      typeof value === "string" && /^-?\d*\.?\d+$/.test(value) ? Number.parseFloat(value) : value,
    ])
  ) as T

/** The effect cases whose keyframes gpuix refuses where they are defined, and why: it animates nothing of them. */
const REFUSED = new Map<string, string>()
for (const scenario of ORACLE.effectCases) {
  if (!scenario.keyframesName || !scenario.keyframes) continue
  try {
    defineKeyframes(
      scenario.keyframesName,
      Object.fromEntries(Object.entries(framesOf(scenario.keyframes)).map(([offset, frame]) => [offset, numeric(frame)]))
    )
  } catch (error) {
    REFUSED.set(scenario.name, (error as Error).message)
  }
}

/** `"50px"` as the number a style takes. */
const toStyle = (style: Record<string, unknown>): Record<string, unknown> =>
  Object.fromEntries(
    Object.entries(style).map(([key, value]) => [
      key,
      typeof value === "string" && /^-?[\d.]+px$/.test(value) ? Number.parseFloat(value) : value,
    ])
  )

const nextTask = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

/** How a `finished` promise stands, as the oracle page reads it. */
const standing = (promise: Promise<Animation>, animation: Animation): Promise<string> =>
  Promise.race([
    promise.then(
      (value) => (value === animation ? "resolved" : "resolved-other"),
      (error) => `rejected:${error instanceof DOMException ? error.name : String(error)}`
    ),
    nextTask().then(() => "pending"),
  ])

function nameOf(animation: Animation): string {
  if (animation instanceof CSSAnimation) return animation.animationName
  if (animation instanceof CSSTransition) return animation.transitionProperty
  throw new Error("An animation that is neither a CSSAnimation nor a CSSTransition")
}

async function replay(scenario: Case): Promise<Snapshot[]> {
  const { render, renderer, unmount } = createTestRoot()
  renderer.clockPause()
  const element: { current: PublicInstance | null } = { current: null }
  let style: Record<string, unknown> = { ...ORACLE.base, ...scenario.style }
  const box = () => (
    <div
      testId="box"
      ref={(node: PublicInstance | null) => {
        element.current = node
      }}
      style={toStyle(style)}
    />
  )
  const frame = () => {
    renderer.clockFastForward(0)
    renderer.dispatchNativeEvents()
  }
  try {
    render(box())
    if (scenario.change) {
      style = { ...style, ...scenario.change }
      render(box())
    }
    frame()
    const tracked = element.current!.getAnimations()
    const promises: Promise<Animation>[][] = tracked.map(() => [])

    const read = (): Omit<Snapshot, "step"> => {
      frame()
      const mounted = element.current !== null
      const bounds = mounted ? renderer.getElementBounds(element.current!.id) : null
      return {
        width: bounds ? bounds.width : null,
        height: bounds ? bounds.height : null,
        animations: tracked.map((animation, index) => {
          const seen = promises[index]
          const finished = animation.finished
          if (!seen.includes(finished)) seen.push(finished)
          return {
            kind: animation.constructor.name,
            name: nameOf(animation),
            playState: animation.playState,
            currentTime: animation.currentTime,
            playbackRate: animation.playbackRate,
            finishedGeneration: seen.indexOf(finished),
          }
        }),
      }
    }

    const records: Snapshot[] = [{ step: ["start"], ...read() }]
    for (const step of scenario.steps) {
      const [op, first, second] = step
      const record: Partial<Snapshot> & { step: Step } = { step }
      if (op === "pause") tracked[first as number].pause()
      else if (op === "play") tracked[first as number].play()
      else if (op === "seek") tracked[first as number].currentTime = second as number
      else if (op === "rate") tracked[first as number].playbackRate = second as number
      else if (op === "style") {
        style = { ...style, ...(first as Record<string, unknown>) }
        render(box())
      } else if (op === "remove") render(<></>)
      else if (op === "settle") {
        frame()
        record.finished = await Promise.all(
          tracked.map((animation, index) => Promise.all(promises[index].map((promise) => standing(promise, animation))))
        )
      }
      if (op === "list") {
        record.list = element.current!.getAnimations().map((animation) => [animation.constructor.name, nameOf(animation)])
      }
      if (op === "same") {
        const again = element.current!.getAnimations()
        const once = element.current!.getAnimations()
        record.same =
          again.length === once.length && again.every((animation, index) => animation === once[index] && animation === tracked[index])
      }
      records.push({ ...record, ...read() } as Snapshot)
    }
    return records
  } finally {
    unmount()
  }
}

describeNative("Web animations against Chrome", () => {
  for (const scenario of ORACLE.cases) {
    // The first window of a loaded machine can take longer than the default 5s to open.
    it(scenario.name, { timeout: 30_000 }, async () => {
      const got = await replay(scenario)
      expect(got.length).toBe(scenario.records.length)
      scenario.records.forEach((want, index) => {
        const here = got[index]
        const where = `${scenario.name}, after ${JSON.stringify(want.step)}`
        expect(here.step, where).toEqual(want.step)
        if (want.width === null) expect(here.width, `${where}: width`).toBeNull()
        else expect(here.width, `${where}: width`).toBeCloseTo(want.width, 0)
        if (want.height === null) expect(here.height, `${where}: height`).toBeNull()
        else expect(here.height, `${where}: height`).toBeCloseTo(want.height, 0)
        want.animations.forEach((expected, animation) => {
          const actual = here.animations[animation]
          const label = `${where}: animation ${animation}`
          expect(actual.kind, label).toBe(expected.kind)
          expect(actual.name, label).toBe(expected.name)
          expect(actual.playState, `${label} playState`).toBe(expected.playState)
          if (expected.currentTime === null) expect(actual.currentTime, `${label} currentTime`).toBeNull()
          else expect(actual.currentTime, `${label} currentTime`).toBeCloseTo(expected.currentTime, 3)
          expect(actual.playbackRate, `${label} playbackRate`).toBe(expected.playbackRate)
          if (want.step[0] === "play") {
            expect(actual.finishedGeneration, `${label} finished promise`).toBe(expected.finishedGeneration)
          }
        })
        if (want.list) expect(here.list, `${where}: getAnimations()`).toEqual(want.list)
        if (want.same !== undefined) expect(here.same, `${where}: the same objects`).toBe(want.same)
        if (want.finished) {
          // The promise the animation held first: a cancel rejects it and leaves a pending one.
          expect(here.finished!.map((promises) => promises[0]), `${where}: finished`).toEqual(
            want.finished.map((promises) => promises[0])
          )
        }
      })
    })
  }
})

/** A number as the oracle writes it: three decimals, `"Infinity"` for infinity. */
const rounded = (value: number | null): number | string | null => {
  if (value === null) return null
  return Number.isFinite(value) ? Math.round(value * 1000) / 1000 : String(value)
}

/** What the oracle page makes of a value it records: numbers rounded, objects as JSON. */
const clean = (value: unknown): unknown =>
  JSON.parse(JSON.stringify(value, (_, member: unknown) => (typeof member === "number" ? rounded(member) : member)))

/** `updateTiming({iterations: "Infinity"})` as the page revives it. */
const revive = (timing: Record<string, unknown>): Record<string, unknown> =>
  Object.fromEntries(Object.entries(timing).map(([key, value]) => [key, value === "Infinity" ? Infinity : value]))

/**
 * Chrome tells the `transitionrun` and `transitionstart` of a transition either before the `ready` of its first
 * frame or after, depending on whether the style was resolved by a read before that frame or in it: the same page
 * gave both orders for the same transition. The leading run of those events is compared with the `ready`s last.
 */
const withoutFirstFrameOrder = (events: string[]): string[] => {
  const first = (event: string) => /^a\d+\.ready resolved$|^transition(run|start) /.test(event)
  const lead = events.findIndex((event) => !first(event))
  const head = lead === -1 ? events : events.slice(0, lead)
  const ready = head.filter((event) => event.includes(".ready resolved"))
  return [...head.filter((event) => !ready.includes(event)), ...ready, ...events.slice(head.length)]
}

const microtasks = async () => {
  for (let index = 0; index < 5; index++) await null
}

const DOM_EVENT_TYPES = new Set([
  "animationstart",
  "animationiteration",
  "animationend",
  "animationcancel",
  "transitionrun",
  "transitionstart",
  "transitionend",
  "transitioncancel",
])

/**
 * Replays an `apiCases` case. Chrome draws no frame between two steps of a case and three at `settle`, so the
 * window draws only there (the reads in between force a layout, as the page's do); the clock moves at the frames,
 * 16 ms each, which is why the events of a step carry the time of a frame before the one they are told in.
 */
async function replayApi(scenario: ApiCase): Promise<ApiSnapshot[]> {
  const { render, renderer, unmount } = createTestRoot()
  renderer.clockPause()
  renderer.setInlineDraws(false)
  const element: { current: PublicInstance | null } = { current: null }
  // The element the effects name, which they keep after it is gone.
  let target: PublicInstance | null = null
  const log: string[] = []
  let disposed = false
  // Chrome runs the promise reactions between two events of a frame, a microtask checkpoint apart. The events
  // of a window are delivered in one go, so what they log is put behind the reactions that were queued first.
  const note = (text: string) => queueMicrotask(() => log.push(text))
  const heard =
    (type: string) =>
    (event: HostAnimationEvent | HostTransitionEvent): void => {
      const name = "animationName" in event ? event.animationName : event.propertyName
      note(`${type} ${name} elapsed=${rounded(event.elapsedTime)} pseudo=${event.pseudoElement}`)
    }
  let style: Record<string, unknown> = { ...ORACLE.base, ...scenario.style }
  const box = () => (
    <div
      testId="box"
      ref={(node: PublicInstance | null) => {
        element.current = node
        target ??= node
      }}
      style={toStyle(style)}
      onAnimationStart={heard("animationstart")}
      onAnimationIteration={heard("animationiteration")}
      onAnimationEnd={heard("animationend")}
      onAnimationCancel={heard("animationcancel")}
      onTransitionRun={heard("transitionrun")}
      onTransitionStart={heard("transitionstart")}
      onTransitionEnd={heard("transitionend")}
      onTransitionCancel={heard("transitioncancel")}
    />
  )
  /** A read of the page that forces the style and layout. */
  const layout = (): { width: number; height: number } | null => {
    const mounted = element.current
    return mounted ? renderer.getElementBounds(mounted.id) : null
  }
  const tracked: Animation[] = []
  const finishedSeen: Promise<Animation>[][] = []
  const readySeen: Promise<Animation>[][] = []
  const watch = (seen: Promise<Animation>[], which: string, index: number, promise: Promise<Animation>) => {
    if (seen.includes(promise)) return
    seen.push(promise)
    promise.then(
      () => log.push(`a${index}.${which} resolved`),
      (error: unknown) => log.push(`a${index}.${which} rejected:${error instanceof DOMException ? error.name : String(error)}`)
    )
  }
  const track = () => {
    for (const animation of element.current?.getAnimations() ?? []) {
      if (tracked.includes(animation)) continue
      const index = tracked.length
      tracked.push(animation)
      finishedSeen.push([])
      readySeen.push([])
      const describe = (event: { type: string; currentTime: number | null; timelineTime: number | null }) => {
        if (disposed) return `a${index}.${event.type} after the window is gone`
        const now = animation.timeline.currentTime
        const side = event.timelineTime === null ? null : event.timelineTime < now ? "before" : event.timelineTime === now ? "now" : "after"
        return `a${index}.${event.type} currentTime=${rounded(event.currentTime)} timelineTime=${side}`
      }
      animation.oncancel = (event) => note(`${describe(event)} (handler)`)
      animation.onfinish = (event) => note(`${describe(event)} (handler)`)
      for (const type of ["cancel", "finish"]) animation.addEventListener(type, (event) => note(describe(event)))
    }
  }
  const relative = (value: unknown): unknown => {
    if (typeof value !== "string") return value
    const matched = /^now([+-]\d+)?$/.exec(value)
    if (!matched) throw new Error(`Unknown start time ${value}`)
    return tracked[0].timeline.currentTime + (matched[1] ? Number(matched[1]) : 0)
  }
  const read = (): Pick<ApiSnapshot, "width" | "height" | "animations"> => {
    const bounds = layout()
    return {
      width: bounds ? bounds.width : null,
      height: bounds ? bounds.height : null,
      animations: tracked.map((animation, index) => {
        watch(finishedSeen[index], "finished", index, animation.finished)
        watch(readySeen[index], "ready", index, animation.ready)
        const startTime = animation.startTime
        return {
          kind: animation.constructor.name,
          name: nameOf(animation),
          playState: animation.playState,
          currentTime: animation.currentTime,
          playbackRate: animation.playbackRate,
          finishedGeneration: finishedSeen[index].indexOf(animation.finished),
          pending: animation.pending,
          readyGeneration: readySeen[index].indexOf(animation.ready),
          hasStartTime: startTime !== null,
          startTimeAge:
            animation.playState === "running" && !animation.pending && startTime !== null && animation.playbackRate !== 0
              ? Math.round(animation.timeline.currentTime - startTime)
              : null,
          timelineIsDocument: animation.timeline === tracked[0].timeline,
          effectTarget: animation.effect.target === target ? "element" : animation.effect.target === null ? "null" : "other",
        }
      }),
    }
  }
  const standings = (seen: Promise<Animation>[][]) =>
    Promise.all(tracked.map((animation, index) => Promise.all(seen[index].map((promise) => standing(promise, animation)))))
  try {
    style = { ...style }
    render(box())
    layout()
    if (scenario.change) {
      style = { ...style, ...scenario.change }
      render(box())
      layout()
    }
    track()
    const records: ApiSnapshot[] = [{ step: ["start"], ...read(), events: log.splice(0) }]
    for (const step of scenario.steps) {
      const [op, first, second] = step
      const record: Partial<ApiSnapshot> & { step: Step } = { step }
      try {
        if (op === "pause") tracked[first as number].pause()
        else if (op === "play") tracked[first as number].play()
        else if (op === "seek") tracked[first as number].currentTime = second as number
        else if (op === "rate") tracked[first as number].playbackRate = second as number
        else if (op === "cancel") tracked[first as number].cancel()
        else if (op === "finish") tracked[first as number].finish()
        else if (op === "reverse") tracked[first as number].reverse()
        else if (op === "updateRate") tracked[first as number].updatePlaybackRate(second as number)
        else if (op === "startTime") tracked[first as number].startTime = relative(second) as number | null
        else if (op === "updateTiming") tracked[first as number].effect.updateTiming(revive(second as Record<string, unknown>))
        else if (op === "style") {
          style = { ...style, ...(first as Record<string, unknown>) }
          render(box())
          layout()
        } else if (op === "remove") render(<></>)
        else if (op === "track") track()
        else if (op === "settle") {
          const held = tracked.map((animation) => animation.currentTime)
          for (let frame = 0; frame < 3; frame++) {
            renderer.clockFastForward(16)
            renderer.drawFrame()
            renderer.dispatchNativeEvents()
          }
          record.finished = await standings(finishedSeen)
          record.ready = await standings(readySeen)
          tracked.forEach((animation, index) => {
            const was = held[index]
            if (animation.pending || was === null || animation.playState === "finished" || animation.playState === "idle") return
            const now = animation.currentTime
            if (now === null || Math.abs(now - was) > 1e-6) animation.currentTime = was
          })
        } else if (!["list", "keyframes", "timing", "computed", "sample"].includes(op)) {
          throw new Error(`Unknown step ${op as string}`)
        }
        if (op === "list") {
          const listed = element.current?.getAnimations() ?? []
          record.list = listed.map((animation) => [animation.constructor.name, nameOf(animation)])
          record.listIndex = listed.map((animation) => tracked.indexOf(animation))
        }
        if (op === "keyframes") record.keyframes = clean(tracked[first as number].effect.getKeyframes())
        if (op === "timing") record.timing = clean(tracked[first as number].effect.getTiming())
        if (op === "computed") record.computedTiming = clean(tracked[first as number].effect.getComputedTiming())
        if (op === "sample") {
          record.samples = (second as number[]).map((time) => {
            const animation = tracked[first as number]
            animation.currentTime = time
            const computed = animation.effect.getComputedTiming()
            const bounds = layout()
            return clean({
              t: time,
              localTime: computed.localTime,
              progress: computed.progress,
              currentIteration: computed.currentIteration,
              endTime: computed.endTime,
              activeDuration: computed.activeDuration,
              width: bounds ? bounds.width : null,
            }) as Sample
          })
        }
      } catch (error) {
        record.threw = error instanceof Error ? error.name : String(error)
      }
      await microtasks()
      Object.assign(record, read())
      await microtasks()
      record.events = log.splice(0)
      records.push(record as ApiSnapshot)
    }
    return records
  } finally {
    // Unmounting removes the element, whose animations are cancelled in a task of their own: nobody hears it.
    disposed = true
    unmount()
  }
}

describeNative("Web animations playback against Chrome", () => {
  for (const scenario of ORACLE.apiCases.filter((scenario) => !composes(scenario))) {
    it(scenario.name, { timeout: 30_000 }, async () => {
      const got = await replayApi(scenario)
      expect(got.length).toBe(scenario.records.length)
      // An element that is gone hears no DOM event: its handlers went with it.
      let removed = false
      scenario.records.forEach((want, index) => {
        const here = got[index]
        const where = `${scenario.name}, after ${JSON.stringify(want.step)}`
        expect(here.step, where).toEqual(want.step)
        expect(here.threw, `${where}: threw`).toBe(want.threw)
        if (want.width === null) expect(here.width, `${where}: width`).toBeNull()
        else expect(here.width, `${where}: width`).toBeCloseTo(want.width, 0)
        if (want.height === null) expect(here.height, `${where}: height`).toBeNull()
        else expect(here.height, `${where}: height`).toBeCloseTo(want.height, 0)
        expect(here.animations.length, `${where}: animations`).toBe(want.animations.length)
        want.animations.forEach((expected, animation) => {
          const actual = here.animations[animation]
          const label = `${where}: animation ${animation}`
          expect(actual.kind, label).toBe(expected.kind)
          expect(actual.name, label).toBe(expected.name)
          expect(actual.playState, `${label} playState`).toBe(expected.playState)
          if (expected.currentTime === null) expect(actual.currentTime, `${label} currentTime`).toBeNull()
          else expect(actual.currentTime, `${label} currentTime`).toBeCloseTo(expected.currentTime, 3)
          expect(actual.playbackRate, `${label} playbackRate`).toBe(expected.playbackRate)
          expect(actual.finishedGeneration, `${label} finished promise`).toBe(expected.finishedGeneration)
          expect(actual.pending, `${label} pending`).toBe(expected.pending)
          expect(actual.readyGeneration, `${label} ready promise`).toBe(expected.readyGeneration)
          expect(actual.hasStartTime, `${label} hasStartTime`).toBe(expected.hasStartTime)
          expect(actual.startTimeAge, `${label} startTimeAge`).toBe(expected.startTimeAge)
          expect(actual.timelineIsDocument, `${label} timeline`).toBe(expected.timelineIsDocument)
          expect(actual.effectTarget, `${label} effect.target`).toBe(expected.effectTarget)
        })
        if (want.keyframes !== undefined) expect(here.keyframes, `${where}: getKeyframes()`).toEqual(want.keyframes)
        if (want.timing !== undefined) expect(here.timing, `${where}: getTiming()`).toEqual(want.timing)
        if (want.computedTiming !== undefined) expect(here.computedTiming, `${where}: getComputedTiming()`).toEqual(want.computedTiming)
        if (want.samples) {
          expect(here.samples?.length, `${where}: samples`).toBe(want.samples.length)
          want.samples.forEach((sample, at) => {
            const got = here.samples![at]
            const label = `${where}: sample at ${sample.t}`
            expect({ ...got, width: undefined }, label).toEqual({ ...sample, width: undefined })
            expect(got.width, `${label}: width`).toBeCloseTo(sample.width, 1)
          })
        }
        if (want.list) expect(here.list, `${where}: getAnimations()`).toEqual(want.list)
        if (want.listIndex) expect(here.listIndex, `${where}: which animations`).toEqual(want.listIndex)
        if (want.finished) expect(here.finished, `${where}: finished`).toEqual(want.finished)
        if (want.ready) expect(here.ready, `${where}: ready`).toEqual(want.ready)
        if (want.step[0] === "remove") removed = true
        const heard = (events: string[]) =>
          removed ? events.filter((event) => !DOM_EVENT_TYPES.has(event.split(" ")[0])) : events
        expect(withoutFirstFrameOrder(heard(here.events)), `${where}: events`).toEqual(
          withoutFirstFrameOrder(heard(want.events))
        )
      })
    })
  }
})

/** gpuix has no `::before` or `::after`: a class in a tree case stands for one. */
const usesPseudoElements = (scenario: TreeCase): boolean => {
  const classed = (node: TreeNode): boolean => node[4] !== null || node[3].some(classed)
  return (
    classed(scenario.tree) ||
    scenario.steps.some((step) => step[0] === "classes" || (step[0] === "add" && classed(step[2] as TreeNode)))
  )
}

interface Node {
  id: string
  style: Record<string, unknown>
  children: Node[]
}

/** Replays a `treeCases` case: what `getAnimations()`, with and without `subtree`, and `document.getAnimations()` list. */
async function replayTree(scenario: TreeCase): Promise<TreeSnapshot[]> {
  const { render, renderer, unmount } = createTestRoot()
  renderer.clockPause()
  renderer.setInlineDraws(false)
  const nodes = new Map<string, Node>()
  const changes: [string, Record<string, unknown>][] = []
  const build = (source: TreeNode): Node => {
    const node: Node = { id: source[0], style: { ...ORACLE.base, ...source[1] }, children: source[3].map(build) }
    nodes.set(node.id, node)
    if (source[2]) changes.push([node.id, source[2]])
    return node
  }
  const applyChanges = () => {
    for (const [id, change] of changes.splice(0)) nodes.get(id)!.style = { ...nodes.get(id)!.style, ...change }
  }
  const root = build(scenario.tree)
  const instances = new Map<string, PublicInstance>()
  const ids = new Map<PublicInstance, string>()
  const view = (node: Node): ReactElement => (
    <div
      key={node.id}
      testId={node.id}
      ref={(instance: PublicInstance | null) => {
        if (!instance) return
        instances.set(node.id, instance)
        ids.set(instance, node.id)
      }}
      style={toStyle(node.style)}
    >
      {node.children.map(view)}
    </div>
  )
  const show = () => {
    render(view(root))
    renderer.getElementBounds(instances.get(root.id)!.id)
  }
  const parentOf = (id: string): Node => {
    for (const node of nodes.values()) if (node.children.some((child) => child.id === id)) return node
    throw new Error(`${id} has no parent`)
  }
  const place = (parent: Node, node: Node, before: string | null) => {
    const at = before === null ? parent.children.length : parent.children.findIndex((child) => child.id === before)
    parent.children.splice(at, 0, node)
  }
  const label = (animation: Animation): string => {
    const target = animation.effect.target
    return `${animation.constructor.name}:${nameOf(animation)}@${ids.get(target as PublicInstance)}`
  }
  const held = new Map<string, Animation>()
  const read = (): Omit<TreeSnapshot, "step"> => ({
    own: Object.fromEntries([...instances].map(([id, instance]) => [id, instance.getAnimations().map(label)])),
    subtree: Object.fromEntries(
      [...instances].map(([id, instance]) => [id, instance.getAnimations({ subtree: true }).map(label)])
    ),
    document: document.getAnimations().map(label),
  })
  try {
    show()
    applyChanges()
    show()
    const records: TreeSnapshot[] = [{ step: ["start"], ...read() }]
    for (const step of scenario.steps) {
      const [op, first, second, third] = step as [string, ...unknown[]]
      const record = { step } as TreeSnapshot
      if (op === "styles") {
        for (const [id, style] of Object.entries(first as Record<string, Record<string, unknown>>)) {
          nodes.get(id)!.style = { ...nodes.get(id)!.style, ...style }
        }
        show()
      } else if (op === "cancel" || op === "finish") {
        const animation = instances.get(first as string)!.getAnimations()[second as number]
        held.set(`${first}:${second}`, animation)
        if (op === "cancel") animation.cancel()
        else animation.finish()
      } else if (op === "play") held.get(`${first}:${second}`)!.play()
      else if (op === "add") {
        place(nodes.get(first as string)!, build(second as TreeNode), third as string | null)
        show()
        applyChanges()
        show()
      } else if (op === "remove") {
        const parent = parentOf(first as string)
        parent.children = parent.children.filter((child) => child.id !== first)
        show()
      } else if (op === "move") {
        const node = nodes.get(first as string)!
        const parent = parentOf(first as string)
        parent.children = parent.children.filter((child) => child !== node)
        place(nodes.get(second as string)!, node, third as string | null)
        show()
      } else if (op === "settle") {
        for (let frame = 0; frame < 3; frame++) {
          renderer.clockFastForward(16)
          renderer.drawFrame()
          renderer.dispatchNativeEvents()
        }
      } else if (op !== "list") throw new Error(`Unknown step ${op}`)
      await microtasks()
      records.push({ ...record, ...read() })
    }
    return records
  } finally {
    unmount()
  }
}

describeNative("Which animations getAnimations() lists, and in what order, against Chrome", () => {
  for (const scenario of ORACLE.treeCases) {
    // The `::before` and `::after` cases: gpuix has no pseudo-elements.
    const run = usesPseudoElements(scenario) ? it.skip : it
    run(scenario.name, { timeout: 30_000 }, async () => {
      const got = await replayTree(scenario)
      expect(got.length).toBe(scenario.records.length)
      scenario.records.forEach((want, index) => {
        const where = `${scenario.name}, after ${JSON.stringify(want.step)}`
        expect(got[index].step, where).toEqual(want.step)
        expect(got[index].document, `${where}: document.getAnimations()`).toEqual(want.document)
        for (const [id, listed] of Object.entries(want.own)) {
          expect(got[index].own[id], `${where}: ${id}.getAnimations()`).toEqual(listed)
        }
        for (const [id, listed] of Object.entries(want.subtree)) {
          expect(got[index].subtree[id], `${where}: ${id}.getAnimations({ subtree: true })`).toEqual(listed)
        }
      })
    })
  }
})

describeNative("Web animations beyond what Chrome can pin on a paused page", () => {
  const widthOf = (renderer: ReturnType<typeof createTestRoot>["renderer"]) => {
    renderer.dispatchNativeEvents()
    return renderer.getElementBounds(renderer.findByTestId("box").id).width
  }
  const grower = (animation: string, ref: (node: PublicInstance | null) => void) => (
    <div testId="box" ref={ref} style={{ width: 50, height: 20, animation }} />
  )

  it("lists the animations below a display: contents element and gives the same objects as the elements do", () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const parent: { current: PublicInstance | null } = { current: null }
    const child: { current: PublicInstance | null } = { current: null }
    render(
      <div testId="parent" ref={(node: PublicInstance | null) => (parent.current = node)} style={{ display: "contents" }}>
        {grower("grow 1s linear", (node) => (child.current = node))}
      </div>
    )
    renderer.clockFastForward(0)
    const [own] = child.current!.getAnimations()
    expect(parent.current!.getAnimations()).toEqual([])
    const [below] = parent.current!.getAnimations({ subtree: true })
    expect(below).toBe(own)
    expect(below.effect.target).toBe(child.current)
    const [listed] = document.getAnimations()
    expect(listed).toBe(own)
    unmount()
  })

  it("runs time at the playbackRate on the test clock and finishes when it gets to the end", async () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const element: { current: PublicInstance | null } = { current: null }
    render(grower("grow 1s linear forwards", (node) => {
        element.current = node
      }))
    renderer.clockFastForward(0)
    const [animation] = element.current!.getAnimations()
    animation.playbackRate = 2
    renderer.clockFastForward(250)
    expect(animation.currentTime).toBeCloseTo(500, 3)
    expect(widthOf(renderer)).toBeCloseTo(150, 0)
    animation.playbackRate = -1
    renderer.clockFastForward(100)
    expect(animation.currentTime).toBeCloseTo(400, 3)
    expect(widthOf(renderer)).toBeCloseTo(140, 0)
    animation.playbackRate = 1
    renderer.clockFastForward(1000)
    expect(animation.playState).toBe("finished")
    expect(animation.currentTime).toBeCloseTo(1000, 3)
    renderer.dispatchNativeEvents()
    await expect(animation.finished).resolves.toBe(animation)
    unmount()
  })

  it("plays a paused animation from the time it holds", () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const element: { current: PublicInstance | null } = { current: null }
    render(grower("grow 1s linear", (node) => {
        element.current = node
      }))
    renderer.clockFastForward(0)
    const [animation] = element.current!.getAnimations()
    renderer.clockFastForward(300)
    animation.pause()
    renderer.clockFastForward(5000)
    expect(animation.currentTime).toBeCloseTo(300, 3)
    animation.play()
    expect(animation.playState).toBe("running")
    // The frame that play() asks for is drawn at once on the test clock, so time runs from the call.
    renderer.clockFastForward(100)
    renderer.clockFastForward(200)
    expect(animation.currentTime).toBeCloseTo(600, 0)
    expect(widthOf(renderer)).toBeCloseTo(160, 0)
    unmount()
  })

  it("keeps a finished animation listed through its fill and finishes a transition once", async () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const element: { current: PublicInstance | null } = { current: null }
    const target = (width: number) => (
      <div
        testId="box"
        ref={(node: PublicInstance | null) => {
          element.current = node
        }}
        style={{ width, height: 20, transition: "width 1s linear" }}
      />
    )
    render(target(50))
    render(target(150))
    renderer.clockFastForward(0)
    const [transition] = element.current!.getAnimations()
    expect(transition).toBeInstanceOf(CSSTransition)
    renderer.clockFastForward(1500)
    renderer.dispatchNativeEvents()
    expect(element.current!.getAnimations()).toEqual([])
    expect(transition.playState).toBe("finished")
    expect(transition.currentTime).toBe(1000)
    await expect(transition.finished).resolves.toBe(transition)
    transition.play()
    expect(transition.playState).toBe("running")
    expect(transition.currentTime).toBe(0)
    expect(element.current!.getAnimations()).toEqual([transition])
    unmount()
  })

  describe("reading an animation before JS has heard that it ended", () => {
    const setup = () => {
      const root = createTestRoot()
      root.renderer.clockPause()
      const element: { current: PublicInstance | null } = { current: null }
      const target = (width: number) => (
        <div
          testId="box"
          ref={(node: PublicInstance | null) => {
            element.current = node
          }}
          style={{ width, height: 20, transition: "width 1s linear" }}
        />
      )
      root.render(target(50))
      root.render(target(150))
      root.renderer.clockFastForward(0)
      root.renderer.dispatchNativeEvents()
      return { ...root, element, target, transition: () => element.current!.getAnimations()[0] }
    }

    it("answers for a transition in the task of the frame that finished it", async () => {
      const { renderer, transition: listed, unmount } = setup()
      const transition = listed()
      renderer.clockFastForward(1500)
      // The frame queued its finish event; nothing has delivered it.
      expect(transition.playState).toBe("finished")
      expect(transition.currentTime).toBe(1000)
      expect(transition.playbackRate).toBe(1)
      renderer.dispatchNativeEvents()
      expect(transition.playState).toBe("finished")
      expect(transition.currentTime).toBe(1000)
      await expect(transition.finished).resolves.toBe(transition)
      unmount()
    })

    it("answers idle for a run cancelled in the task of the frame that cancelled it, which native keeps until the app lets go", async () => {
      const { render, renderer, element, target, transition: listed, unmount } = setup()
      const transition = listed()
      const id = renderer.getAnimations(element.current!.id)[0].id
      // A cancel rejects the promise held when it comes, and leaves a new pending one.
      const held = transition.finished
      renderer.clockFastForward(300)
      render(target(80))
      renderer.clockFastForward(0)
      expect(transition.playState).toBe("idle")
      expect(transition.currentTime).toBeNull()
      expect(transition.playbackRate).toBe(1)
      expect(renderer.getAnimation(id).playState).toBe("idle")
      renderer.dispatchNativeEvents()
      expect(transition.playState).toBe("idle")
      await expect(held).rejects.toMatchObject({ name: "AbortError" })
      // The run stays, idle, for a command to bring it back; one native has dropped is cancelled for good.
      expect(renderer.getAnimation(id).playState).toBe("idle")
      renderer.forgetAnimation(id)
      expect(() => renderer.getAnimation(id)).toThrow()
      expect(transition.playState).toBe("idle")
      expect(() => transition.pause()).toThrow(DOMException)
      unmount()
    })

    it("releases the run of an event no animation object was made for", () => {
      const { render, renderer, element, target, unmount } = setup()
      // Native only: `element.getAnimations()` is what makes the object.
      const id = renderer.getAnimations(element.current!.id)[0].id
      renderer.clockFastForward(300)
      render(target(80))
      renderer.clockFastForward(0)
      expect(renderer.getAnimation(id).playState).toBe("idle")
      renderer.dispatchNativeEvents()
      expect(() => renderer.getAnimation(id)).toThrow()
      unmount()
    })

    it("keeps answering for a transition that finished after its event was heard, until the style moves off its end", () => {
      const { render, renderer, element, target, transition: listed, unmount } = setup()
      const transition = listed()
      const id = renderer.getAnimations(element.current!.id)[0].id
      renderer.clockFastForward(1500)
      renderer.dispatchNativeEvents()
      expect(renderer.getAnimation(id).playState).toBe("finished")
      render(target(150))
      renderer.clockFastForward(0)
      renderer.dispatchNativeEvents()
      expect(transition.playState).toBe("finished")
      render(target(90))
      renderer.clockFastForward(0)
      renderer.dispatchNativeEvents()
      expect(transition.playState).toBe("idle")
      expect(renderer.getAnimation(id).playState).toBe("idle")
      renderer.forgetAnimation(id)
      expect(() => renderer.getAnimation(id)).toThrow()
      unmount()
    })
  })

  it("keeps the internal events away from the listeners of the app", () => {
    const { render, renderer, unmount } = createTestRoot()
    renderer.clockPause()
    const seen: string[] = []
    const element: { current: PublicInstance | null } = { current: null }
    render(
      <div
        testId="box"
        ref={(node: PublicInstance | null) => {
          element.current = node
        }}
        onAnimationEnd={(event) => seen.push(event.type)}
        style={{ width: 50, height: 20, animation: "grow 1s linear" }}
      />
    )
    renderer.clockFastForward(0)
    const [animation] = element.current!.getAnimations()
    renderer.clockFastForward(2000)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual(["animationend"])
    expect(animation.playState).toBe("finished")
    unmount()
  })
})

/**
 * The cases of `effectCases`: what `getKeyframes()` and `getTiming()` read for a CSS animation or transition of each
 * kind of value. They read the same whatever the time, so the cases of one kind of element share a window: a `div`
 * for each case of a `div`, a shape in one `<svg>` for each of a shape.
 *
 * Chrome's own answers that gpuix has no value to give them from are skipped, each for a reason.
 */
const EFFECT_SKIPS: [RegExp, string][] = [
  // A length of 0% is the number 0 once parsed, which reads back as 0px.
  [/border-top-right-radius in percent|border-radius in percent|flex of one number|flex with a percent basis|transform-origin (keywords|of one keyword)/, "0% reads 0px"],
  // An em on an SVG length is not a number of px, which is all an SVG attribute holds here.
  [/^animation: text font-size in em/, "an SVG length in em is not read"],
  // A two-value border-color in a style is not read as the shorthand.
  [/^transition: border-color$/, "a two-value border-color is not read"],
  // A percentage of 1em is folded into px when the thickness is parsed.
  [/text-decoration-thickness in percent/, "a percentage thickness is folded into px"],
  [/font-weight relative/, "lighter and bolder are not parsed"],
  // Chrome writes the colour a transition starts from as black when it was written currentcolor.
  [/^transition: (border-top-color|box-shadow with) currentcolor/, "Chrome writes currentcolor as black in a transition"],
  // Chrome starts no transition between these two, gpuix does (or the other way round).
  [/^transition: (text-underline-offset from auto|text-decoration-thickness from|aspect-ratio with auto and a ratio|translate of one value|transform translateZ)/, "a transition gpuix does not start as Chrome does"],
  [/^transition: transform-origin of one value/, "a transition gpuix does not start as Chrome does"],
]

/** The cases whose colours are Chrome's to the last digits but one: see `closeEnough`. */
const APPROXIMATE = new Set<string>([
  "transition: background-color lab(50% 40 59)",
  "transition: background-color lab(50% 40 59 / 0.5)",
  "transition: background-color lch(50% 80 30)",
  "transition: background-color lch(50% 80 30deg / 0.4)",
  "transition: background-color color(srgb 1 0 0)",
  "transition: background-color color(srgb 0.5 0.25 0.1 / 0.5)",
  "transition: background-color color(srgb-linear 0.5 0.5 0.5)",
  "transition: background-color color(display-p3 1 0 0)",
  "transition: background-color color(a98-rgb 1 0 0)",
  "transition: background-color color(prophoto-rgb 1 0 0)",
  "transition: background-color color(rec2020 1 0 0)",
  "transition: background-color color(xyz 0.2 0.3 0.4)",
  "transition: background-color color(xyz-d50 0.2 0.3 0.4)",
  "transition: background-color color-mix(in srgb, red, blue)",
  "animation: background-color color-mix(in oklch, red 30%, blue)",
  "transition: background-color color-mix(in oklch, red 30%, blue)",
  "transition: background-color color-mix(in srgb, red 50%, transparent)",
  "transition: background-color color-mix(in srgb, #f00, #00f 25%)",
  "transition: background-color color-mix(in hsl, red, blue)",
  "transition: background-color color-mix(in hwb, red, blue)",
  "animation: background-color color-mix(in lab, red, blue)",
  "transition: background-color color-mix(in lab, red, blue)",
  "animation: background-color color-mix(in oklab, red, blue)",
  "transition: background-color color-mix(in oklab, red, blue)",
  "animation: background-color color-mix(in lch longer hue, red, blue)",
  "transition: background-color color-mix(in lch longer hue, red, blue)",
  "animation: background-color color-mix(in xyz, red, blue)",
  "transition: background-color color-mix(in xyz, red, blue)",
  "animation: background-color color-mix(in display-p3, red, blue)",
  "transition: background-color color-mix(in display-p3, red, blue)",
  "transition: background-color color-mix(in srgb, red 30%, blue 20%)",
  "transition: background-color lch(120% 200 30)",
  "transition: background-color color(srgb 2 -1 0.5)",
  "transition: background-color color(display-p3 2 -1 0.5)",
  "transition: background-color lab(50% 100% -100%)",
  "transition: background-color color(srgb 1 0 0 / 150%)",
  "transition: outline-color from the default",
  "transition: box-shadow with a modern colour",
  "transition: fill in a modern colour",
  "transition: stroke in a modern colour",
])

describeNative("What getKeyframes() and getTiming() read, against Chrome", () => {
  const groups: [string, (scenario: EffectCase) => boolean][] = [
    ["a div", (scenario) => scenario.tag === "div"],
    ["a shape of an svg", (scenario) => scenario.tag !== "div" && scenario.tag !== "svg"],
    ["an svg", (scenario) => scenario.tag === "svg"],
  ]
  /**
   * Chrome converts a colour between spaces with float matrices of its own, so what it writes for a colour it had to
   * convert (a `lab()` in a transition, which holds colours in Oklab, a `color-mix()`) differs from ours in the last
   * digits. The numbers of such a colour are compared to 2e-4 plus 3e-4 of their size; everything else about it, and every other value, is
   * compared as written.
   */
  const closeEnough = (got: unknown, want: unknown): boolean => {
    if (typeof got === "string" && typeof want === "string") {
      if (!/\b(oklab|oklch|lab|lch|color)\(/.test(want)) return got === want
      const shape = (text: string) => text.replace(/-?\d*\.?\d+(?:e-?\d+)?/g, "#")
      const numbers = (text: string) => (text.match(/-?\d*\.?\d+(?:e-?\d+)?/g) ?? []).map(Number)
      if (shape(got) !== shape(want)) return false
      const [a, b] = [numbers(got), numbers(want)]
      return a.every((number, index) => Math.abs(number - b[index]) <= 2e-4 + 3e-4 * Math.abs(b[index]))
    }
    if (Array.isArray(got) && Array.isArray(want)) {
      return got.length === want.length && got.every((member, index) => closeEnough(member, want[index]))
    }
    if (got && want && typeof got === "object" && typeof want === "object") {
      const [a, b] = [got as Record<string, unknown>, want as Record<string, unknown>]
      return Object.keys(a).length === Object.keys(b).length && Object.keys(b).every((key) => closeEnough(a[key], b[key]))
    }
    return got === want
  }
  /** The members of an animation's effect that differ, as the two read them. */
  const compact = (value: unknown): string => JSON.stringify(value).slice(0, 700)
  for (const [group, takes] of groups) {
    const cases = ORACLE.effectCases.filter(
      (scenario) =>
        takes(scenario) &&
        !REFUSED.has(scenario.name) &&
        !REFUSED.has(scenario.name.replace(/^transition:/, "animation:")) &&
        !EFFECT_SKIPS.some(([skipped]) => skipped.test(scenario.name))
    )
    for (let from = 0; from < cases.length; from += 100) {
      const chunk = cases.slice(from, from + 100)
      it(`${group}: ${chunk[0].name} to ${chunk[chunk.length - 1].name}`, { timeout: 300_000 }, () => {
        const { render, renderer, unmount } = createTestRoot()
        renderer.clockPause()
        const failures: string[] = []
        const approximate: string[] = []
        const read = (call: () => unknown) => {
          try {
            return call()
          } catch (error) {
            return { threw: (error as Error).name }
          }
        }
        try {
          // One case at a time in one window: a style gpuix cannot parse spoils its own case and no other.
          for (const scenario of chunk) {
            const target: { current: PublicInstance | null } = { current: null }
            const element = (changed: boolean) => {
              const style = numeric({
                ...(scenario.tag === "div" ? ORACLE.base : {}),
                ...scenario.style,
                ...(changed ? scenario.change : {}),
              })
              const attrs = { ...scenario.attrs, ...(changed ? scenario.attrChange : {}) }
              const ref = (node: PublicInstance | null) => {
                target.current = node
              }
              if (scenario.tag === "div") return <div style={style} ref={ref} />
              if (scenario.tag === "svg") return createElement("svg", { ...attrs, style, ref })
              return (
                <svg width={200} height={100} viewBox="0 0 200 100">
                  {createElement(scenario.tag, { ...attrs, style, ref }, ...(scenario.tag === "text" ? ["hello"] : []))}
                </svg>
              )
            }
            try {
              render(element(false))
              renderer.clockFastForward(0)
              renderer.getElementBounds(target.current!.id)
              render(element(true))
              renderer.clockFastForward(0)
              renderer.dispatchNativeEvents()
              const animations = target.current!.getAnimations()
              const got = animations.map((animation) => ({
                kind: animation.constructor.name,
                name: nameOf(animation),
                keyframes: read(() => clean(animation.effect.getKeyframes())),
                timing: read(() => clean(animation.effect.getTiming())),
              }))
              if (JSON.stringify(got) !== JSON.stringify(scenario.animations)) {
                try {
                  expect(got).toEqual(scenario.animations)
                } catch {
                  if (closeEnough(got, scenario.animations)) approximate.push(scenario.name)
                  else failures.push(`${scenario.name}\n   want ${compact(scenario.animations)}\n   got  ${compact(got)}`)
                }
              }
              if (scenario.update && scenario.updated) {
                const updated = animations.map((animation) =>
                  read(() => {
                    animation.effect.updateTiming(revive(scenario.update!))
                    return clean(animation.effect.getTiming())
                  })
                )
                try {
                  expect(updated).toEqual(scenario.updated)
                } catch {
                  failures.push(`${scenario.name}: updateTiming()\n   want ${compact(scenario.updated)}\n   got  ${compact(updated)}`)
                }
              }
            } catch (error) {
              failures.push(`${scenario.name}: threw ${(error as Error).message}`)
            } finally {
              render(<></>)
            }
          }
        } finally {
          unmount()
        }
        expect(failures.length, `${failures.length} of ${chunk.length} differ:\n${failures.join("\n")}`).toBe(0)
        expect(approximate.filter((name) => !APPROXIMATE.has(name)), "cases that are only close").toEqual([])
      })
    }
  }
})

describeNative("What gpuix refuses to animate where Chrome does", () => {
  it("refuses, where the keyframes are defined, the values that only layout or a parser it has not got can give", () => {
    expect([...REFUSED.keys()]).toEqual([
  "animation: width to min-content",
  "animation: width to fit-content",
  "animation: min-height from auto",
  "animation: max-width from none",
  "animation: column-gap from normal",
  "animation: border-left-width keywords",
  "animation: font-size keywords",
  "animation: line-height in px",
  "animation: line-height in percent",
  "animation: line-height from normal",
  "animation: z-index from auto",
  "animation: aspect-ratio as a ratio",
  "animation: aspect-ratio from auto",
  "animation: aspect-ratio with auto and a ratio",
  "animation: opacity in percent",
  "animation: display",
  "animation: position",
  "animation: overflow-x",
  "animation: overflow-y",
  "animation: pointer-events",
  "animation: table-layout",
  "animation: border-collapse",
  "animation: caption-side",
  "animation: empty-cells",
  "animation: flex-direction",
  "animation: flex-wrap",
  "animation: align-items",
  "animation: align-self",
  "animation: align-content",
  "animation: justify-content",
  "animation: justify-items",
  "animation: justify-self",
  "animation: display in keyframes",
  "animation: overflow in keyframes",
  "animation: pointer-events in keyframes",
  "animation: outline",
  "animation: border",
  "animation: border-top",
  "animation: background",
  "animation: text-decoration",
  "animation: overflow",
  "animation: place-items",
  "animation: circle r in em",
  "animation: text letter-spacing from normal",
  "animation: text letter-spacing in em",
  "keyframe easing linear(0.5)",
])
  })
})
