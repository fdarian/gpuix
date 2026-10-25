import type {
  ElementWebAnimationInfo,
  EventPayload,
  WebAnimationEffectInfo,
  WebAnimationInfo,
  WebAnimationKeyframeInfo,
  WebAnimationTimingUpdate,
} from "../index.js"
import { flushPendingMutations } from "./bounds.js"
import { EVENT_PHASE_AT_TARGET, EVENT_PHASE_NONE, reportUncaught, type DomEvent } from "./events/dispatch.js"
import {
  elementOf,
  GpuixEventTarget,
  setDocumentAnimations,
  type AddEventListenerOptions,
  type WindowEventListener,
} from "./events/window-target.js"
import type { NativeRenderer } from "./host.js"

export type AnimationPlayState = "idle" | "running" | "paused" | "finished"

/** The renderer methods the `Animation` objects are built on. */
export type WebAnimationHost = Required<
  Pick<
    NativeRenderer,
    | "getAnimations"
    | "getSubtreeAnimations"
    | "getDocumentAnimations"
    | "getAnimation"
    | "pauseAnimation"
    | "playAnimation"
    | "cancelAnimation"
    | "finishAnimation"
    | "reverseAnimation"
    | "setAnimationCurrentTime"
    | "setAnimationPlaybackRate"
    | "updateAnimationPlaybackRate"
    | "setAnimationStartTime"
    | "getAnimationTimelineTime"
    | "releaseAnimation"
    | "getAnimationEffect"
    | "getAnimationKeyframes"
    | "updateAnimationTiming"
  >
>

const HOST_METHODS = [
  "getAnimations",
  "getSubtreeAnimations",
  "getDocumentAnimations",
  "getAnimation",
  "pauseAnimation",
  "playAnimation",
  "cancelAnimation",
  "finishAnimation",
  "reverseAnimation",
  "setAnimationCurrentTime",
  "setAnimationPlaybackRate",
  "updateAnimationPlaybackRate",
  "setAnimationStartTime",
  "getAnimationTimelineTime",
  "releaseAnimation",
  "getAnimationEffect",
  "getAnimationKeyframes",
  "updateAnimationTiming",
] as const

const FINISH_EVENT = "webanimationfinish"
const CANCEL_EVENT = "webanimationcancel"
const READY_EVENT = "webanimationready"

/** What native says when a command refuses, before it is a `DOMException`. */
const INVALID_STATE_PREFIX = "InvalidStateError: "
const TYPE_ERROR_PREFIX = "TypeError: "

interface Deferred {
  readonly promise: Promise<Animation>
  resolve(animation: Animation): void
  reject(error: unknown): void
  settled: "pending" | "resolved" | "rejected"
}

function deferred(): Deferred {
  let settle!: Pick<Deferred, "resolve" | "reject">
  const promise = new Promise<Animation>((resolve, reject) => {
    settle = { resolve, reject }
  })
  // A cancel rejects it whether or not anyone is listening, and that is not an error to report.
  promise.catch(() => {})
  const result: Deferred = {
    promise,
    settled: "pending",
    resolve(animation) {
      result.settled = "resolved"
      settle.resolve(animation)
    },
    reject(error) {
      result.settled = "rejected"
      settle.reject(error)
    },
  }
  return result
}

function resolved(animation: Animation): Deferred {
  const result = deferred()
  result.resolve(animation)
  return result
}

/**
 * Where an animation stands. `live`: native has the run: it answers for it, idle
 * after `cancel()`, or from what it showed when it went between the frame that
 * ended it and the event reaching JS. `gone`: the style stopped naming it, a new
 * transition replaced it or its element went; it is idle for good.
 */
type Mode = "live" | "gone"

interface State {
  readonly host: WebAnimationHost
  readonly registry: Registry
  readonly timeline: DocumentTimeline
  readonly id: number
  readonly elementId: number
  mode: Mode
  /** What native last said, kept for what outlives the run: the rate an idle animation still reports. */
  last: WebAnimationInfo
  /** The timing and the keyframes of the effect as native last said, which outlive the run. */
  effect: Snapshot<WebAnimationEffectInfo>
  keyframes: Snapshot<WebAnimationKeyframeInfo[]>
  readonly keyframeEffect: KeyframeEffect
  finished: Deferred
  ready: Deferred
  /** `cancel()` calls whose `cancel` event native has not delivered yet. */
  cancelEvents: number
  /** `finished` was renewed because the animation went idle, which its cancel event then finds done. */
  renewedForCancel: boolean
  handlers: Map<string, EventHandler | null>
}

type EventHandler = ((event: AnimationPlaybackEvent) => void) | null

const states = new WeakMap<Animation, State>()

function stateOf(animation: Animation): State {
  const state = states.get(animation)
  if (!state) throw new TypeError("Illegal invocation")
  return state
}

/** What `getAnimations()` makes an `Animation` from. Not for apps. */
export interface AnimationInit {
  host: WebAnimationHost
  registry: Registry
  timeline: DocumentTimeline
  /** The element the run is of, which the effect has for its `target`. */
  target: object
  elementId: number
  info: WebAnimationInfo
}

function invalidState(message: string): DOMException {
  return new DOMException(message, "InvalidStateError")
}

