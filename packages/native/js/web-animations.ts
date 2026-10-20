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
    | "releaseAnimation"
  >
>

const HOST_METHODS = [
  "getAnimations",
  "getAnimation",
  "pauseAnimation",
  "playAnimation",
  "setAnimationCurrentTime",
  "setAnimationPlaybackRate",
  "releaseAnimation",
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
 * Where an animation stands. `live`: native answers for it, from the run or,
 * between the frame that cancelled it and the event reaching JS, from what it
 * showed then. `idle`: cancelled.
 */
type Mode = "live" | "idle"

interface State {
  readonly host: WebAnimationHost
  readonly registry: Registry
  readonly id: number
  readonly elementId: number
  mode: Mode
  /** What native last said, kept for what outlives the run: the rate an idle animation still reports. */
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
  registry: Registry
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

/**
 * Sends a command to native. A run native no longer has is one that was
 * cancelled since the frame JS last heard of, as far as JS is concerned.
 */
function command(animation: Animation, action: string, send: (state: State) => void): void {
  const state = stateOf(animation)
  const cancelled = () => invalidState(`Cannot ${action} an animation that was cancelled`)
  if (state.mode === "idle") throw cancelled()
  try {
    send(state)
  } catch (error) {
    throw isUnknownAnimation(error) ? cancelled() : error
  }
  renewFinished(state)
}

/**
 * Once the animation has left `finished` (a `play()` that rewound it, a seek),
 * `finished` is a new promise, as in the DOM.
 */
function renewFinished(state: State): void {
  if (state.mode !== "live") return
  if (state.finished.settled === "resolved" && readLive(state).playState !== "finished") {
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
      registry: init.registry,
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
      case "live": {
        const info = readLive(state)
        return info.playState === "idle" ? null : info.currentTime
      }
      case "idle":
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
      case "idle":
        return "idle"
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
    command(this, "pause", (state) => state.host.pauseAnimation(state.id))
  }

  /**
   * Continues from the held time at the next frame. A finished animation
   * starts over: from 0, or from the end when the rate is negative. From here
   * `animation-play-state` no longer decides.
   */
  play(): void {
    command(this, "play", (state) => state.host.playAnimation(state.id))
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

function adopt(registry: Registry, host: WebAnimationHost, elementId: number, info: WebAnimationInfo): Animation {
  const known = registry.byId.get(info.id)
  if (known) return known
  const init = { host, registry, elementId, info }
  const animation = info.kind === "transition" ? new CSSTransition(init) : new CSSAnimation(init)
  register(registry, elementId, info.id, animation)
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

function cancel(animation: Animation): void {
  const state = stateOf(animation)
  unregister(state.registry, state, animation)
  state.mode = "idle"
  if (state.finished.settled === "pending") {
    state.finished.reject(new DOMException("The animation was cancelled", "AbortError"))
  }
  state.finished = deferred()
}

function finish(animation: Animation): void {
  const state = stateOf(animation)
  if (state.mode !== "live") return
  const info = readLive(state)
  // Cancelled since the frame that finished it: its cancel event is next and wins.
  if (info.playState !== "finished") return
  if (state.finished.settled === "pending") state.finished.resolve(animation)
}

/** Whether the event is one of the internal ones `deliverWebAnimationEvent` takes. */
export function isWebAnimationEvent(eventType: string): boolean {
  return eventType === FINISH_EVENT || eventType === CANCEL_EVENT
}

/**
 * Takes the finish or cancel of a CSS animation or transition for the
 * `Animation` object listed for it, then tells native it is consumed, which
 * stops answering for the run as it ended. Nothing else hears these events:
 * they are not DOM events. A run no one listed is released all the same.
 */
export function deliverWebAnimationEvent(renderer: NativeRenderer, event: EventPayload): void {
  if (event.animationId === undefined) {
    throw new Error(`Event "${event.eventType}" arrived without its animationId`)
  }
  try {
    const animation = registryOf(renderer).byId.get(event.animationId)
    if (!animation) return
    if (event.eventType === FINISH_EVENT) finish(animation)
    else cancel(animation)
  } finally {
    renderer.releaseAnimation?.(event.animationId)
  }
}

/**
 * A removed element's animations are cancelled, as the DOM does: native sends
 * nothing when an element goes, so the ids the mutation queue was told are
 * destroyed end here.
 */
export function cancelAnimationsOfElements(renderer: object, elementIds: readonly number[]): void {
  const registry = registryOf(renderer)
  for (const elementId of elementIds) {
    for (const animation of [...(registry.byElement.get(elementId) ?? [])]) cancel(animation)
  }
}
