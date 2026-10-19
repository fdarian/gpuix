import type { EventPayload, WebAnimationInfo } from "../index.js"
import { flushPendingMutations } from "./bounds.js"
import type { NativeRenderer } from "./host.js"

export type AnimationPlayState = "idle" | "running" | "paused" | "finished"

/** The renderer methods the `Animation` objects are built on. */
export type WebAnimationHost = Required<
  Pick<
    NativeRenderer,
    | "getAnimations"
    | "getAnimation"
    | "pauseAnimation"
    | "playAnimation"
    | "setAnimationCurrentTime"
    | "setAnimationPlaybackRate"
  >
>

const HOST_METHODS = [
  "getAnimations",
  "getAnimation",
  "pauseAnimation",
  "playAnimation",
  "setAnimationCurrentTime",
  "setAnimationPlaybackRate",
] as const

const FINISH_EVENT = "webanimationfinish"
const CANCEL_EVENT = "webanimationcancel"

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

/**
 * Where an animation stands. `live`: native still has the run and answers for
 * it. `frozen`: a transition that finished and left its element, shown as it
 * ended. `idle`: cancelled.
 */
type Mode = "live" | "frozen" | "idle"

interface State {
  readonly host: WebAnimationHost
  readonly id: number
  readonly elementId: number
  mode: Mode
  /**
   * What native last said, kept for what outlives the run: a frozen
   * transition's end and the rate an idle animation still reports.
   */
  last: WebAnimationInfo
  finished: Deferred
}

const states = new WeakMap<Animation, State>()

function stateOf(animation: Animation): State {
  const state = states.get(animation)
  if (!state) throw new TypeError("Illegal invocation")
  return state
}

/** What `getAnimations()` makes an `Animation` from. Not for apps. */
export interface AnimationInit {
  host: WebAnimationHost
  elementId: number
  info: WebAnimationInfo
}

function invalidState(message: string): DOMException {
  return new DOMException(message, "InvalidStateError")
}

function readLive(state: State): WebAnimationInfo {
  state.last = state.host.getAnimation(state.id)
  return state.last
}

function requireLive(state: State, action: string): void {
  if (state.mode === "live") return
  throw invalidState(
    state.mode === "idle"
      ? `Cannot ${action} an animation that was cancelled`
      : `Cannot ${action} a transition that finished and left its element`
  )
}

/**
 * Reads the run after a command. A transition that the command carried to its
 * end is already gone natively, and its finish event is what freezes it:
 * `patch` is what the command changed, kept for that.
 */
function readAfterCommand(state: State, patch: Partial<WebAnimationInfo>): WebAnimationInfo | undefined {
  state.last = { ...state.last, ...patch }
  try {
    return readLive(state)
  } catch (error) {
    if (isUnknownAnimation(error)) return undefined
    throw error
  }
}

/**
 * Once the animation has left `finished` (a `play()` that rewound it, a seek),
 * `finished` is a new promise, as in the DOM.
 */
function renewFinished(state: State, patch: Partial<WebAnimationInfo> = {}): void {
  if (state.mode !== "live") return
  const info = readAfterCommand(state, patch)
  if (info && state.finished.settled === "resolved" && info.playState !== "finished") {
    state.finished = deferred()
  }
}

/**
 * A CSS animation or transition of an element, as `element.getAnimations()`
 * returns it: the Web Animations `Animation` of the DOM, for what the CSS
 * engine runs. `getAnimations()` gives the same object for the same run every
 * time. `playState`, `currentTime` and `playbackRate` are read when asked,
 * as of the last frame.
 *
 * Not built: `cancel()`, `finish()`, `reverse()`, `updatePlaybackRate()`,
 * `ready`, `effect`, `timeline`, `startTime`, `pending`, `onfinish` and
 * `oncancel`. An animation of the motion engine is not listed.
 */
