import type { EventPayload } from "../index.js"
import { flushPendingMutations } from "./bounds.js"
import type { HostWheelEvent } from "./events/types.js"
import type { NativeRenderer } from "./host.js"

export type ScrollBehavior = "auto" | "instant" | "smooth"
export type ScrollLogicalPosition = "start" | "center" | "end" | "nearest"

export interface ScrollToOptions {
  /** Pixels from the start of the content. Unset keeps the current position. */
  left?: number
  top?: number
  /** `"auto"` follows the element's `scrollBehavior` style. */
  behavior?: ScrollBehavior
}

export interface ScrollIntoViewOptions {
  /** Vertical alignment. Defaults to `"start"`, like the DOM. */
  block?: ScrollLogicalPosition
  /** Horizontal alignment. Defaults to `"nearest"`, like the DOM. */
  inline?: ScrollLogicalPosition
  /** `"auto"` follows the `scrollBehavior` style of each scroller. */
  behavior?: ScrollBehavior
}

/** `onWheel`'s event: see `HostWheelEvent`. */
export type WheelEvent = HostWheelEvent

/** `onScroll`: the element's scroll offset changed, whatever moved it. */
export interface ScrollEvent extends EventPayload {
  /** Pixels scrolled from the start, as on the DOM element. */
  scrollTop: number
  scrollLeft: number
}

/** The scroll geometry of an element, in the DOM's terms. */
export interface ScrollMetrics {
  scrollLeft: number
  scrollTop: number
  scrollWidth: number
  scrollHeight: number
  clientWidth: number
  clientHeight: number
}

/** Fills in the DOM names of a native `scroll` payload. A `wheel` arrives complete from the dispatcher. */
export function shapeScrollEvent(event: EventPayload): EventPayload {
  if (event.eventType === "scroll") {
    // Native reports the offset the way GPUI keeps it: zero, then negative.
    const scroll: Pick<ScrollEvent, "scrollLeft" | "scrollTop"> = {
      scrollLeft: 0 - required(event.scrollX, "scrollX"),
      scrollTop: 0 - required(event.scrollY, "scrollY"),
    }
    return { ...event, ...scroll }
  }
  return event
}

/** DOM `key` values that scroll by default, and the name `scrollByKey` knows each by. */
const SCROLL_KEY_NAMES: ReadonlyMap<string, string> = new Map([
  ["ArrowUp", "up"],
  ["ArrowDown", "down"],
  ["ArrowLeft", "left"],
  ["ArrowRight", "right"],
  ["PageUp", "pageup"],
  ["PageDown", "pagedown"],
  ["Home", "home"],
  ["End", "end"],
  [" ", "space"],
])

/**
 * What a key's default action does once nothing consumed or prevented it:
 * scroll the nearest scroller. Undefined for a key that does not scroll.
 */
export function scrollKeyName(key: string | undefined): string | undefined {
  return key === undefined ? undefined : SCROLL_KEY_NAMES.get(key)
}

function required(value: number | undefined, field: string): number {
  if (value === undefined) throw new Error(`The native ${field} field is missing`)
  return value
}

/**
 * The one place that asks native for an element's scroll geometry. Every
 * `scrollTop`-style read goes through here.
 *
 * It is never stale, like `getBoundingClientRect`: queued mutations are sent first and
 * native draws any pending change before it measures.
 *
 * An element native has not painted has none; the DOM answers zero for an element
 * with no layout box, and so does this.
 */
export function readScrollMetrics(renderer: NativeRenderer, id: number): ScrollMetrics {
  if (!renderer.getScrollMetrics) {
    throw new Error("This renderer cannot read scroll geometry")
  }
  flushPendingMutations(renderer)
  const metrics = renderer.getScrollMetrics(id)
  if (!metrics) {
    return {
      scrollLeft: 0,
      scrollTop: 0,
      scrollWidth: 0,
      scrollHeight: 0,
      clientWidth: 0,
      clientHeight: 0,
    }
  }
  const [scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight] = metrics
  return { scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight }
}

