import type { EventPayload } from "../index.js"
import {
  flushPendingMutations,
  HOST_BINDING,
  HOST_MOUNT,
  readBoundingClientRect,
  toBoundingClientRect,
  type BoundingClientRect,
  type HostBinding,
  type ObservableElement,
} from "./bounds.js"

/**
 * `ResizeObserver` and `IntersectionObserver` for host elements.
 *
 * Native reports an observed element's border box, content box and visible
 * (clipped) rect after each paint where they changed; the rules of the two DOM
 * APIs are applied here. Entries are delivered once per batch of native
 * events, after all of a frame's reports have arrived.
 *
 * Observing does not read layout. The first observation of every element
 * observed since the last frame is probed together, in one step after the
 * commit, so N observers started by one commit cost one draw, not N.
 */

export const RESIZE_OBSERVE_EVENT = "resizeObserve"
export const INTERSECT_OBSERVE_EVENT = "intersectObserve"

type ObserveEvent = typeof RESIZE_OBSERVE_EVENT | typeof INTERSECT_OBSERVE_EVENT

const EPSILON = 0.01

// ── Per-renderer hub ─────────────────────────────────────────────────

interface Subscriber {
  receive(event: EventPayload): void
  /** The element is mounted and the subscriber is listening to it. */
  bound?(binding: HostBinding): void
  /**
   * The step after `bound` that probes every new subscription at once: the
   * listener is registered and the window has been laid out and painted, so a
   * first report is already on its way if the element painted. `painted` says
   * whether it did.
   */
  settled?(binding: HostBinding, painted: boolean): void
}

interface Listening {
  readonly binding: HostBinding
  readonly subscribers: Set<Subscriber>
  /**
   * The newest report of the element. Native reports a change of the element,
   * not of an observer, so a subscriber that joins later has to be given it.
   */
  last: EventPayload | undefined
}

interface Probe {
  readonly binding: HostBinding
  readonly id: number
  readonly entry: Listening
  readonly subscriber: Subscriber
  cancelled: boolean
}

interface Delivery {
  deliver(): void
}

interface Hub {
  readonly listening: Record<ObserveEvent, Map<number, Listening>>
  readonly queued: Set<Delivery>
  probes: Probe[]
  scheduled: ReturnType<typeof setTimeout> | ReturnType<typeof setImmediate> | undefined
}

const HUBS_KEY = Symbol.for("@gpuix/native/observer-hubs")

function hubFor(renderer: object): Hub {
  let hubs = Reflect.get(globalThis, HUBS_KEY) as WeakMap<object, Hub> | undefined
  if (!hubs) {
    hubs = new WeakMap()
    Reflect.set(globalThis, HUBS_KEY, hubs)
  }
  let hub = hubs.get(renderer)
  if (!hub) {
    hub = {
      listening: {
        [RESIZE_OBSERVE_EVENT]: new Map(),
        [INTERSECT_OBSERVE_EVENT]: new Map(),
      },
      queued: new Set(),
      probes: [],
      scheduled: undefined,
    }
    hubs.set(renderer, hub)
  }
  return hub
}

function setNativeListener(
  binding: HostBinding,
  id: number,
  kind: ObserveEvent,
  on: boolean,
  flush: boolean
) {
  binding.mutations.setEventListener(id, kind, on)
  if (flush) binding.mutations.flushMutations()
}

/**
 * Add a subscriber to the element's reports. The first report, or the
 * subscriber's own copy of the last one, comes with the next `settleObservations`:
 * the DOM too delivers an initial observation at the next rendering step.
 */
function attach(
  binding: HostBinding,
  id: number,
  kind: ObserveEvent,
  subscriber: Subscriber
): () => void {
  const hub = hubFor(binding.renderer)
  const listening = hub.listening[kind]
  let entry = listening.get(id)
  if (!entry) {
    entry = { binding, subscribers: new Set(), last: undefined }
    listening.set(id, entry)
    setNativeListener(binding, id, kind, true, false)
  }
  const current = entry
  current.subscribers.add(subscriber)
  subscriber.bound?.(binding)
  const probe: Probe = { binding, id, entry: current, subscriber, cancelled: false }
  hub.probes.push(probe)
  ensureScheduled(binding.renderer)
  return () => {
    probe.cancelled = true
    current.subscribers.delete(subscriber)
    if (current.subscribers.size > 0 || listening.get(id) !== current) return
    listening.delete(id)
    setNativeListener(binding, id, kind, false, true)
  }
}