export class Animation {
  constructor(init: AnimationInit) {
    const state: State = {
      host: init.host,
      id: init.info.id,
      elementId: init.elementId,
      mode: "live",
      last: init.info,
      finished: deferred(),
    }
    states.set(this, state)
    if (init.info.playState === "finished") state.finished.resolve(this)
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
      case "live":
        return readLive(state).currentTime
      case "idle":
        return null
      case "frozen":
        return state.last.playbackRate < 0 ? 0 : state.last.endTime
    }
  }

  set currentTime(value: number | null) {
    const state = stateOf(this)
    if (typeof value !== "number" || !Number.isFinite(value)) {
      throw new TypeError(`Failed to set currentTime: ${String(value)} is not a finite number`)
    }
    requireLive(state, "set currentTime of")
    state.host.setAnimationCurrentTime(state.id, value)
    renewFinished(state)
  }

  /** The speed of time, negative to run back. Changing it keeps `currentTime`. */
  get playbackRate(): number {
    const state = stateOf(this)
    return state.mode === "live" ? readLive(state).playbackRate : state.last.playbackRate
  }

  set playbackRate(value: number) {
    const state = stateOf(this)
    if (typeof value !== "number" || !Number.isFinite(value)) {
      throw new TypeError(`Failed to set playbackRate: ${String(value)} is not a finite number`)
    }
    requireLive(state, "set playbackRate of")
    state.host.setAnimationPlaybackRate(state.id, value)
    renewFinished(state, { playbackRate: value })
  }

  get playState(): AnimationPlayState {
    const state = stateOf(this)
    switch (state.mode) {
      case "live":
        return readLive(state).playState as AnimationPlayState
      case "idle":
        return "idle"
      case "frozen":
        return "finished"
    }
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
    renewFinished(state)
    return state.finished.promise as Promise<this>
  }

  /** Holds the current time at once. From here `animation-play-state` no longer decides. */
  pause(): void {
    const state = stateOf(this)
    requireLive(state, "pause")
    state.host.pauseAnimation(state.id)
  }

  /**
   * Continues from the held time at the next frame. A finished animation
   * starts over: from 0, or from the end when the rate is negative. From here
   * `animation-play-state` no longer decides.
   */
  play(): void {
    const state = stateOf(this)
    requireLive(state, "play")
    state.host.playAnimation(state.id)
    renewFinished(state)
  }
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

function release(registry: Registry, state: State, animation: Animation): void {
  registry.byId.delete(state.id)
  const siblings = registry.byElement.get(state.elementId)
  siblings?.delete(animation)
  if (siblings?.size === 0) registry.byElement.delete(state.elementId)
}

function adopt(registry: Registry, host: WebAnimationHost, elementId: number, info: WebAnimationInfo): Animation {
  const known = registry.byId.get(info.id)
  if (known) return known
  const init = { host, elementId, info }
  const animation = info.kind === "transition" ? new CSSTransition(init) : new CSSAnimation(init)
  registry.byId.set(info.id, animation)
  const siblings = registry.byElement.get(elementId) ?? new Set()
  siblings.add(animation)
  registry.byElement.set(elementId, siblings)
  return animation
}

/**
 * `element.getAnimations()`: the CSS transitions, then the CSS animations,
 * that are running, paused or in effect through a fill, in composite order.
 * Pending mutations are sent first, so a style just set is seen.
 */
export function getElementAnimations(renderer: NativeRenderer, elementId: number): Animation[] {
  const host = hostOf(renderer)
  flushPendingMutations(renderer)
  const registry = registryOf(renderer)
  return host.getAnimations(elementId).map((info) => adopt(registry, host, elementId, info))
}

function cancel(registry: Registry, animation: Animation): void {
  const state = stateOf(animation)
  release(registry, state, animation)
  state.mode = "idle"
  if (state.finished.settled === "pending") {
    state.finished.reject(new DOMException("The animation was cancelled", "AbortError"))
  }
  state.finished = deferred()
}

function finish(registry: Registry, animation: Animation): void {
  const state = stateOf(animation)
  if (state.mode !== "live") return
  if (state.last.kind === "transition") {
    // Native dropped the run when it finished: what it showed last is all that is left.
    release(registry, state, animation)
    state.mode = "frozen"
  } else {
    try {
      if (readLive(state).playState !== "finished") return
    } catch (error) {
      // Cancelled since the frame that finished it: its cancel event is next
      // and wins, so this finish is not owed to anyone.
      if (isUnknownAnimation(error)) return
      throw error
    }
  }
  if (state.finished.settled === "pending") state.finished.resolve(animation)
}

/** Whether the event is one of the internal ones `deliverWebAnimationEvent` takes. */
export function isWebAnimationEvent(eventType: string): boolean {
  return eventType === FINISH_EVENT || eventType === CANCEL_EVENT
}

/**
 * Takes the finish or cancel of a CSS animation or transition for the
 * `Animation` object listed for it. Nothing else hears these events: they are
 * not DOM events. A run no one listed is ignored.
 */
export function deliverWebAnimationEvent(renderer: NativeRenderer, event: EventPayload): void {
  if (event.animationId === undefined) {
    throw new Error(`Event "${event.eventType}" arrived without its animationId`)
  }
  const registry = registryOf(renderer)
  const animation = registry.byId.get(event.animationId)
  if (!animation) return
  if (event.eventType === FINISH_EVENT) finish(registry, animation)
  else cancel(registry, animation)
}

/**
 * A removed element's animations are cancelled, as the DOM does: native sends
 * nothing when an element goes, so the ids the mutation queue was told are
 * destroyed end here.
 */
export function cancelAnimationsOfElements(renderer: object, elementIds: readonly number[]): void {
  const registry = registryOf(renderer)
  for (const elementId of elementIds) {
    for (const animation of [...(registry.byElement.get(elementId) ?? [])]) cancel(registry, animation)
  }
}
