import type { EventPayload } from "../../index.js"
import type { EventHandlerMap } from "../host.js"

export const EVENT_PHASE_NONE = 0
export const EVENT_PHASE_CAPTURING = 1
export const EVENT_PHASE_AT_TARGET = 2
export const EVENT_PHASE_BUBBLING = 3

/**
 * Types that do not bubble, compared case-insensitively because native types
 * are camelCase (`mouseEnter`) and DOM names are lowercase (`mouseenter`).
 * Everything else bubbles.
 */
const NON_BUBBLING_TYPES: ReadonlySet<string> = new Set([
  "mouseenter",
  "mouseleave",
  "pointerenter",
  "pointerleave",
  "focus",
  "blur",
  "load",
  "error",
  "scroll",
])

/** `preventDefault()` is a no-op on these, as in the DOM. */
const NON_CANCELABLE_TYPES: ReadonlySet<string> = new Set([
  ...NON_BUBBLING_TYPES,
  "focusin",
  "focusout",
  "pointercancel",
  "gotpointercapture",
  "lostpointercapture",
])

export function eventBubbles(eventType: string): boolean {
  return !NON_BUBBLING_TYPES.has(eventType.toLowerCase())
}

export function eventIsCancelable(eventType: string): boolean {
  return !NON_CANCELABLE_TYPES.has(eventType.toLowerCase())
}

/**
 * Key of a capture-phase handler in `EventHandlerMap`: the event type plus
 * `"Capture"`, next to the bubble-phase handler registered under the bare type.
 */
export function captureHandlerKey(eventType: string): string {
  return `${eventType}Capture`
}

/** The event object handlers receive: the native payload plus the DOM surface. */
export interface DomEvent extends EventPayload {
  readonly type: string
  readonly target: number
  currentTarget: number | null
  eventPhase: number
  readonly bubbles: boolean
  readonly cancelable: boolean
  readonly timeStamp: number
  defaultPrevented: boolean
  stopPropagation(): void
  stopImmediatePropagation(): void
  preventDefault(): void
}

export interface DomDispatchResult {
  defaultPrevented: boolean
  propagationStopped: boolean
}

export interface DomEventRoot {
  eventHandlers: EventHandlerMap
}

/**
 * Runs `payload` through the handlers registered on `root`: capture handlers
 * from the root down to the target, then the target's, then bubble handlers
 * from the target back up, along `payload.path` (target first). Handlers run
 * synchronously. A payload without `path` is handled at `payload.elementId`
 * only. A handler that throws propagates the error and ends the dispatch.
 */
export function dispatchDomEvent(root: DomEventRoot, payload: EventPayload): DomDispatchResult {
  const path = payload.path ?? [payload.elementId]
  if (path.length === 0) {
    throw new Error(`Event "${payload.eventType}" has an empty propagation path.`)
  }
  const target = path[0]
  if (payload.path && payload.target != null && payload.target !== target) {
    throw new Error(
      `Event "${payload.eventType}" targets element ${payload.target} but its path starts at ${target}.`
    )
  }

  let propagationStopped = false
  let immediatePropagationStopped = false
  const cancelable = eventIsCancelable(payload.eventType)
  const event: DomEvent = {
    ...payload,
    type: payload.eventType,
    target,
    currentTarget: null,
    eventPhase: EVENT_PHASE_NONE,
    bubbles: eventBubbles(payload.eventType),
    cancelable,
    timeStamp: performance.now(),
    defaultPrevented: false,
    stopPropagation() {
      propagationStopped = true
    },
    stopImmediatePropagation() {
      propagationStopped = true
      immediatePropagationStopped = true
    },
    preventDefault() {
      if (cancelable) event.defaultPrevented = true
    },
  }

  const captureKey = captureHandlerKey(payload.eventType)
  const visit = (elementId: number, phase: number, keys: string[]) => {
    event.currentTarget = elementId
    event.eventPhase = phase
    const handlers = root.eventHandlers.get(elementId)
    if (!handlers) return
    for (const key of keys) {
      if (immediatePropagationStopped) return
      handlers.get(key)?.(event)
    }
  }

  for (let index = path.length - 1; index > 0 && !propagationStopped; index--) {
    visit(path[index], EVENT_PHASE_CAPTURING, [captureKey])
  }
  if (!propagationStopped) {
    visit(target, EVENT_PHASE_AT_TARGET, [captureKey, payload.eventType])
  }
  if (event.bubbles) {
    for (let index = 1; index < path.length && !propagationStopped; index++) {
      visit(path[index], EVENT_PHASE_BUBBLING, [payload.eventType])
    }
  }

  event.currentTarget = null
  event.eventPhase = EVENT_PHASE_NONE
  return { defaultPrevented: event.defaultPrevented, propagationStopped }
}
