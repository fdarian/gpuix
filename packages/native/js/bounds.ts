import type { ElementBounds } from "../index.js"
import type { MutationRenderer, NativeRenderer } from "./host.js"

/** Border box in window coordinates, shaped like a DOM `DOMRectReadOnly`. */
export interface BoundingClientRect {
  x: number
  y: number
  width: number
  height: number
  top: number
  left: number
  right: number
  bottom: number
}

interface FlushableQueue {
  flushMutations(): void
}

const QUEUES_KEY = Symbol.for("@gpuix/native/mutation-queues")

// Keyed by the renderer object a queue applies to. Queues are held weakly: a
// root that was unmounted and replaced leaves its queue behind otherwise.
function allQueues(): WeakMap<object, Set<WeakRef<FlushableQueue>>> {
  const existing = Reflect.get(globalThis, QUEUES_KEY) as
    | WeakMap<object, Set<WeakRef<FlushableQueue>>>
    | undefined
  if (existing) return existing
  const queues = new WeakMap<object, Set<WeakRef<FlushableQueue>>>()
  Reflect.set(globalThis, QUEUES_KEY, queues)
  return queues
}

/**
 * Let `flushPendingMutations(renderer)` reach this queue. `createMutationQueue`
 * calls it, so a framework adapter never has to.
 */
export function registerMutationQueue(renderer: object, queue: FlushableQueue): void {
  const queues = allQueues()
  let refs = queues.get(renderer)
  if (!refs) {
    refs = new Set()
    queues.set(renderer, refs)
  }
  for (const ref of refs) {
    if (ref.deref() === undefined) refs.delete(ref)
  }
  refs.add(new WeakRef(queue))
}

/**
 * Send every mutation a framework has queued for this renderer. A read that
 * must see the current tree (bounds, hit tests) calls it first, because Solid
 * flushes in a microtask and React only at commit.
 */
export function flushPendingMutations(renderer: object): void {
  const refs = allQueues().get(renderer)
  if (!refs) return
  for (const ref of [...refs]) {
    const queue = ref.deref()
    if (queue) queue.flushMutations()
    else refs.delete(ref)
  }
}

/** The zero rect a DOM element without a box reports. */
export function toBoundingClientRect(bounds: ElementBounds | null): BoundingClientRect {
  if (!bounds) return { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 }
  return {
    x: bounds.x,
    y: bounds.y,
    width: bounds.width,
    height: bounds.height,
    top: bounds.y,
    left: bounds.x,
    right: bounds.x + bounds.width,
    bottom: bounds.y + bounds.height,
  }
}

/**
 * `getBoundingClientRect` for the element with this id: pending mutations are
 * sent and the renderer lays out before it answers. An element with no box
 * (not mounted, not painted) reports a zero rect.
 */
export function readBoundingClientRect(renderer: NativeRenderer, id: number): BoundingClientRect {
  if (!renderer.getElementBounds) {
    throw new Error("This renderer cannot measure elements: it has no getElementBounds")
  }
  flushPendingMutations(renderer)
  return toBoundingClientRect(renderer.getElementBounds(id))
}

/**
 * Key of the method a framework adapter puts on its host element instances to
 * let shared code (observers) reach the element's renderer and mutation queue.
 */
export const HOST_BINDING: unique symbol = Symbol.for("@gpuix/native/host-binding")

/**
 * Key of the method a host element offers to be told each time it is created
 * natively. Solid creates the native element when the tree is inserted and
 * again after a removal, so an observer started earlier must follow it. An
 * element that is always mounted when handed out (React) does not have it.
 */
export const HOST_MOUNT: unique symbol = Symbol.for("@gpuix/native/host-mount")

export interface HostBinding {
  readonly renderer: NativeRenderer
  readonly mutations: MutationRenderer
}

/** A mounted host element, as `ResizeObserver` and `IntersectionObserver` take it. */
export interface ObservableElement {
  readonly id: number
  /** The binding while the element is mounted, `undefined` before and after. */
  [HOST_BINDING](): HostBinding | undefined
  /** Calls `listener` each time the element is created natively. Returns the stop function. */
  [HOST_MOUNT]?(listener: () => void): () => void
}