/**
 * The `AnimationPlaybackEvent` of an animation's `finish` and `cancel`: the time the animation
 * was at, and the time of the timeline, when it happened. `null` for a cancelled animation's
 * `currentTime`.
 */
export interface AnimationPlaybackEvent {
  readonly type: "finish" | "cancel"
  readonly target: Animation
  currentTarget: Animation | null
  readonly currentTime: number | null
  readonly timelineTime: number | null
  readonly eventPhase: number
  readonly bubbles: false
  readonly cancelable: false
  readonly defaultPrevented: false
  readonly timeStamp: number
  composedPath(): Animation[]
  stopPropagation(): void
  stopImmediatePropagation(): void
  preventDefault(): void
}

/**
 * The timeline of every animation of a renderer: its time is the clock of the renderer, in
 * milliseconds, at the last frame. `startTime` and the `timelineTime` of the events are on it.
 * A renderer is the document of its animations, so there is one for each, and no
 * `document.timeline`: the `document` of GPUIX spans every window.
 */
export class DocumentTimeline {
  readonly #host: WebAnimationHost

  /** Not for apps: the timeline of an animation is `animation.timeline`. */
  constructor(host: WebAnimationHost) {
    this.#host = host
  }

  get currentTime(): number {
    return this.#host.getAnimationTimelineTime()
  }
}

/** `readLive` can find the run gone, which the earlier checks of the mode cannot see. */
function isGone(state: State): boolean {
  return state.mode === "gone"
}

function readLive(state: State): WebAnimationInfo {
  try {
    state.last = state.host.getAnimation(state.id)
  } catch (error) {
    if (!isUnknownAnimation(error)) throw error
    // The idle run of a `cancel()` that the style stopped naming: native dropped it without a word.
    state.mode = "gone"
    state.last = { ...state.last, playState: "idle", pending: false, startTime: undefined }
  }
  return state.last
}

/** What native refused with, as the DOM would. */
function refusal(error: unknown): unknown {
  if (error instanceof Error && error.message.startsWith(INVALID_STATE_PREFIX)) {
    return invalidState(error.message.slice(INVALID_STATE_PREFIX.length))
  }
  if (error instanceof Error && error.message.startsWith(TYPE_ERROR_PREFIX)) {
    return new TypeError(error.message.slice(TYPE_ERROR_PREFIX.length))
  }
  return error
}

/**
 * Sends a command to native, then finds what it did: a play or pause that was
 * waiting and is not (`ready`), a finish, and the end of one. A run native no
 * longer has is one that was cancelled since the frame JS last heard of, as far
 * as JS is concerned.
 */
function command(animation: Animation, action: string, send: (state: State) => void): void {
  const state = stateOf(animation)
  const cancelled = () => invalidState(`Cannot ${action} an animation that was cancelled`)
  if (state.mode === "gone") throw cancelled()
  let before: WebAnimationInfo
  try {
    before = readLive(state)
    if (isGone(state)) throw cancelled()
    send(state)
  } catch (error) {
    throw isUnknownAnimation(error) ? cancelled() : refusal(error)
  }
  settle(animation, state, before)
}

/** What the state native is in says about the promises, after a command. */
function settle(animation: Animation, state: State, before: WebAnimationInfo): void {
  if (state.mode !== "live") return
  const info = readLive(state)
  state.renewedForCancel = false
  if (before.pending && !info.pending) {
    if (state.ready.settled === "pending") state.ready.resolve(animation)
  } else if (info.pending && !before.pending) {
    state.ready = deferred()
  }
  if (info.playState === "finished") {
    if (state.finished.settled === "pending") state.finished.resolve(animation)
  } else if (state.finished.settled === "resolved") {
    state.finished = deferred()
  }
}

/**
 * What native said of a run's effect, or why it could not: a value Chrome writes as a computed
 * value in a form that is not known yet (a transform, a `linear()` easing) is an error for the
 * one who asks, not for the one who lists the animations.
 */
type Snapshot<T> = { readonly value: T } | { readonly error: DOMException }

function snapshot<T>(read: () => T): Snapshot<T> {
  try {
    return { value: read() }
  } catch (error) {
    if (isUnknownAnimation(error)) throw error
    return { error: new DOMException(error instanceof Error ? error.message : String(error), "NotSupportedError") }
  }
}

function unwrap<T>(snapshot: Snapshot<T>): T {
  if ("error" in snapshot) throw snapshot.error
  return snapshot.value
}

export type FillMode = "none" | "forwards" | "backwards" | "both" | "auto"
export type PlaybackDirection = "normal" | "reverse" | "alternate" | "alternate-reverse"

/** The `EffectTiming` of the DOM. */
export interface EffectTiming {
  delay: number
  endDelay: number
  fill: FillMode
  iterationStart: number
  iterations: number
  duration: number | "auto"
  direction: PlaybackDirection
  easing: string
}

export type OptionalEffectTiming = Partial<EffectTiming>

/** The `ComputedEffectTiming` of the DOM: milliseconds, `null` where the effect has none. */
export interface ComputedEffectTiming extends Omit<EffectTiming, "duration"> {
  duration: number
  endTime: number
  activeDuration: number
  localTime: number | null
  progress: number | null
  currentIteration: number | null
}