function scrollToOptions(
  xOrOptions: number | ScrollToOptions | undefined,
  y: number | undefined
): ScrollToOptions {
  if (typeof xOrOptions === "number") {
    if (y === undefined) throw new TypeError("scrollTo(x, y) needs both coordinates")
    return { left: xOrOptions, top: y }
  }
  return xOrOptions ?? {}
}

function scrollTo(
  renderer: NativeRenderer,
  id: number,
  options: ScrollToOptions,
  relative: boolean
): void {
  if (!renderer.scrollTo) throw new Error("This renderer cannot scroll")
  const current = readScrollMetrics(renderer, id)
  const left = options.left === undefined ? current.scrollLeft : options.left + (relative ? current.scrollLeft : 0)
  const top = options.top === undefined ? current.scrollTop : options.top + (relative ? current.scrollTop : 0)
  renderer.scrollTo(id, left, top, options.behavior)
}

/** The members a host element exposes next to `scrollIntoView`. */
export interface ScrollableElement {
  scrollTop: number
  scrollLeft: number
  readonly scrollWidth: number
  readonly scrollHeight: number
  readonly clientWidth: number
  readonly clientHeight: number
  scrollTo(options?: ScrollToOptions): void
  scrollTo(x: number, y: number): void
  scrollBy(options?: ScrollToOptions): void
  scrollBy(x: number, y: number): void
  scrollIntoView(arg?: boolean | ScrollIntoViewOptions): void
}

/** `scrollIntoView(false)` aligns the end, `true` or nothing the start, like the DOM. */
function intoViewOptions(arg: boolean | ScrollIntoViewOptions | undefined): ScrollIntoViewOptions {
  if (typeof arg === "boolean") return { block: arg ? "start" : "end" }
  return arg ?? {}
}

/**
 * Defines the scroll members on `target`, which is a prototype or one instance.
 * `bind` finds the renderer and element id behind `this`.
 */
export function defineScrollMembers<This extends object>(
  target: object,
  bind: (self: This) => { renderer: NativeRenderer | undefined; id: number }
): void {
  const resolve = (self: This) => {
    const bound = bind(self)
    if (!bound.renderer) throw new Error("This element is not attached to a renderer")
    return { renderer: bound.renderer, id: bound.id }
  }
  const metric = (field: keyof ScrollMetrics) => ({
    configurable: true,
    get(this: This) {
      const bound = resolve(this)
      return readScrollMetrics(bound.renderer, bound.id)[field]
    },
  })
  Object.defineProperties(target, {
    scrollTop: {
      ...metric("scrollTop"),
      set(this: This, top: number) {
        const bound = resolve(this)
        scrollTo(bound.renderer, bound.id, { top, behavior: "instant" }, false)
      },
    },
    scrollLeft: {
      ...metric("scrollLeft"),
      set(this: This, left: number) {
        const bound = resolve(this)
        scrollTo(bound.renderer, bound.id, { left, behavior: "instant" }, false)
      },
    },
    scrollWidth: metric("scrollWidth"),
    scrollHeight: metric("scrollHeight"),
    clientWidth: metric("clientWidth"),
    clientHeight: metric("clientHeight"),
    scrollTo: {
      configurable: true,
      writable: true,
      value(this: This, xOrOptions?: number | ScrollToOptions, y?: number) {
        const bound = resolve(this)
        scrollTo(bound.renderer, bound.id, scrollToOptions(xOrOptions, y), false)
      },
    },
    scrollBy: {
      configurable: true,
      writable: true,
      value(this: This, xOrOptions?: number | ScrollToOptions, y?: number) {
        const bound = resolve(this)
        scrollTo(bound.renderer, bound.id, scrollToOptions(xOrOptions, y), true)
      },
    },
    scrollIntoView: {
      configurable: true,
      writable: true,
      value(this: This, arg?: boolean | ScrollIntoViewOptions) {
        const bound = resolve(this)
        if (!bound.renderer.scrollIntoView) throw new Error("This renderer cannot scroll")
        const options = intoViewOptions(arg)
        flushPendingMutations(bound.renderer)
        bound.renderer.scrollIntoView(bound.id, options.block, options.inline, options.behavior)
      },
    },
  })
}
