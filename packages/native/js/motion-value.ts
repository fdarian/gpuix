/// Motion values: values that live in the renderer's native graph, are sampled
/// at the frame clock, and bind to element style through the `motionValues`
/// custom prop. This file is the framework-free core; the React and Solid
/// adapters wrap it.
///
/// A value starts in JS, with its state here, and gets a native node when it is
/// bound to a renderer (`bind`). Every binding and every listener keeps the node
/// alive, and the node is disposed when the last one goes away, so a value
/// outlives its elements without leaking. `destroy()` is for values the caller
/// drops.

import type {
  BoxShadow,
  MotionCurve,
  MotionEase,
  MotionKeyframes,
  MotionSpringDurationTransition,
  MotionSpringPhysicsTransition,
  MotionTransition,
  SvgMotionTransition,
  MotionValueHost,
  NativeRenderer,
} from "./host.js"
import { createRendererState } from "./renderer-state.js"
import type { MotionValueSink } from "./renderer-state.js"

/** A renderer that owns a motion value graph. */
export type MotionValueRenderer = NativeRenderer & MotionValueHost

/** What a motion value can hold: a number, a colour or percentage string, a list of numbers, or box shadows. */
export type MotionData = number | string | number[] | BoxShadow | BoxShadow[]

/** `motionValue(0)` is a `MotionValue<number>`, not a `MotionValue<0>`. */
export type Widen<Data> = Data extends number ? number : Data extends string ? string : Data

/** A spring transition without its `type`. */
export type MotionSpringConfig =
  | Omit<MotionSpringPhysicsTransition, "type">
  | Omit<MotionSpringDurationTransition, "type">

export type MotionTransformOptions = {
  /** Hold the output at its ends outside the input range. Defaults to true. */
  clamp?: boolean
  /** One easing for every segment, or one per segment. */
  ease?: MotionEase | MotionEase[]
}

export type MotionValueEvents<Data extends MotionData> = {
  /**
   * The value changed. For a bound value this arrives with the frame that
   * sampled the change, not inside the call that made it.
   */
  change: (value: Data) => void
  animationStart: () => void
  animationComplete: () => void
}

export interface AnimationControls extends PromiseLike<void> {
  /** Resolves when the animation completes, is stopped, cancelled or replaced. Never rejects. */
  readonly finished: Promise<void>
  /** Ends the animation where it is. */
  stop(): void
  /** Ends the animation and puts the value back where it started. */
  cancel(): void
  /** Ends the animation at its target. */
  complete(): void
}

/**
 * What `animate` takes for a value: one target, or keyframes. A `null` stands
 * for the keyframe before it, or for the value's own value when it comes
 * first. An array is keyframes unless it is itself a value of the kind the
 * source holds: a `number[]` target of a list-of-numbers value, or a
 * `BoxShadow[]` target of a shadow value. Keyframes of those kinds are lists of
 * lists.
 */
export type MotionTarget<Data extends MotionData> = MotionKeyframes<Data>

export type NumberAnimationOptions = {
  /** The renderer that samples the animation. */
  host: MotionValueRenderer
  onUpdate?: (value: number) => void
}

const HOST_METHODS = [
  "createMotionValue",
  "createMotionTransform",
  "createMotionFollower",
  "createMotionSpring",
  "disposeMotionValue",
  "getMotionValue",
  "getMotionValueVelocity",
  "isMotionValueAnimating",
  "setMotionValue",
  "jumpMotionValue",
  "animateMotionValue",
  "stopMotionValue",
  "setMotionValueListener",
] as const satisfies readonly (keyof MotionValueHost)[]

const INSTANT: MotionCurve = { duration: 0 }

/**
 * JSON has no `Infinity`, so an infinite repeat goes to the renderer as the
 * string it accepts. The motion components send their transitions the same way.
 */
export function wireTransition(transition: MotionCurve): MotionCurve
export function wireTransition(transition: MotionCurve | undefined): MotionCurve | undefined
export function wireTransition(transition: MotionCurve | undefined): MotionCurve | undefined {
  if (transition?.repeat !== Infinity) return transition
  return { ...transition, repeat: "Infinity" } as unknown as MotionCurve
}

/**
 * `wireTransition` for what a `motion.*` component takes: the repeat of the
 * curve at the top, of `default`, and of each property's own.
 */