/** A keyframe as `getKeyframes()` returns it: the other members are the properties, as computed values. */
export interface ComputedKeyframe {
  /** `null` for a keyframe of an animation of the motion engine that was given no offset. */
  offset: number | null
  computedOffset: number
  easing: string
  composite: "auto" | "replace" | "add" | "accumulate"
  [property: string]: string | number | null | undefined
}

/** What a run's animation makes its effect from. */
interface EffectInit {
  animation: Animation
  target: object
}

const effects = new WeakMap<KeyframeEffect, EffectInit>()

function effectOf(effect: KeyframeEffect): EffectInit {
  const init = effects.get(effect)
  if (!init) throw new TypeError("Illegal invocation")
  return init
}

/** What native says of the effect now; what it said last once it no longer has the run. */
function readEffect(state: State): WebAnimationEffectInfo {
  let read: Snapshot<WebAnimationEffectInfo> = state.effect
  if (state.mode === "live") {
    try {
      read = state.effect = snapshot(() => state.host.getAnimationEffect(state.id))
    } catch (error) {
      if (!isUnknownAnimation(error)) throw error
    }
  }
  const info = unwrap(read)
  // A run native let go of is idle, whatever it showed when it went.
  if (state.mode === "gone" || readLive(state).playState === "idle") {
    return { ...info, localTime: undefined, progress: undefined, currentIteration: undefined }
  }
  return info
}

/** A member the DOM takes as a finite number, which `updateTiming()` refuses to leave anything else. */
function finite(name: string, value: unknown): number {
  const number = Number(value)
  if (!Number.isFinite(number)) throw new TypeError(`Failed to execute 'updateTiming' on 'AnimationEffect': ${name} must be a finite number`)
  return number
}

function oneOf<T extends string>(name: string, value: unknown, names: readonly T[]): T {
  if (typeof value !== "string" || !names.includes(value as T)) {
    throw new TypeError(`Failed to execute 'updateTiming' on 'AnimationEffect': ${String(value)} is not a valid ${name}`)
  }
  return value as T
}

/** The `updateTiming()` argument as native takes it, or the `TypeError` the DOM throws for a member that is wrong. */
function timingUpdate(timing: OptionalEffectTiming | undefined): WebAnimationTimingUpdate {
  if (timing === undefined || timing === null) return {}
  if (typeof timing !== "object") {
    throw new TypeError("Failed to execute 'updateTiming' on 'AnimationEffect': the timing is not an object")
  }
  const update: WebAnimationTimingUpdate = {}
  if (timing.delay !== undefined) update.delay = finite("delay", timing.delay)
  if (timing.endDelay !== undefined) update.endDelay = finite("endDelay", timing.endDelay)
  if (timing.iterationStart !== undefined) {
    update.iterationStart = finite("iterationStart", timing.iterationStart)
    if (update.iterationStart < 0) throw new TypeError("Failed to execute 'updateTiming' on 'AnimationEffect': iterationStart must not be negative")
  }
  if (timing.iterations !== undefined) {
    const iterations = Number(timing.iterations)
    if (Number.isNaN(iterations) || iterations < 0) {
      throw new TypeError("Failed to execute 'updateTiming' on 'AnimationEffect': iterations must not be negative")
    }
    update.iterations = iterations
  }
  if (timing.duration !== undefined) {
    if (typeof timing.duration === "string") {
      if (timing.duration !== "auto") throw new TypeError(`Failed to execute 'updateTiming' on 'AnimationEffect': ${timing.duration} is not a valid duration`)
      update.duration = "auto"
    } else {
      const duration = Number(timing.duration)
      if (Number.isNaN(duration) || duration < 0) {
        throw new TypeError("Failed to execute 'updateTiming' on 'AnimationEffect': duration must not be negative")
      }
      update.duration = duration
    }
  }
  // `auto` is no fill: an effect has none of its own.
  if (timing.fill !== undefined) {
    const fill = oneOf("fill", timing.fill, ["none", "forwards", "backwards", "both", "auto"] as const)
    update.fill = fill === "auto" ? "none" : fill
  }
  if (timing.direction !== undefined) {
    update.direction = oneOf("direction", timing.direction, ["normal", "reverse", "alternate", "alternate-reverse"] as const)
  }
  if (timing.easing !== undefined) update.easing = String(timing.easing)
  return update
}

const EASING_KEYWORDS = ["linear", "ease", "ease-in", "ease-out", "ease-in-out", "step-start", "step-end"]

