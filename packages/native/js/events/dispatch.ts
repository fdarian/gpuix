import type { EventPayload } from "../../index.js"
import type { EventHandlerMap } from "../host.js"

export const EVENT_PHASE_NONE = 0
export const EVENT_PHASE_CAPTURING = 1
export const EVENT_PHASE_AT_TARGET = 2
export const EVENT_PHASE_BUBBLING = 3

/**
 * Types that neither bubble nor can be cancelled. Types are compared
 * case-insensitively because native types are camelCase (`mouseEnter`) and DOM
 * names are lowercase (`mouseenter`).
 */
const PLAIN_NOTIFICATIONS: ReadonlySet<string> = new Set([
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

/** Types that do not bubble. Everything else bubbles. `invalid` is cancelable. */
const NON_BUBBLING_TYPES: ReadonlySet<string> = new Set([...PLAIN_NOTIFICATIONS, "invalid"])

/** `preventDefault()` is a no-op on these, as in the DOM. */
const NON_CANCELABLE_TYPES: ReadonlySet<string> = new Set([
  ...PLAIN_NOTIFICATIONS,
  "focusin",
  "focusout",
  "pointercancel",
  "gotpointercapture",
  "lostpointercapture",
  "transitionrun",
  "transitionstart",
  "transitionend",
  "transitioncancel",
  "animationstart",
  "animationiteration",
  "animationend",
  "animationcancel",
  "compositionstart",
  "compositionupdate",
  "compositionend",
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

/**
 * What a handler sees of the native payload: every field but the three that
 * carry element ids, which the event replaces with elements. `elementId` stays
 * the numeric id of the element the payload was addressed to.
 */
export type PublicEventFields = Omit<EventPayload, "target" | "relatedTarget" | "path">

/**
 * An event with no propagation, such as `onSelectionChange` or
 * `onPointerLockChange`: the payload with its elements in place of their ids.
 * An element is the object a `ref` gives in the adapter (React's host
 * instance, Solid's element).
 */
export interface RootEvent extends PublicEventFields {
  /**
   * The element the event was aimed at. `null` for an event aimed at the
   * window: a key with nothing focused, a pointer move outside every element.
   * GPUIX has no `body` or `html` element to stand in.
   */
  readonly target: object | null
  /** The other element of a boundary or focus event, or `null` when there is none. */
  readonly relatedTarget: object | null
}

/** The event object handlers receive: the native payload plus the DOM surface. */
export interface DomEvent extends RootEvent {
  readonly type: string
  /**
   * The element whose handler is running, or the `window` / `document` object
   * during their phases. `null` outside a dispatch.
   */
  currentTarget: object | null
  /** The elements on the propagation path, target first, like the DOM's `composedPath()`. */
  composedPath(): object[]
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
  /**
   * The element for a native element id, as the adapter's ref gives it, or
   * `undefined` once the root has dropped it.
   */
  elementById(id: number): object | undefined
  /**
   * Where an error a handler throws goes: the renderer's `onError`. Without it
   * the error is reported to the global handler, as the DOM does.
   */
  reportError?: (error: unknown) => void
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
  /**
   * Before the first capture handler. Skipped phases follow `state`. A throw
   * of a listener goes to `fail` so the dispatch goes on.
   */
  beforeCapture?(event: DomEvent, state: DomEventState, fail: (error: unknown) => void): void
  /**
   * After the last bubble handler, for an event that bubbles and was not stopped.
   * A throw of a listener goes to `fail`.
   */
  afterBubble?(event: DomEvent, state: DomEventState, fail: (error: unknown) => void): void
}

/** What a form's `submit` event adds: the `FormData` and no submitter button. */
function formSubmission(entries: string[][]): { formData: FormData; submitter: null } {
  const formData = new FormData()
  for (const [name, value] of entries) formData.append(name, value)
  return { formData, submitter: null }
}

/** The element for `id`, or throws: the root dropped an element native still names. */
export function requireElement(root: DomEventRoot, id: number, eventType: string): object {
  const element = root.elementById(id)
  if (element === undefined) {
    throw new Error(`Event "${eventType}" names element ${id}, which its root does not have.`)
  }
  return element
}

/**
 * The element for an id native may not have sent. `null` for no id, and for an
 * element the root has already dropped: one that left the tree is not the other
 * end of a boundary or focus event any more.
 */
export function optionalElement(root: DomEventRoot, id: number | undefined): object | null {
  if (id == null) return null
  const element = root.elementById(id)
  return element === undefined ? null : element
}

/** The payload as a public event: element ids replaced by the elements they name. */
export function toRootEvent(root: DomEventRoot, payload: EventPayload, target: object | null): RootEvent {
  const fields: PublicEventFields & Partial<Pick<EventPayload, "target" | "relatedTarget" | "path">> = {
    ...payload,
  }
  delete fields.target
  delete fields.relatedTarget
  delete fields.path
  return { ...fields, target, relatedTarget: optionalElement(root, payload.relatedTarget) }
}

/**
 * Creates the event object and the state behind its stop methods. `path` is
 * the propagation path as elements, target first; empty for an event aimed at
 * the window, which has a `null` target.
 */
export function createDomEvent(
  root: DomEventRoot,
  payload: EventPayload,
  path: readonly object[]
): { event: DomEvent; state: DomEventState } {
  const state = { propagationStopped: false, immediatePropagationStopped: false }
  const cancelable = eventIsCancelable(payload.eventType)
  const event: DomEvent = {
    ...toRootEvent(root, payload, path[0] ?? null),
    ...(payload.formEntries && formSubmission(payload.formEntries)),
    type: payload.eventType,
    currentTarget: null,
    composedPath: () => [...path],
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
export function reportUncaught(error: unknown): void {
  if (typeof reportError === "function") {
    reportError(error)
    return
  }
  queueMicrotask(() => {
    throw error
  })
}

/** Reports the errors a dispatch collected, once it is over. */
export function reportHandlerErrors(root: DomEventRoot, errors: readonly unknown[]): void {
  errors.forEach(root.reportError ?? reportUncaught)
}

/**
 * Runs `payload` through the handlers registered on `root`: capture handlers
 * from the root down to the target, then the target's, then bubble handlers
 * from the target back up, along `payload.path` (target first). Handlers run
 * synchronously. A payload without `path` is handled at `payload.elementId`
 * only. A handler that throws does not end the dispatch: the error goes to
 * `root.reportError` (or the global handler) after the dispatch, and the rest
 * of the handlers run.
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

  const elements = path.map((id) => requireElement(root, id, payload.eventType))
  const { event, state } = createDomEvent(root, payload, elements)
  const errors: unknown[] = []
  const captureKey = captureHandlerKey(payload.eventType)
  const visit = (index: number, phase: number, keys: string[]) => {
    event.currentTarget = elements[index]
    event.eventPhase = phase
    const handlers = root.eventHandlers.get(path[index])
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

  const fail = (error: unknown) => errors.push(error)
  hooks?.beforeCapture?.(event, state, fail)
  for (let index = path.length - 1; index > 0 && !state.propagationStopped; index--) {
    visit(index, EVENT_PHASE_CAPTURING, [captureKey])
  }
  if (!state.propagationStopped) {
    visit(0, EVENT_PHASE_AT_TARGET, [captureKey, payload.eventType])
  }
  if (event.bubbles) {
    for (let index = 1; index < path.length && !state.propagationStopped; index++) {
      visit(index, EVENT_PHASE_BUBBLING, [payload.eventType])
    }
  }
  if (event.bubbles && !state.propagationStopped) hooks?.afterBubble?.(event, state, fail)

  event.currentTarget = null
  event.eventPhase = EVENT_PHASE_NONE
  reportHandlerErrors(root, errors)
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
export function preventEventDefault(event: PublicEventFields): void {
  const cancelable = event as Partial<DomEvent>
  if (!cancelable.preventDefault) {
    throw new Error(`Event "${event.eventType}" cannot be cancelled: it was not dispatched through the DOM walk.`)
  }
  cancelable.preventDefault()
}