export function wireMotionTransition<Transition extends MotionTransition | SvgMotionTransition>(
  transition: Transition | undefined
): Transition | undefined {
  if (transition === undefined) return undefined
  const wired: Record<string, unknown> = { ...wireTransition(transition) }
  for (const key of Object.keys(wired)) {
    const value = wired[key]
    if (typeof value === "object" && value !== null && !Array.isArray(value)) {
      wired[key] = wireTransition(value as MotionCurve)
    }
  }
  return wired as Transition
}

function isShadowLayer(item: unknown): boolean {
  return typeof item === "object" && item !== null && !Array.isArray(item)
}

/**
 * Where an animation of `target` ends. The renderer reads an array as keyframes
 * unless it is a list of the kind `from` holds, and this reads it the same way.
 */
function endOf(target: MotionTarget<MotionData>, from: MotionData): MotionData {
  if (!Array.isArray(target)) return target
  const items: unknown[] = target
  const holdsNumbers = Array.isArray(from) && typeof from[0] === "number"
  const holdsShadows = !holdsNumbers && typeof from === "object"
  if (
    (holdsNumbers && items.every((item) => typeof item === "number")) ||
    (holdsShadows && items.every(isShadowLayer))
  ) {
    return target as MotionData
  }
  for (let index = items.length - 1; index >= 0; index--) {
    if (items[index] !== null) return items[index] as MotionData
  }
  return from
}

type ComputedTrigger = "change" | "animationComplete"

type Definition =
  | { kind: "source" }
  | { kind: "spring"; config: MotionSpringConfig }
  | {
      kind: "transform"
      input: MotionValue<number>
      inputRange: number[]
      outputRange: MotionData[]
      options?: MotionTransformOptions
    }
  | { kind: "follower"; source: MotionValue<MotionData>; config: MotionSpringConfig }
  | { kind: "computed"; compute: () => MotionData; triggers: ComputedTrigger[] }

type ListenerKind = "change" | "animationStart" | "animationComplete"

type Listener = {
  callback: (...args: never[]) => void
}

type Attachment = {
  readonly renderer: MotionValueRenderer
  /** Releases of the values this one reads, undone after its node is disposed. */
  readonly teardown: Array<() => void>
  unsubscribe?: () => void
  created: boolean
  sentChange: boolean
  sentAnimation: boolean
}

type Animation = {
  readonly target: MotionTarget<MotionData>
  readonly from: MotionData
  readonly transition: MotionCurve | undefined
  readonly resolve: () => void
  started: boolean
  unwatch?: () => void
}

const COUNTER_KEY = Symbol.for("@gpuix/native/motion-value-ids")

/** One counter per process, so values from two copies of this module never share an id. */
function nextId(): number {
  const next = ((Reflect.get(globalThis, COUNTER_KEY) as number | undefined) ?? 0) + 1
  Reflect.set(globalThis, COUNTER_KEY, next)
  return next
}

let tracker: ((value: MotionValue<MotionData>) => void) | undefined

function tracking<Result>(
  onRead: (value: MotionValue<MotionData>) => void,
  run: () => Result
): Result {
  const outer = tracker
  tracker = onRead
  try {
    return run()
  } finally {
    tracker = outer
  }
}

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve = () => {}
  const promise = new Promise<void>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function sameValue(a: unknown, b: unknown): boolean {
  return a === b || JSON.stringify(a) === JSON.stringify(b)
}

function assertHost(renderer: NativeRenderer): asserts renderer is MotionValueRenderer {
  const missing = HOST_METHODS.filter((method) => typeof Reflect.get(renderer, method) !== "function")
  if (missing.length > 0) {
    throw new Error(
      `This renderer has no motion value graph (missing ${missing.join(", ")}), so a motion value cannot be bound to it`
    )
  }
}

export class MotionValue<Data extends MotionData = MotionData> {
  readonly id = nextId()