/** `updateTiming()` of a run native no longer has: it changes what the effect reports from now on. */
function updateGone(state: State, update: WebAnimationTimingUpdate): void {
  const info = unwrap(state.effect)
  if (update.easing !== undefined && !EASING_KEYWORDS.includes(update.easing.trim())) {
    throw new DOMException(
      "The easing of an animation that was cancelled for good can be set to a keyword only",
      "NotSupportedError"
    )
  }
  const next: WebAnimationEffectInfo = { ...info }
  if (update.delay !== undefined) next.delay = update.delay
  if (update.endDelay !== undefined) next.endDelay = update.endDelay
  if (update.fill !== undefined) next.fill = update.fill
  if (update.iterationStart !== undefined) next.iterationStart = update.iterationStart
  if (update.iterations !== undefined) next.iterations = update.iterations
  if (update.direction !== undefined) next.direction = update.direction
  if (update.duration !== undefined) next.duration = update.duration === "auto" ? undefined : (update.duration as number)
  if (update.easing !== undefined) next.easing = update.easing.trim() === "step-start" ? "steps(1, start)" : update.easing.trim() === "step-end" ? "steps(1)" : update.easing.trim()
  const duration = next.duration ?? 0
  const active = duration === 0 || next.iterations === 0 ? 0 : duration * next.iterations
  next.activeDuration = active
  next.endTime = Math.max(next.delay + active + next.endDelay, 0)
  state.effect = { value: next }
}

/**
 * The effect of a CSS animation or transition, as the DOM's `KeyframeEffect`: the keyframes
 * and the timing it plays by. `animation.effect` is the same object every time. It cannot be
 * made, and there is no `setKeyframes()`, `composite` or `pseudoElement`.
 */
export class KeyframeEffect {
  /** Not for apps: the effect of an animation is `animation.effect`. */
  constructor(init: EffectInit) {
    effects.set(this, init)
  }

  /** The element the run is of, also after the element is gone. */
  get target(): object | null {
    return effectOf(this).target
  }

  /**
   * The keyframes with computed values: those of the `@keyframes` rule with the ones Chrome adds
   * for a property that has none at 0% or 100% (`composite: "replace"`, at the value the element
   * has without the animation), or the start and the end of a transition. As of the last frame.
   * Throws a `NotSupportedError` for a value whose computed text is not known yet.
   */
  getKeyframes(): ComputedKeyframe[] {
    const state = stateOf(effectOf(this).animation)
    if (state.mode === "live") {
      try {
        state.keyframes = snapshot(() => state.host.getAnimationKeyframes(state.id))
      } catch (error) {
        if (!isUnknownAnimation(error)) throw error
      }
    }
    return unwrap(state.keyframes).map((frame) => {
      const keyframe: ComputedKeyframe = {
        offset: frame.offset ?? null,
        easing: frame.easing,
        composite: frame.composite as ComputedKeyframe["composite"],
        computedOffset: frame.computedOffset,
      }
      for (const value of frame.values) keyframe[value.property] = value.value
      return keyframe
    })
  }

  /**
   * The timing in the units of the DOM: milliseconds, `iterations` `Infinity` for an animation
   * that repeats for ever. For a CSS animation the `easing` is `"linear"` and its
   * `animation-timing-function` is the easing of its keyframes; for a transition it is the
   * `transition-timing-function`.
   */
  getTiming(): EffectTiming {
    const info = readEffect(stateOf(effectOf(this).animation))
    return {
      delay: info.delay,
      endDelay: info.endDelay,
      fill: info.fill as FillMode,
      iterationStart: info.iterationStart,
      iterations: info.iterations,
      duration: info.duration ?? "auto",
      direction: info.direction as PlaybackDirection,
      easing: info.easing,
    }
  }

  /**
   * The timing with what it works out to at the current time of the animation: `localTime`,
   * `progress` and `currentIteration` are `null` for an idle animation and `progress` and
   * `currentIteration` outside the active phase unless a fill holds the effect.
   */
  getComputedTiming(): ComputedEffectTiming {
    const info = readEffect(stateOf(effectOf(this).animation))
    return {
      delay: info.delay,
      endDelay: info.endDelay,
      fill: info.fill as FillMode,
      iterationStart: info.iterationStart,
      iterations: info.iterations,
      duration: info.duration ?? 0,
      direction: info.direction as PlaybackDirection,
      easing: info.easing,
      endTime: info.endTime,
      activeDuration: info.activeDuration,
      localTime: info.localTime ?? null,
      progress: info.progress ?? null,
      currentIteration: info.currentIteration ?? null,
    }
  }

  /**
   * Sets the timing members given, for this animation only. What a CSS animation is set to here
   * stays against a later change of its `animation-*` style, which still moves the members that
   * were not set; a running transition never follows a change of its `transition-*` style. A
   * member that is wrong throws a `TypeError` and changes nothing. An effect that ends later than
   * the animation has gone makes a finished animation run again.
   */
  updateTiming(timing?: OptionalEffectTiming): void {
    const owner = effectOf(this)
    const update = timingUpdate(timing)
    const state = stateOf(owner.animation)
    if (state.mode === "live") {
      try {
        command(owner.animation, "update the timing of", (live) => live.host.updateAnimationTiming(live.id, update))
        // What the animation keeps once native lets the run go.
        try {
          state.effect = snapshot(() => state.host.getAnimationEffect(state.id))
        } catch (error) {
          if (!isUnknownAnimation(error)) throw error
        }
        return
      } catch (error) {
        // Cancelled since the frame JS last heard of: the effect is what it was.
        if (!(error instanceof DOMException && error.name === "InvalidStateError")) throw error
      }
    }
    updateGone(state, update)
  }
}

