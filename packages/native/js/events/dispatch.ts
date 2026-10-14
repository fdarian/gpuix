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

/**
 * Non-bubbling types an ancestor hears in the capture phase only. Native is told
 * about their capture listeners apart from the bubble ones (see
 * `nativeEventType`), so it can skip the event where no one above is capturing.
 */
const ENTER_LEAVE_TYPES: ReadonlySet<string> = new Set([
  "mouseenter",
  "mouseleave",
  "pointerenter",
  "pointerleave",
])

export function isEnterLeaveType(eventType: string): boolean {
  return ENTER_LEAVE_TYPES.has(eventType)
}

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
  /** Undefined only for an event aimed at the window, such as a key with nothing focused. */
  readonly target: number | undefined
  /** An element id, or the `window` / `document` object during their phases. */
  currentTarget: number | object | null
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

/** What `stopPropagation()` and `stopImmediatePropagation()` have done so far. */
export interface DomEventState {
  readonly propagationStopped: boolean
  readonly immediatePropagationStopped: boolean
}

/**
 * Phases that belong outside the element tree: `window` and `document` wrap
 * the path. They run with the live event, so a `preventDefault()` or
 * `stopPropagation()` on either side is seen by the other.
 */
export interface DomDispatchHooks {
  /** Before the first capture handler. Skipped phases follow `state`. */
  beforeCapture?(event: DomEvent, state: DomEventState): void
  /** After the last bubble handler, for an event that bubbles and was not stopped. */
  afterBubble?(event: DomEvent, state: DomEventState): void
}

/**
 * Creates the event object and the state behind its stop methods. `target` is
 * undefined for an event with no element to aim at.
 */
export function createDomEvent(
  payload: EventPayload,
  target: number | undefined
): { event: DomEvent; state: DomEventState } {
  const state = { propagationStopped: false, immediatePropagationStopped: false }
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
      state.propagationStopped = true
    },
    stopImmediatePropagation() {
      state.propagationStopped = true
      state.immediatePropagationStopped = true
    },
    preventDefault() {
      if (cancelable) event.defaultPrevented = true
    },
  }
  return { event, state }
}

/**
 * What the DOM does with an exception a listener leaves uncaught: report it to
 * the global error handler without disturbing the dispatch or its caller.
 */
function reportHandlerError(error: unknown): void {
  if (typeof reportError === "function") {
    reportError(error)
    return
  }
  queueMicrotask(() => {
    throw error
  })
}

/**
 * Runs `payload` through the handlers registered on `root`: capture handlers
 * from the root down to the target, then the target's, then bubble handlers
 * from the target back up, along `payload.path` (target first). Handlers run
 * synchronously. A payload without `path` is handled at `payload.elementId`
 * only. A handler that throws does not end the dispatch: the error is reported
 * after the dispatch, as the DOM reports it, and the rest of the handlers run.
 */
export function dispatchDomEvent(
  root: DomEventRoot,
  payload: EventPayload,
  hooks?: DomDispatchHooks
): DomDispatchResult {
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

  const { event, state } = createDomEvent(payload, target)
  const errors: unknown[] = []
  const captureKey = captureHandlerKey(payload.eventType)
  const visit = (elementId: number, phase: number, keys: string[]) => {
    event.currentTarget = elementId
    event.eventPhase = phase
    const handlers = root.eventHandlers.get(elementId)
    if (!handlers) return
    for (const key of keys) {
      if (state.immediatePropagationStopped) return
      try {
        handlers.get(key)?.(event)
      } catch (error) {
        errors.push(error)
      }
    }
  }

  hooks?.beforeCapture?.(event, state)
  for (let index = path.length - 1; index > 0 && !state.propagationStopped; index--) {
    visit(path[index], EVENT_PHASE_CAPTURING, [captureKey])
  }
  if (!state.propagationStopped) {
    visit(target, EVENT_PHASE_AT_TARGET, [captureKey, payload.eventType])
  }
  if (event.bubbles) {
    for (let index = 1; index < path.length && !state.propagationStopped; index++) {
      visit(path[index], EVENT_PHASE_BUBBLING, [payload.eventType])
    }
  }
  if (event.bubbles && !state.propagationStopped) hooks?.afterBubble?.(event, state)

  event.currentTarget = null
  event.eventPhase = EVENT_PHASE_NONE
  errors.forEach(reportHandlerError)
  return {
    defaultPrevented: event.defaultPrevented,
    propagationStopped: state.propagationStopped,
  }
}

/**
 * `event.preventDefault()` for a handler typed with the plain payload. Throws
 * for an event that did not come through `dispatchDomEvent`, where there is no
 * default to cancel.
 */
export function preventEventDefault(event: EventPayload): void {
  const cancelable = event as Partial<DomEvent>
  if (!cancelable.preventDefault) {
    throw new Error(`Event "${event.eventType}" cannot be cancelled: it was not dispatched through the DOM walk.`)
  }
  cancelable.preventDefault()
}