  readonly #definition: Definition
  /** The value while there is no native node. Sources and springs only. */
  #state: MotionData | undefined
  #attachment: Attachment | undefined
  #bindings = 0
  /** Elements that bind this value to a position or size property under reduced motion. */
  #reducers = 0
  #destroyed = false
  #animation: Animation | undefined
  readonly #counts: Record<ListenerKind, number> = {
    change: 0,
    animationStart: 0,
    animationComplete: 0,
  }
  readonly #listeners: Record<ListenerKind, Listener[]> = {
    change: [],
    animationStart: [],
    animationComplete: [],
  }

  /** Use `motionValue`, `transformValue`, `springValue`, `computedValue` or `velocityValue`. */
  constructor(definition: Definition, initial?: Data) {
    this.#definition = definition
    this.#state = initial
  }

  /** The renderer this value is bound to, if any. */
  get host(): MotionValueRenderer | undefined {
    return this.#attachment?.renderer
  }

  /**
   * The current value. Bound, it is read from the renderer at its clock. A
   * transform and a spring follower have no state of their own, so they can
   * only be read while bound.
   */
  get(): Data {
    tracker?.(this)
    this.#assertAlive()
    if (this.#attachment) return this.#attachment.renderer.getMotionValue(this.id) as Data
    const definition = this.#definition
    switch (definition.kind) {
      case "source":
      case "spring":
        return this.#state as Data
      case "computed":
        return definition.compute() as Data
      case "transform":
      case "follower":
        throw new Error(
          `A ${definition.kind} motion value has no state until it is bound to a renderer, so it cannot be read yet`
        )
    }
  }

  /**
   * Writes the value. It does not stop a running animation: call `stop()`
   * first to take the value from the animation. On a spring it moves the
   * target.
   */
  set(value: Data): void {
    this.#write("set", value)
  }

  /** Writes the value, ending any animation and dropping its velocity. */
  jump(value: Data): void {
    this.#write("jump", value)
    this.#settle(this.#animation)
  }

  /** Units per second, for a value that holds a number. A value that is not bound has none. */
  getVelocity(): number {
    tracker?.(this)
    this.#assertAlive()
    if (this.#attachment) return this.#attachment.renderer.getMotionValueVelocity(this.id)
    if (typeof this.get() !== "number") {
      throw new Error("getVelocity is for motion values that hold a number")
    }
    return 0
  }

  isAnimating(): boolean {
    this.#assertAlive()
    if (this.#attachment) return this.#attachment.renderer.isMotionValueAnimating(this.id)
    return this.#animation !== undefined
  }

  /** Ends an animation where it is, keeping the speed for the next one. */
  stop(): void {
    this.#assertAlive()
    this.#attachment?.renderer.stopMotionValue(this.id)
    this.#settle(this.#animation)
  }

  /**
   * Animates from where the value is, with the speed it has. Before the value
   * is bound the animation waits, and starts from the value at that point.
   * Only a `motionValue()` animates: a spring is moved with `set()`.
   */
  animate(target: MotionTarget<Data>, transition?: MotionCurve): AnimationControls {
    this.#assertAlive()
    if (this.#definition.kind !== "source") {
      throw new Error(
        this.#definition.kind === "spring"
          ? "A spring motion value cannot be animated: set it to move its target"
          : `A ${this.#definition.kind} motion value is computed from other values and cannot be animated`
      )
    }
    const waiting = deferred()
    const finished = waiting.promise
    const animation: Animation = {
      target: target as MotionTarget<MotionData>,
      from: this.get(),
      transition: this.#reducers > 0 ? INSTANT : transition,
      resolve: waiting.resolve,
      started: false,
    }
    const previous = this.#animation
    this.#animation = animation
    if (this.#attachment) this.#start(animation)
    // The new animation holds the node first, so replacing the last one
    // cannot be the release that disposes it.
    if (previous) this.#finish(previous)

    const current = () => this.#animation === animation
    return {
      finished,
      then: (onFulfilled, onRejected) => finished.then(onFulfilled, onRejected),
      stop: () => {
        if (current()) this.stop()
      },
      cancel: () => {
        if (!current()) return
        this.stop()
        this.jump(animation.from as Data)
      },
      complete: () => {
        if (current()) this.jump(endOf(animation.target, animation.from) as Data)
      },
    }
  }

  /**
   * Makes `animate()` jump to its target, finishing on the next frame as a
   * zero-length animation does, for as long as the returned release is not
   * called. The adapters call it for a value that a reduced-motion
   * `motion.*` element binds to a position or size property.
   */
  reduceMotion(): () => void {
    this.#reducers += 1
    let released = false
    return () => {
      if (released) return
      released = true
      this.#reducers -= 1
    }
  }

  on<Event extends ListenerKind>(event: Event, callback: MotionValueEvents<Data>[Event]): () => void {
    this.#assertAlive()
    const listener: Listener = { callback: callback as Listener["callback"] }
    this.#listeners[event].push(listener)
    const unwatch = this.#watch(event)
    return () => {
      const index = this.#listeners[event].indexOf(listener)
      if (index >= 0) this.#listeners[event].splice(index, 1)
      unwatch()
    }
  }

  /**
   * Binds the value to a renderer, creating its native node on the first
   * bind. Returns the release. The node goes when every binding is released
   * and no listener is left; the value keeps its state in JS.
   */
  bind(renderer: NativeRenderer): () => void {
    this.#assertAlive()
    assertHost(renderer)
    if (this.#attachment) {
      if (this.#attachment.renderer !== renderer) {
        throw new Error(
          `Motion value ${this.id} is bound to another renderer. A value drives one window at a time; release it there first`
        )
      }
    } else {
      this.#attach(renderer)
    }
    this.#bindings += 1
    let released = false
    return () => {
      if (released) return
      released = true
      if (this.#destroyed) return
      this.#bindings -= 1
      this.#maybeDetach()
    }
  }

  /**
   * Drops the value: its node is disposed whatever still binds it, its
   * listeners are removed and any animation ends. Every later call throws.
   */
  destroy(): void {
    if (this.#destroyed) return
    this.#settle(this.#animation)
    this.#detach(false)
    this.#destroyed = true
    this.#bindings = 0
    for (const kind of Object.keys(this.#listeners) as ListenerKind[]) {
      this.#listeners[kind].length = 0
      this.#counts[kind] = 0
    }
  }

  #assertAlive(): void {
    if (this.#destroyed) throw new Error(`Motion value ${this.id} was destroyed`)
  }

  #write(method: "set" | "jump", value: Data): void {
    this.#assertAlive()
    const definition = this.#definition
    if (definition.kind === "transform" || definition.kind === "computed") {
      throw new Error(`A ${definition.kind} motion value is computed from other values and cannot be written`)
    }
    const attachment = this.#attachment
    if (attachment) {
      if (method === "set") attachment.renderer.setMotionValue(this.id, value)
      else attachment.renderer.jumpMotionValue(this.id, value)
      return
    }
    if (definition.kind === "follower") {
      throw new Error("A spring follower has no state until it is bound to a renderer, so it cannot be written yet")
    }
    const changed = !sameValue(this.#state, value)
    this.#state = value
    if (changed) this.#emit("change", value)
  }

  #emit(event: ListenerKind, ...args: unknown[]): void {
    for (const listener of [...this.#listeners[event]]) {
      ;(listener.callback as (...args: unknown[]) => void)(...args)
    }
  }

  #sink: MotionValueSink = {
    change: (value) => this.#emit("change", value),
    animationStart: () => this.#emit("animationStart"),
    animationComplete: () => {
      this.#emit("animationComplete")
      const attachment = this.#attachment
      const animation = this.#animation
      // A stop and a new animation in one frame deliver the old completion
      // after the new start, so ask whether the value still moves.
      if (attachment && animation && !attachment.renderer.isMotionValueAnimating(this.id)) {
        this.#settle(animation)
      }
    },
  }

  #watch(kind: ListenerKind): () => void {
    this.#counts[kind] += 1
    this.#syncListener()
    let watching = true
    return () => {
      if (!watching) return
      watching = false
      if (this.#destroyed) return
      this.#counts[kind] -= 1
      this.#syncListener()
      this.#maybeDetach()
    }
  }

  #syncListener(): void {
    const attachment = this.#attachment
    if (!attachment) return
    const change = this.#counts.change > 0
    const animation = this.#counts.animationStart + this.#counts.animationComplete > 0
    if (attachment.sentChange === change && attachment.sentAnimation === animation) return
    attachment.sentChange = change
    attachment.sentAnimation = animation
    attachment.renderer.setMotionValueListener(this.id, change, animation)
  }

  #maybeDetach(): void {
    if (!this.#attachment || this.#bindings > 0) return
    if (Object.values(this.#counts).some((count) => count > 0)) return
    this.#detach(true)
  }

  #attach(renderer: MotionValueRenderer): void {
    const attachment: Attachment = {
      renderer,
      teardown: [],
      created: false,
      sentChange: false,
      sentAnimation: false,
    }
    this.#attachment = attachment
    try {
      this.#create(attachment)
      attachment.created = true
      attachment.unsubscribe = createRendererState(renderer).subscribeMotionValue(this.id, this.#sink)
      this.#syncListener()
      const animation = this.#animation
      if (animation && !animation.started) this.#start(animation)
    } catch (error) {
      this.#attachment = undefined
      attachment.unsubscribe?.()
      if (attachment.created) renderer.disposeMotionValue(this.id)
      for (const undo of attachment.teardown.reverse()) undo()
      throw error
    }
  }

  #create(attachment: Attachment): void {
    const renderer = attachment.renderer
    const definition = this.#definition
    switch (definition.kind) {
      case "source":
        renderer.createMotionValue(this.id, this.#state)
        return
      case "spring":
        renderer.createMotionSpring(this.id, this.#state, definition.config)
        return
      case "transform":
        attachment.teardown.push(definition.input.bind(renderer))
        renderer.createMotionTransform(
          this.id,
          definition.input.id,
          definition.inputRange,
          definition.outputRange,
          definition.options
        )
        return
      case "follower":
        attachment.teardown.push(definition.source.bind(renderer))
        renderer.createMotionFollower(this.id, definition.source.id, definition.config)
        return
      case "computed":
        this.#createComputed(attachment, definition)
        return
    }
  }

  /**
   * A computed value is a source that JS keeps current: it follows the change
   * events of every value its function read, so it is not linked natively and
   * trails them by a frame.
   */
  #createComputed(
    attachment: Attachment,
    definition: Extract<Definition, { kind: "computed" }>
  ): void {
    const renderer = attachment.renderer
    const watched = new Map<MotionValue<MotionData>, () => void>()
    let reads = new Set<MotionValue<MotionData>>()
    const read = (dep: MotionValue<MotionData>) => {
      if (dep === (this as MotionValue<MotionData>) || reads.has(dep)) return
      reads.add(dep)
      if (watched.has(dep)) return
      // Bound before the read goes on, so a transform can be read at all.
      const release = dep.bind(renderer)
      const stops = definition.triggers.map((trigger) => dep.on(trigger, update))
      watched.set(dep, () => {
        for (const stop of stops) stop()
        release()
      })
    }
    const evaluate = () => {
      reads = new Set()
      const result = tracking(read, definition.compute)
      for (const [dep, release] of watched) {
        if (reads.has(dep)) continue
        watched.delete(dep)
        release()
      }
      return result
    }
    const update = () => {
      if (this.#attachment !== attachment) return
      renderer.setMotionValue(this.id, evaluate())
    }
    attachment.teardown.push(() => {
      for (const release of watched.values()) release()
      watched.clear()
    })
    renderer.createMotionValue(this.id, evaluate())
  }

  #detach(keepState: boolean): void {
    const attachment = this.#attachment
    if (!attachment) return
    this.#attachment = undefined
    const kind = this.#definition.kind
    if (keepState && (kind === "source" || kind === "spring")) {
      this.#state = attachment.renderer.getMotionValue(this.id) as MotionData
    }
    this.#settle(this.#animation)
    // The sink goes first, then the node, then the inputs the node read.
    attachment.unsubscribe?.()
    attachment.renderer.disposeMotionValue(this.id)
    for (const undo of attachment.teardown.reverse()) undo()
  }

  #start(animation: Animation): void {
    const attachment = this.#attachment
    if (!attachment) throw new Error(`Motion value ${this.id} has no renderer to animate on`)
    animation.started = true
    // Listened before the animation starts, so its start and end are seen.
    animation.unwatch = this.#watch("animationComplete")
    try {
      attachment.renderer.animateMotionValue(this.id, animation.target, wireTransition(animation.transition))
    } catch (error) {
      this.#settle(animation)
      throw error
    }
    if (!attachment.renderer.isMotionValueAnimating(this.id)) this.#settle(animation)
  }

  /** Ends `animation` if it is the current one. */
  #settle(animation: Animation | undefined): void {
    if (!animation || this.#animation !== animation) return
    this.#animation = undefined
    this.#finish(animation)
  }

  #finish(animation: Animation): void {
    animation.unwatch?.()
    animation.resolve()
  }
}

/** A value that is written with `set` and `jump` or animated. */
export function motionValue<Data extends MotionData>(initial: Data): MotionValue<Widen<Data>> {
  return new MotionValue<Widen<Data>>({ kind: "source" }, initial as Widen<Data>)
}

export function isMotionValue(value: unknown): value is MotionValue {
  return value instanceof MotionValue
}

/**
 * Maps a number value through `inputRange` and `outputRange`, natively: it is
 * sampled at the same instant as its input. The input range is ascending or
 * descending and may repeat a value, which is a step. The output holds
 * numbers, colours or shadows. A bad range is reported when the value is bound.
 */
export function transformValue<Data extends MotionData>(
  input: MotionValue<number>,
  inputRange: number[],
  outputRange: Data[],
  options?: MotionTransformOptions
): MotionValue<Widen<Data>> {
  return new MotionValue<Widen<Data>>({
    kind: "transform",
    input,
    inputRange,
    outputRange,
    options,
  })
}

/**
 * A spring that follows `source`, or, given a plain value, a spring with a
 * target of its own that `set` moves.
 */
export function springValue<Data extends MotionData>(
  source: MotionValue<Data>,
  config?: MotionSpringConfig
): MotionValue<Data>
export function springValue<Data extends MotionData>(
  initial: Data,
  config?: MotionSpringConfig
): MotionValue<Widen<Data>>
export function springValue(
  source: MotionValue<MotionData> | MotionData,
  config: MotionSpringConfig = {}
): MotionValue<MotionData> {
  if (isMotionValue(source)) return new MotionValue({ kind: "follower", source, config })
  return new MotionValue({ kind: "spring", config }, source)
}

/**
 * A value computed in JS from the motion values `compute` reads with `get()`.
 * It is not native-linked: it follows their change events and writes a source,
 * so it trails them by a frame, and it only updates while it is bound.
 * Unbound, `get()` runs `compute`.
 */
export function computedValue<Data extends MotionData>(
  compute: () => Data
): MotionValue<Widen<Data>> {
  return new MotionValue<Widen<Data>>({
    kind: "computed",
    compute,
    triggers: ["change"],
  })
}

/**
 * The velocity of a number value, in units per second. Like `computedValue`
 * it is JS-derived and not native-linked: it reads `getVelocity()` of the
 * source on each of its changes, and again when its animation completes.
 */
export function velocityValue(source: MotionValue<number>): MotionValue<number> {
  return new MotionValue<number>({
    kind: "computed",
    compute: () => source.getVelocity(),
    triggers: ["change", "animationComplete"],
  })
}

/** Animates `value`. Returns controls whose `finished` resolves when it ends. */
export function animate<Data extends MotionData>(
  value: MotionValue<Data>,
  target: MotionTarget<Data>,
  transition?: MotionCurve
): AnimationControls
/**
 * Animates a number on `options.host`, reporting each frame to `onUpdate`.
 * The temporary value is destroyed when the animation ends.
 */
export function animate(
  from: number,
  to: MotionTarget<number>,
  options: MotionCurve & NumberAnimationOptions
): AnimationControls
export function animate(
  first: MotionValue<MotionData> | number,
  second: MotionTarget<MotionData>,
  third?: (MotionCurve & Partial<NumberAnimationOptions>) | MotionCurve
): AnimationControls {
  if (first instanceof MotionValue) return first.animate(second, third as MotionCurve | undefined)
  const options = third as MotionCurve & Partial<NumberAnimationOptions>
  if (!options?.host) throw new Error("animate(from, to) needs options.host, the renderer that samples the animation")
  const curve: Record<string, unknown> = { ...options }
  delete curve.host
  delete curve.onUpdate
  const temporary = motionValue(first)
  const release = temporary.bind(options.host)
  const onUpdate = options.onUpdate
  const stopUpdates = onUpdate ? temporary.on("change", onUpdate) : () => {}
  const controls = temporary.animate(second as MotionTarget<number>, curve as MotionCurve)
  void controls.finished.then(() => {
    stopUpdates()
    release()
    temporary.destroy()
  })
  return controls
}