/**
 * A CSS animation or transition of an element, or an animation of a `motion` element that Motion would hand to the
 * browser's Web Animations (an `opacity` or `backgroundColor` animation, a colour in `oklch()`, ...; a plain
 * `Animation`, as the one `element.animate()` makes), as `element.getAnimations()`
 * returns it: the Web Animations `Animation` of the DOM, for what the CSS
 * engine runs. `getAnimations()` gives the same object for the same run every
 * time. `playState`, `currentTime`, `playbackRate`, `startTime` and `pending`
 * are read when asked, as of the last frame, which is also the time of
 * `timeline`. The promises `ready` and `finished` and the `finish` and `cancel`
 * events follow Chrome's order.
 *
 * Not built: `replaceState`, `persist()`, `commitStyles()`,
 * `onremove`, and setting `effect`. An animation of the motion engine can be read and not controlled yet.
 */
export class Animation extends GpuixEventTarget {
  /** Empty for what `getAnimations()` lists: the animations of CSS and of Motion have no id. */
  id = ""

  constructor(init: AnimationInit) {
    super()
    const state: State = {
      host: init.host,
      registry: init.registry,
      timeline: init.timeline,
      id: init.info.id,
      elementId: init.elementId,
      mode: "live",
      last: init.info,
      effect: snapshot(() => init.host.getAnimationEffect(init.info.id)),
      keyframes: snapshot(() => init.host.getAnimationKeyframes(init.info.id)),
      keyframeEffect: new KeyframeEffect({ animation: this, target: init.target }),
      finished: deferred(),
      ready: deferred(),
      cancelEvents: 0,
      renewedForCancel: false,
      handlers: new Map(),
    }
    states.set(this, state)
    // A finished animation that waits for its first frame is told of its finish by the first command or
    // event, not when it is made.
    if (init.info.playState === "finished" && !init.info.pending) state.finished.resolve(this)
    if (!init.info.pending) state.ready.resolve(this)
    // An animation made finished (an effect with no length) has nothing to wait a frame for: Chrome
    // runs its play right after the script that first read it, which a seek to where it is does here.
    if (init.info.pending && init.info.playState === "finished") queueMicrotask(() => startFinished(this))
  }

  /** The timeline all the animations of a renderer share. */
  get timeline(): DocumentTimeline {
    return stateOf(this).timeline
  }

  /** The keyframes and the timing the run plays by. The same object every time. */
  get effect(): KeyframeEffect {
    return stateOf(this).keyframeEffect
  }

  /**
   * Milliseconds from the start, the delay inside it: 0 at the start whatever
   * the delay, and the end of the effect when it ends. `null` once the
   * animation is cancelled. Setting it seeks at once and the next frame paints
   * it; `null` cannot be set.
   */
  get currentTime(): number | null {
    const state = stateOf(this)
    switch (state.mode) {
      case "live": {
        const info = readLive(state)
        return info.playState === "idle" ? null : info.currentTime
      }
      case "gone":
        return null
    }
  }

  set currentTime(value: number | null) {
    if (typeof value !== "number" || !Number.isFinite(value)) {
      throw new TypeError(`Failed to set currentTime: ${String(value)} is not a finite number`)
    }
    command(this, "set currentTime of", (state) =>
      state.host.setAnimationCurrentTime(state.id, value)
    )
  }

  /**
   * When the animation started on `timeline`, in milliseconds: `currentTime` is
   * `(timeline.currentTime - startTime) * playbackRate`. `null` while it is
   * pending, paused or idle. Setting it moves the animation to that time; `null`
   * holds the time it is at.
   */
  get startTime(): number | null {
    const state = stateOf(this)
    switch (state.mode) {
      case "live":
        return readLive(state).startTime ?? null
      case "gone":
        return null
    }
  }

  set startTime(value: number | null) {
    if (value !== null && (typeof value !== "number" || !Number.isFinite(value))) {
      throw new TypeError(`Failed to set startTime: ${String(value)} is not a finite number or null`)
    }
    command(this, "set startTime of", (state) => state.host.setAnimationStartTime(state.id, value))
  }

  /** The speed of time, negative to run back. Changing it keeps `currentTime`. */
  get playbackRate(): number {
    const state = stateOf(this)
    return state.mode === "live" ? readLive(state).playbackRate : state.last.playbackRate
  }

  set playbackRate(value: number) {
    if (typeof value !== "number" || !Number.isFinite(value)) {
      throw new TypeError(`Failed to set playbackRate: ${String(value)} is not a finite number`)
    }
    command(this, "set playbackRate of", (state) =>
      state.host.setAnimationPlaybackRate(state.id, value)
    )
  }

  get playState(): AnimationPlayState {
    const state = stateOf(this)
    switch (state.mode) {
      case "live":
        return readLive(state).playState as AnimationPlayState
      case "gone":
        return "idle"
    }
  }

  /** Whether a play or pause waits for the next frame to run. */
  get pending(): boolean {
    const state = stateOf(this)
    return state.mode === "live" && readLive(state).pending
  }

  /**
   * Resolves with the animation when the play or pause that is pending has
   * run, at the next frame; already resolved when none is. A `pause()` of a
   * running animation, a `play()` of a paused, finished or cancelled one and
   * the others that wait for a frame make a new one, unless one is already
   * waiting. A cancel rejects the one that is waiting with an `AbortError`.
   */
  get ready(): Promise<this> {
    return stateOf(this).ready.promise as Promise<this>
  }