/**
 * Subscribe to an element's native reports. An element that is not mounted yet
 * (a Solid `ref` runs before the tree is inserted) stays pending, as in the
 * DOM, and reports once it has a box. An element created natively again after
 * a removal is subscribed again.
 */
function listen(kind: ObserveEvent, element: ObservableElement, subscriber: Subscriber): () => void {
  let release: (() => void) | undefined
  const bind = () => {
    const binding = element[HOST_BINDING]()
    if (!binding) return
    if (release) {
      // Created again: the native element is new and has no listener.
      setNativeListener(binding, element.id, kind, true, false)
      return
    }
    release = attach(binding, element.id, kind, subscriber)
  }
  bind()
  const stopMountWatch = element[HOST_MOUNT]?.(bind)
  return () => {
    stopMountWatch?.()
    release?.()
    release = undefined
  }
}

function schedule(run: () => void): Hub["scheduled"] {
  return typeof setImmediate === "function" ? setImmediate(run) : setTimeout(run, 0)
}

function cancelScheduled(handle: NonNullable<Hub["scheduled"]>): void {
  if (typeof clearImmediate === "function") clearImmediate(handle as ReturnType<typeof setImmediate>)
  clearTimeout(handle as ReturnType<typeof setTimeout>)
}

function ensureScheduled(renderer: object): void {
  const hub = hubFor(renderer)
  if (hub.scheduled === undefined) hub.scheduled = schedule(() => flushObservations(renderer))
}

function queueDelivery(renderer: object, delivery: Delivery): void {
  hubFor(renderer).queued.add(delivery)
  ensureScheduled(renderer)
}

/**
 * Probe every subscription made since the last probe, now: the pending
 * mutations are sent and the window is laid out once for all of them. A
 * subscriber that joined an element that has reported already is handed that
 * report instead, with no layout read.
 */
export function settleObservations(renderer: object): void {
  const hub = hubFor(renderer)
  const probes = hub.probes.filter((probe) => !probe.cancelled)
  hub.probes = []
  if (probes.length === 0) return
  flushPendingMutations(renderer)
  for (const probe of probes) {
    if (probe.entry.last) {
      probe.subscriber.receive(probe.entry.last)
      continue
    }
    if (!probe.binding.renderer.getElementBounds) {
      throw new Error("This renderer cannot observe elements: it has no getElementBounds")
    }
    // Read even for a subscriber that does not care: the read is what draws
    // the frame whose report it is waiting for.
    const painted = probe.binding.renderer.getElementBounds(probe.id) !== null
    probe.subscriber.settled?.(probe.binding, painted)
  }
}

function reportCallbackError(error: unknown): void {
  const report = (globalThis as { reportError?: (error: unknown) => void }).reportError
  if (report) report(error)
  else queueMicrotask(() => {
    throw error
  })
}

/**
 * Run the callbacks of every observer with entries waiting, now. The frame
 * loop delivers on its own right after a frame's reports; a test calls this
 * after draining native events, since nothing else would run.
 */
export function flushObservations(renderer: object): void {
  settleObservations(renderer)
  const hub = hubFor(renderer)
  if (hub.scheduled !== undefined) {
    cancelScheduled(hub.scheduled)
    hub.scheduled = undefined
  }
  const queued = [...hub.queued]
  hub.queued.clear()
  for (const delivery of queued) delivery.deliver()
}

/** Called for every native event. Returns whether an observer wanted it. */
export function deliverObservation(renderer: object, event: EventPayload): boolean {
  if (event.eventType !== RESIZE_OBSERVE_EVENT && event.eventType !== INTERSECT_OBSERVE_EVENT) {
    return false
  }
  const entry = hubFor(renderer).listening[event.eventType].get(event.elementId)
  if (!entry || !event.observation) return false
  entry.last = event
  for (const subscriber of [...entry.subscribers]) subscriber.receive(event)
  return true
}

// ── ResizeObserver ───────────────────────────────────────────────────

export type ResizeObserverBoxOptions = "border-box" | "content-box"

export interface ResizeObserverOptions {
  /** Which box's size triggers the callback. Defaults to `"content-box"`. */
  box?: ResizeObserverBoxOptions
}

export interface ResizeObserverSize {
  readonly inlineSize: number
  readonly blockSize: number
}

export interface ResizeObserverEntry {
  readonly target: ObservableElement
  /** The content box, positioned by the padding on the left and top. */
  readonly contentRect: BoundingClientRect
  readonly borderBoxSize: readonly [ResizeObserverSize]
  readonly contentBoxSize: readonly [ResizeObserverSize]
}

export type ResizeObserverCallback = (
  entries: ResizeObserverEntry[],
  observer: ResizeObserver
) => void