  /**
   * Resolves with the animation when it finishes; rejects with an
   * `AbortError` when it is cancelled (the style stopped naming it, a new
   * transition replaced it, its element went). A new promise replaces a
   * resolved one when the animation leaves `finished` again, and a cancel
   * leaves a new pending one.
   */
  get finished(): Promise<this> {
    const state = stateOf(this)
    if (state.mode === "live" && state.finished.settled === "resolved") {
      const playState = readLive(state).playState
      if (playState !== "finished") {
        state.finished = deferred()
        state.renewedForCancel = playState === "idle"
      }
    }
    return state.finished.promise as Promise<this>
  }

  /** Runs when the animation finishes, in the order it was first set among the listeners. */
  get onfinish(): EventHandler {
    return this.#handler("finish")
  }

  set onfinish(handler: EventHandler) {
    this.#setHandler("finish", handler)
  }

  /** Runs when the animation is cancelled, in the order it was first set among the listeners. */
  get oncancel(): EventHandler {
    return this.#handler("cancel")
  }

  set oncancel(handler: EventHandler) {
    this.#setHandler("cancel", handler)
  }

  #handler(type: string): EventHandler {
    return stateOf(this).handlers.get(type) ?? null
  }

  #setHandler(type: string, handler: EventHandler): void {
    const state = stateOf(this)
    const registered = state.handlers.has(type)
    state.handlers.set(type, typeof handler === "function" ? handler : null)
    // The handler is a listener from the moment it is first set, as in the DOM, so the
    // ones added after it come after it.
    if (!registered) {
      this.addEventListener(type, (event) => state.handlers.get(type)?.call(this, event as unknown as AnimationPlaybackEvent))
    }
  }

  /** Holds the current time at once. From here `animation-play-state` no longer decides. */
  pause(): void {
    command(this, "pause", (state) => state.host.pauseAnimation(state.id))
  }

  /**
   * Continues from the held time at the next frame. A finished animation
   * starts over: from 0, or from the end when the rate is negative; so does a
   * cancelled one. From here `animation-play-state` no longer decides.
   */
  play(): void {
    command(this, "play", (state) => state.host.playAnimation(state.id))
  }

  /**
   * Stops the animation: idle, with no current time, out of `getAnimations()`,
   * its element as if it had none. A style change does not start it again;
   * `play()` does, from the start. Its `finished` promise rejects with an
   * `AbortError` and `ready` too if it was waiting; the `cancel` event comes
   * with the next frame.
   */
  cancel(): void {
    const state = stateOf(this)
    if (state.mode === "gone") return
    const before = readLive(state)
    if (before.playState === "idle") return
    try {
      state.host.cancelAnimation(state.id)
    } catch (error) {
      // Cancelled since the frame JS last heard of: its cancel event is on its way.
      if (isUnknownAnimation(error)) return
      throw error
    }
    state.cancelEvents++
    abort(state, before.pending)
    readLive(state)
  }

  /**
   * Goes to the end the rate runs toward at once, the end of the effect or 0.
   * Throws an `InvalidStateError` at a rate of 0 and for an animation that
   * never ends running forward.
   */
  finish(): void {
    command(this, "finish", (state) => state.host.finishAnimation(state.id))
  }

  /**
   * Plays the animation the other way: the rate flips when the next frame
   * comes, and an animation at the end it was running from goes to the other.
   * Throws an `InvalidStateError` for an animation that never ends, from its start.
   */
  reverse(): void {
    command(this, "reverse", (state) => state.host.reverseAnimation(state.id))
  }

  /**
   * Sets the rate the way `playbackRate` does, but keeps the time continuous across the change:
   * a running animation takes the new rate at the next frame, a paused, finished or cancelled
   * one at once.
   */
  updatePlaybackRate(rate: number): void {
    if (typeof rate !== "number" || !Number.isFinite(rate)) {
      throw new TypeError(`Failed to execute updatePlaybackRate: ${String(rate)} is not a finite number`)
    }
    command(this, "update the playbackRate of", (state) => state.host.updateAnimationPlaybackRate(state.id, rate))
  }
}

// The listeners of `finish` and `cancel` are given the playback event.
export interface Animation {
  addEventListener(
    type: "finish" | "cancel",
    listener: (event: AnimationPlaybackEvent) => void,
    options?: boolean | AddEventListenerOptions
  ): void
  addEventListener(type: string, listener: WindowEventListener, options?: boolean | AddEventListenerOptions): void
  removeEventListener(
    type: "finish" | "cancel",
    listener: (event: AnimationPlaybackEvent) => void,
    options?: boolean | Pick<AddEventListenerOptions, "capture">
  ): void
  removeEventListener(
    type: string,
    listener: WindowEventListener,
    options?: boolean | Pick<AddEventListenerOptions, "capture">
  ): void
}

/** A CSS animation: the run of an `animation-name`. */
export class CSSAnimation extends Animation {
  readonly animationName: string

  constructor(init: AnimationInit) {
    super(init)
    if (init.info.animationName === undefined) {
      throw new TypeError(`Animation ${init.info.id} is not a CSS animation`)
    }
    this.animationName = init.info.animationName
  }
}

/** A CSS transition: the run of one property. */
export class CSSTransition extends Animation {
  /** The CSS name of the property, `background-color`. */
  readonly transitionProperty: string

  constructor(init: AnimationInit) {
    super(init)
    if (init.info.transitionProperty === undefined) {
      throw new TypeError(`Animation ${init.info.id} is not a CSS transition`)
    }
    this.transitionProperty = init.info.transitionProperty
  }
}

/** The animations a renderer has handed out, by native id and by element. */
interface Registry {
  readonly byId: Map<number, Animation>
  readonly byElement: Map<number, Set<Animation>>
  /** Made with the first animation listed. */
  timeline?: DocumentTimeline
}

// Shared through `globalThis` like the renderer states: the JSX runtime can load
// a second copy of this module, and a copy that lists animations must be heard
// by the copy that routes the events.
const REGISTRIES_KEY = Symbol.for("@gpuix/native/web-animations")

function registryOf(renderer: object): Registry {
  let all = Reflect.get(globalThis, REGISTRIES_KEY) as WeakMap<object, Registry> | undefined
  if (!all) {
    all = new WeakMap()
    Reflect.set(globalThis, REGISTRIES_KEY, all)
  }
  let registry = all.get(renderer)
  if (!registry) {
    registry = { byId: new Map(), byElement: new Map() }
    all.set(renderer, registry)
  }
  return registry
}

function hostOf(renderer: NativeRenderer): WebAnimationHost {
  for (const method of HOST_METHODS) {
    if (!renderer[method]) throw new Error(`This renderer does not support ${method}`)
  }
  return renderer as WebAnimationHost
}

/** The error `getAnimation` throws for a run native no longer has. */
function isUnknownAnimation(error: unknown): boolean {
  return error instanceof Error && error.message.includes("No running CSS animation or transition has the id")
}

function unregister(registry: Registry, state: State, animation: Animation): void {
  registry.byId.delete(state.id)
  const siblings = registry.byElement.get(state.elementId)
  siblings?.delete(animation)
  if (siblings?.size === 0) registry.byElement.delete(state.elementId)
}

function register(registry: Registry, elementId: number, id: number, animation: Animation): void {
  registry.byId.set(id, animation)
  const siblings = registry.byElement.get(elementId) ?? new Set()
  siblings.add(animation)
  registry.byElement.set(elementId, siblings)
}

/** The class of the animation of a run: the DOM makes an `Animation` for a script and a subclass for CSS. */
function animationOf(kind: string, init: AnimationInit): Animation {
  switch (kind) {
    case "transition":
      return new CSSTransition(init)
    case "script":
      return new Animation(init)
    default:
      return new CSSAnimation(init)
  }
}

function adopt(
  registry: Registry,
  host: WebAnimationHost,
  timeline: DocumentTimeline,
  target: object,
  elementId: number,
  info: WebAnimationInfo
): Animation {
  const known = registry.byId.get(info.id)
  if (known) return known
  const init = { host, registry, timeline, target, elementId, info }
  const animation = animationOf(info.kind, init)
  register(registry, elementId, info.id, animation)
  return animation
}

export interface GetAnimationsOptions {
  /** Also the animations of the elements below, in document order. */
  subtree?: boolean
}

/**
 * `element.getAnimations()`: the CSS transitions, then the CSS animations,
 * that are running, paused or in effect through a fill, in composite order.
 * With `subtree`, those of the element and everything below it: every
 * transition of the elements in document order, then every animation.
 * Pending mutations are sent first, so a style just set is seen.
 */
export function getElementAnimations(
  renderer: NativeRenderer,
  elementId: number,
  target: object,
  options?: GetAnimationsOptions
): Animation[] {
  const host = hostOf(renderer)
  flushPendingMutations(renderer)
  const registry = registryOf(renderer)
  const timeline = (registry.timeline ??= new DocumentTimeline(host))
  if (!options?.subtree) {
    return host.getAnimations(elementId).map((info) => adopt(registry, host, timeline, target, elementId, info))
  }
  return host
    .getSubtreeAnimations(elementId)
    .map((listed) => adoptListed(registry, host, timeline, renderer, listed, elementId, target))
}

/** `document.getAnimations()` of one renderer: the animations of its window, in document order. */
function getRendererAnimations(renderer: NativeRenderer): Animation[] {
  const host = hostOf(renderer)
  flushPendingMutations(renderer)
  const registry = registryOf(renderer)
  const timeline = (registry.timeline ??= new DocumentTimeline(host))
  return host
    .getDocumentAnimations()
    .map((listed) => adoptListed(registry, host, timeline, renderer, listed, undefined, undefined))
}

function adoptListed(
  registry: Registry,
  host: WebAnimationHost,
  timeline: DocumentTimeline,
  renderer: NativeRenderer,
  listed: ElementWebAnimationInfo,
  readOn: number | undefined,
  readOnTarget: object | undefined
): Animation {
  const known = registry.byId.get(listed.info.id)
  if (known) return known
  const target = listed.elementId === readOn ? readOnTarget : elementOf(renderer, listed.elementId)
  if (target === undefined) {
    throw new Error(`Element ${listed.elementId} has an animation but its root has no element for it`)
  }
  return adopt(registry, host, timeline, target, listed.elementId, listed.info)
}