interface ResizeRecord {
  readonly target: ObservableElement
  readonly box: ResizeObserverBoxOptions
  /** Starts at 0 by 0 like the DOM: an element that is empty has nothing to report. */
  last: readonly [number, number]
  stop(): void
}

function near(left: number, right: number): boolean {
  return Math.abs(left - right) < EPSILON
}

export class ResizeObserver {
  readonly #callback: ResizeObserverCallback
  readonly #records = new Map<ObservableElement, ResizeRecord>()
  #pending = new Map<ObservableElement, ResizeObserverEntry>()
  readonly #delivery: Delivery = { deliver: () => this.#deliver() }
  /** Set when the first record binds; every event arrives after that. */
  #renderer!: object

  constructor(callback: ResizeObserverCallback) {
    if (typeof callback !== "function") {
      throw new TypeError("ResizeObserver: the callback is not a function")
    }
    this.#callback = callback
  }

  /** Start observing, or restart: observing an element again reports its size again. */
  observe(target: ObservableElement, options: ResizeObserverOptions = {}): void {
    const box = options.box ?? "content-box"
    if (box !== "content-box" && box !== "border-box") {
      throw new TypeError(`ResizeObserver: unsupported box "${String(box)}"`)
    }
    this.unobserve(target)
    const record: ResizeRecord = {
      target,
      box,
      last: [0, 0],
      stop: () => {},
    }
    this.#records.set(target, record)
    record.stop = listen(RESIZE_OBSERVE_EVENT, target, {
      receive: (event) => this.#receive(record, event),
      bound: (binding) => {
        this.#renderer = binding.renderer
      },
    })
  }

  unobserve(target: ObservableElement): void {
    this.#records.get(target)?.stop()
    this.#records.delete(target)
    this.#pending.delete(target)
  }

  disconnect(): void {
    for (const record of this.#records.values()) record.stop()
    this.#records.clear()
    this.#pending.clear()
  }

  #receive(record: ResizeRecord, event: EventPayload): void {
    const observation = event.observation!
    const borderWidth = event.width ?? 0
    const borderHeight = event.height ?? 0
    const size: readonly [number, number] =
      record.box === "border-box"
        ? [borderWidth, borderHeight]
        : [observation.contentWidth, observation.contentHeight]
    if (near(size[0], record.last[0]) && near(size[1], record.last[1])) return
    record.last = size
    const left = observation.paddingLeft
    const top = observation.paddingTop
    this.#pending.set(record.target, {
      target: record.target,
      contentRect: {
        x: left,
        y: top,
        width: observation.contentWidth,
        height: observation.contentHeight,
        left,
        top,
        right: left + observation.contentWidth,
        bottom: top + observation.contentHeight,
      },
      borderBoxSize: [{ inlineSize: borderWidth, blockSize: borderHeight }],
      contentBoxSize: [
        { inlineSize: observation.contentWidth, blockSize: observation.contentHeight },
      ],
    })
    queueDelivery(this.#renderer, this.#delivery)
  }

  #deliver(): void {
    if (this.#pending.size === 0) return
    const entries = [...this.#pending.values()]
    this.#pending.clear()
    try {
      this.#callback.call(this, entries, this)
    } catch (error) {
      reportCallbackError(error)
    }
  }
}

// ── IntersectionObserver ─────────────────────────────────────────────

export interface IntersectionObserverInit {
  /** The element whose box targets are tested against. `null` is the window. */
  root?: ObservableElement | null
  /** Grows or shrinks the root before testing: 1 to 4 lengths in `px` or `%`. */
  rootMargin?: string
  threshold?: number | number[]
}

export interface IntersectionObserverEntry {
  /** `performance.now()` when the change was observed. */
  readonly time: number
  /** The root's box with `rootMargin` applied. */
  readonly rootBounds: BoundingClientRect
  readonly boundingClientRect: BoundingClientRect
  readonly intersectionRect: BoundingClientRect
  readonly isIntersecting: boolean
  readonly intersectionRatio: number
  readonly target: ObservableElement
}

export type IntersectionObserverCallback = (
  entries: IntersectionObserverEntry[],
  observer: IntersectionObserver
) => void

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

interface IntersectionSample {
  present: boolean
  border: Rect
  /** Ancestor `overflow` clips, nearest first. */
  clips: readonly { id: number; rect: Rect }[]
  viewport: { width: number; height: number }
}

interface IntersectionRecord {
  readonly target: ObservableElement
  sample: IntersectionSample | undefined
  previousThresholdIndex: number
  previousIsIntersecting: boolean
  stop(): void
}