setDocumentAnimations((renderer) => getRendererAnimations(renderer as NativeRenderer))

/** The cancel of an animation: its promises, as `cancel()` and a style that stops naming it leave them. */
function abort(state: State, wasPending: boolean): void {
  const error = () => new DOMException("The animation was cancelled", "AbortError")
  if (wasPending && state.ready.settled === "pending") {
    state.ready.reject(error())
    state.ready = deferred()
  }
  if (state.finished.settled === "pending") state.finished.reject(error())
  state.finished = deferred()
}

/** Runs the play of an animation that was made finished, which has no frame to wait for. */
function startFinished(animation: Animation): void {
  const state = stateOf(animation)
  if (state.mode !== "live") return
  try {
    if (!readLive(state).pending) return
    command(animation, "start", (live) => live.host.setAnimationCurrentTime(live.id, readLive(live).currentTime))
  } catch (error) {
    // Cancelled since: there is nothing left to start.
    if (!(error instanceof DOMException && error.name === "InvalidStateError")) throw error
  }
}

/** The animation went for good: nothing native has stays to be played again. */
function lose(animation: Animation): void {
  const state = stateOf(animation)
  state.mode = "gone"
  if (state.renewedForCancel) {
    state.renewedForCancel = false
    return
  }
  abort(state, state.last.pending)
}

function dispatchPlaybackEvent(
  animation: Animation,
  type: "finish" | "cancel",
  currentTime: number | null,
  timelineTime: number | null
): void {
  let stopped = false
  const event: AnimationPlaybackEvent = {
    type,
    target: animation,
    currentTarget: null,
    currentTime,
    timelineTime,
    eventPhase: EVENT_PHASE_NONE,
    bubbles: false,
    cancelable: false,
    defaultPrevented: false,
    timeStamp: performance.now(),
    composedPath: () => [animation],
    stopPropagation() {},
    stopImmediatePropagation() {
      stopped = true
    },
    preventDefault() {},
  }
  const errors: unknown[] = []
  animation.run(
    event as unknown as DomEvent,
    {
      propagationStopped: false,
      get immediatePropagationStopped() {
        return stopped
      },
    },
    false,
    EVENT_PHASE_AT_TARGET,
    (error) => errors.push(error)
  )
  event.currentTarget = null
  for (const error of errors) reportUncaught(error)
}

/** Whether the event is one of the internal ones `deliverWebAnimationEvent` takes. */
export function isWebAnimationEvent(eventType: string): boolean {
  return eventType === FINISH_EVENT || eventType === CANCEL_EVENT || eventType === READY_EVENT
}

/**
 * Takes the ready, finish or cancel of a CSS animation or transition for the
 * `Animation` object listed for it, then tells native a finish or cancel is
 * consumed, which stops answering for the run as it ended. Nothing else hears
 * these events: they are not DOM events. A run no one listed is released all
 * the same.
 */
export function deliverWebAnimationEvent(renderer: NativeRenderer, event: EventPayload): void {
  if (event.animationId === undefined) {
    throw new Error(`Event "${event.eventType}" arrived without its animationId`)
  }
  try {
    const registry = registryOf(renderer)
    const animation = registry.byId.get(event.animationId)
    if (!animation) return
    const state = stateOf(animation)
    const timelineTime = event.timelineTime ?? null
    switch (event.eventType) {
      case READY_EVENT:
        if (state.ready.settled === "pending") state.ready.resolve(animation)
        return
      case FINISH_EVENT:
        // A command that finished the animation resolved it then. This is the finish by itself.
        if (state.finished.settled === "pending" && readLive(state).playState === "finished") {
          state.finished.resolve(animation)
        }
        dispatchPlaybackEvent(animation, "finish", event.currentTime ?? null, timelineTime)
        return
      default:
        if (state.cancelEvents > 0) {
          state.cancelEvents--
        } else {
          lose(animation)
        }
        if (state.mode === "gone") unregister(registry, state, animation)
        dispatchPlaybackEvent(animation, "cancel", null, timelineTime)
    }
  } finally {
    if (event.eventType !== READY_EVENT) renderer.releaseAnimation?.(event.animationId)
  }
}

/**
 * A removed element's animations are cancelled, as the DOM does: native sends
 * nothing when an element goes, so the ids the mutation queue was told are
 * destroyed end here. The `cancel` event comes in a task, as the DOM's comes at
 * its next frame.
 */
export function cancelAnimationsOfElements(renderer: object, elementIds: readonly number[]): void {
  const registry = registryOf(renderer)
  for (const elementId of elementIds) {
    // Chrome cancels the last of an element's animations first.
    for (const animation of [...(registry.byElement.get(elementId) ?? [])].reverse()) {
      const state = stateOf(animation)
      const wasIdle = state.mode === "gone" || state.last.playState === "idle"
      lose(animation)
      unregister(registry, state, animation)
      if (wasIdle) continue
      const timelineTime = state.timeline.currentTime
      setTimeout(() => dispatchPlaybackEvent(animation, "cancel", null, timelineTime), 0)
    }
  }
}