interface MarginLength {
  readonly value: number
  readonly percent: boolean
}

const MARGIN_LENGTH = /^(-?(?:\d+\.?\d*|\.\d+))(px|%)$/

function parseRootMargin(text: string): readonly [MarginLength, MarginLength, MarginLength, MarginLength] {
  const tokens = text.trim().split(/\s+/)
  if (text.trim() === "" || tokens.length > 4) {
    throw new SyntaxError(`IntersectionObserver: invalid rootMargin "${text}"`)
  }
  const lengths = tokens.map((token): MarginLength => {
    const match = MARGIN_LENGTH.exec(token)
    if (!match) throw new SyntaxError(`IntersectionObserver: invalid rootMargin "${text}"`)
    return { value: Number(match[1]), percent: match[2] === "%" }
  })
  const [top, right = top, bottom = top, left = right] = lengths as [
    MarginLength,
    MarginLength?,
    MarginLength?,
    MarginLength?,
  ]
  return [top!, right!, bottom!, left!]
}

function area(rect: Rect): number {
  return rect.width * rect.height
}

function overlap(left: Rect, right: Rect): Rect {
  const x = Math.max(left.x, right.x)
  const y = Math.max(left.y, right.y)
  return {
    x,
    y,
    width: Math.max(0, Math.min(left.x + left.width, right.x + right.width) - x),
    height: Math.max(0, Math.min(left.y + left.height, right.y + right.height) - y),
  }
}

function contains(outer: Rect, inner: Rect): boolean {
  return (
    inner.x >= outer.x &&
    inner.y >= outer.y &&
    inner.x + inner.width <= outer.x + outer.width &&
    inner.y + inner.height <= outer.y + outer.height
  )
}

function asRect(rect: BoundingClientRect): Rect {
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

function asBoundingClientRect(rect: Rect): BoundingClientRect {
  return toBoundingClientRect(rect)
}

export class IntersectionObserver {
  readonly root: ObservableElement | null
  readonly rootMargin: string
  readonly thresholds: readonly number[]
  readonly #callback: IntersectionObserverCallback
  readonly #margin: ReturnType<typeof parseRootMargin>
  readonly #records = new Map<ObservableElement, IntersectionRecord>()
  #queue: IntersectionObserverEntry[] = []
  readonly #delivery: Delivery = { deliver: () => this.#deliver() }
  #renderer!: object
  #stopWatchingRoot: (() => void) | undefined

  constructor(callback: IntersectionObserverCallback, options: IntersectionObserverInit = {}) {
    if (typeof callback !== "function") {
      throw new TypeError("IntersectionObserver: the callback is not a function")
    }
    this.#callback = callback
    this.root = options.root ?? null
    this.#margin = parseRootMargin(options.rootMargin ?? "0px")
    this.rootMargin = this.#margin
      .map((length) => `${length.value}${length.percent ? "%" : "px"}`)
      .join(" ")
    const threshold = options.threshold ?? 0
    const thresholds = (Array.isArray(threshold) ? threshold : [threshold]).slice()
    for (const value of thresholds) {
      if (typeof value !== "number" || !(value >= 0 && value <= 1)) {
        throw new RangeError(`IntersectionObserver: threshold ${String(value)} is not between 0 and 1`)
      }
    }
    this.thresholds = (thresholds.length === 0 ? [0] : thresholds).sort((a, b) => a - b)
  }

  /** Start observing. The first entry arrives after the next frame, whatever the state. */
  observe(target: ObservableElement): void {
    if (this.#records.has(target)) return
    const record: IntersectionRecord = {
      target,
      sample: undefined,
      previousThresholdIndex: -1,
      previousIsIntersecting: false,
      stop: () => {},
    }
    this.#records.set(target, record)
    if (this.root && !this.#stopWatchingRoot) {
      // The root moving or resizing changes what the targets intersect, and
      // nothing else tells us: a target that does not move reports nothing.
      this.#stopWatchingRoot = listen(INTERSECT_OBSERVE_EVENT, this.root, {
        receive: () => this.#reevaluateAll(),
      })
    }
    record.stop = listen(INTERSECT_OBSERVE_EVENT, target, {
      receive: (event) => this.#receive(record, event),
      bound: (binding) => {
        this.#renderer = binding.renderer
      },
      settled: (binding, painted) => {
        // An element that does not paint sends no report, but the DOM still
        // reports it once, as not intersecting.
        if (!painted && record.sample === undefined) {
          this.#evaluate(record, absentSample(binding.renderer.getWindowSize?.()))
        }
      },
    })
  }

  unobserve(target: ObservableElement): void {
    this.#records.get(target)?.stop()
    this.#records.delete(target)
    this.#queue = this.#queue.filter((entry) => entry.target !== target)
    if (this.#records.size === 0) this.#stopRoot()
  }

  disconnect(): void {
    for (const record of this.#records.values()) record.stop()
    this.#records.clear()
    this.#queue = []
    this.#stopRoot()
  }

  /** Entries not yet delivered, removed from the queue. */
  takeRecords(): IntersectionObserverEntry[] {
    const entries = this.#queue
    this.#queue = []
    return entries
  }

  #stopRoot(): void {
    this.#stopWatchingRoot?.()
    this.#stopWatchingRoot = undefined
  }

  #receive(record: IntersectionRecord, event: EventPayload): void {
    const observation = event.observation!
    const sample: IntersectionSample = observation.present
      ? {
          present: true,
          border: {
            x: event.x ?? 0,
            y: event.y ?? 0,
            width: event.width ?? 0,
            height: event.height ?? 0,
          },
          clips: observation.clips.map((clip) => ({
            id: clip.id,
            rect: { x: clip.x, y: clip.y, width: clip.width, height: clip.height },
          })),
          viewport: { width: observation.viewportWidth, height: observation.viewportHeight },
        }
      : absentSample(record.sample?.viewport)
    this.#evaluate(record, sample)
  }

  #reevaluateAll(): void {
    for (const record of this.#records.values()) {
      if (record.sample) this.#evaluate(record, record.sample)
    }
  }

  #rootRect(sample: IntersectionSample): Rect {
    let base: Rect
    if (this.root) {
      const binding = this.root[HOST_BINDING]()
      base = binding
        ? asRect(readBoundingClientRect(binding.renderer, this.root.id))
        : { x: 0, y: 0, width: 0, height: 0 }
    } else {
      base = { x: 0, y: 0, width: sample.viewport.width, height: sample.viewport.height }
    }
    const [top, right, bottom, left] = this.#margin.map((length, index) =>
      length.percent ? (length.value / 100) * (index % 2 === 0 ? base.height : base.width) : length.value
    ) as [number, number, number, number]
    return {
      x: base.x - left,
      y: base.y - top,
      width: base.width + left + right,
      height: base.height + top + bottom,
    }
  }

  #evaluate(record: IntersectionRecord, sample: IntersectionSample): void {
    record.sample = sample
    const rootBounds = this.#rootRect(sample)
    const zero = { x: 0, y: 0, width: 0, height: 0 }
    const border = sample.present ? sample.border : zero
    let intersection = zero
    let isIntersecting = false
    if (sample.present) {
      // Clip by the ancestors between the target and the root, like the DOM.
      // Ancestors above the root, and the window edge, do not clip.
      let clipped = border
      for (const clip of sample.clips) {
        if (this.root && clip.id === this.root.id) break
        clipped = overlap(clipped, clip.rect)
      }
      intersection = overlap(clipped, rootBounds)
      isIntersecting =
        area(intersection) > 0 || (area(border) === 0 && contains(rootBounds, border))
      if (area(intersection) === 0 && !isIntersecting) intersection = zero
    }
    const ratio = !isIntersecting ? 0 : area(border) === 0 ? 1 : Math.min(1, area(intersection) / area(border))
    let thresholdIndex = this.thresholds.findIndex((threshold) => threshold > ratio)
    if (thresholdIndex < 0) thresholdIndex = this.thresholds.length
    if (
      thresholdIndex === record.previousThresholdIndex &&
      isIntersecting === record.previousIsIntersecting
    ) {
      return
    }
    record.previousThresholdIndex = thresholdIndex
    record.previousIsIntersecting = isIntersecting
    this.#queue.push({
      time: performance.now(),
      rootBounds: asBoundingClientRect(rootBounds),
      boundingClientRect: asBoundingClientRect(border),
      intersectionRect: asBoundingClientRect(intersection),
      isIntersecting,
      intersectionRatio: ratio,
      target: record.target,
    })
    queueDelivery(this.#renderer, this.#delivery)
  }

  #deliver(): void {
    const entries = this.takeRecords()
    if (entries.length === 0) return
    try {
      this.#callback.call(this, entries, this)
    } catch (error) {
      reportCallbackError(error)
    }
  }
}

function absentSample(viewport = { width: 0, height: 0 }): IntersectionSample {
  const zero = { x: 0, y: 0, width: 0, height: 0 }
  return { present: false, border: zero, clips: [], viewport }
}
