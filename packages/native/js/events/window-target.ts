import type { EventPayload } from "../../index.js"
import {
  createDomEvent,
  EVENT_PHASE_BUBBLING,
  EVENT_PHASE_CAPTURING,
  EVENT_PHASE_NONE,
  dispatchDomEvent,
  type DomDispatchResult,
  type DomEvent,
  type DomEventRoot,
  type DomEventState,
} from "./dispatch.js"

export type WindowEventListener = (event: DomEvent) => void

export interface AddEventListenerOptions {
  /** Listen in the capture phase, before the element tree sees the event. */
  capture?: boolean
  /** Remove the listener after its first call. */
  once?: boolean
}

interface Registration {
  listener: WindowEventListener
  capture: boolean
  once: boolean
}

/**
 * The `window` and `document` of a GPUIX app: an `EventTarget` shaped like the
 * DOM's, for events that reach the top of the tree.
 *
 * Event types are the DOM's lowercase names (`keydown`, `keyup`, `focusin`,
 * `focusout`, `focus`, `blur`). `focus` and `blur` do not bubble, so only a
 * capture listener hears them, as in the DOM.
 */
export class GpuixEventTarget {
  private readonly registrations = new Map<string, Registration[]>()

  addEventListener(
    type: string,
    listener: WindowEventListener,
    options?: boolean | AddEventListenerOptions
  ): void {
    const capture = typeof options === "boolean" ? options : options?.capture === true
    const once = typeof options === "object" && options.once === true
    const key = type.toLowerCase()
    const registrations = this.registrations.get(key) ?? []
    if (registrations.some((entry) => entry.listener === listener && entry.capture === capture)) {
      return
    }
    registrations.push({ listener, capture, once })
    this.registrations.set(key, registrations)
  }

  removeEventListener(
    type: string,
    listener: WindowEventListener,
    options?: boolean | Pick<AddEventListenerOptions, "capture">
  ): void {
    const capture = typeof options === "boolean" ? options : options?.capture === true
    const key = type.toLowerCase()
    const registrations = this.registrations.get(key)
    if (!registrations) return
    const index = registrations.findIndex(
      (entry) => entry.listener === listener && entry.capture === capture
    )
    if (index < 0) return
    registrations.splice(index, 1)
    if (registrations.length === 0) this.registrations.delete(key)
  }

  /** Whether any listener is registered for `type` in either phase. */
  hasListeners(type: string): boolean {
    return this.registrations.has(type.toLowerCase())
  }

  /** @internal Calls the listeners of one phase. */
  run(event: DomEvent, state: DomEventState, capture: boolean, phase: number): void {
    const registrations = this.registrations.get(event.type.toLowerCase())
    if (!registrations) return
    event.currentTarget = this
    event.eventPhase = phase
    // A listener may add or remove listeners; this call sees the ones that
    // were there when the phase started.
    for (const entry of [...registrations]) {
      if (state.immediatePropagationStopped) return
      if (entry.capture !== capture) continue
      if (entry.once) this.removeEventListener(event.type, entry.listener, entry.capture)
      entry.listener(event)
    }
  }
}

export const window = new GpuixEventTarget()
export const document = new GpuixEventTarget()

export interface PropagationResult extends DomDispatchResult {
  /** The event the listeners saw. */
  event: DomEvent
}

/**
 * Runs an event through the whole propagation: window capture, document
 * capture, the element path, document bubble, window bubble. An event with no
 * element path is aimed at the window and only those two targets hear it.
 *
 * `scoped` is a window-level target that belongs to one renderer, such as the
 * `onKeyDown` option of its root. It hears the bubble phase after `window`.
 */
export function dispatchPropagating(
  root: DomEventRoot,
  payload: EventPayload,
  scoped?: GpuixEventTarget
): PropagationResult {
  const bubble = (event: DomEvent, state: DomEventState) => {
    document.run(event, state, false, EVENT_PHASE_BUBBLING)
    if (!state.propagationStopped) window.run(event, state, false, EVENT_PHASE_BUBBLING)
    if (!state.propagationStopped) scoped?.run(event, state, false, EVENT_PHASE_BUBBLING)
  }
  const capture = (event: DomEvent, state: DomEventState) => {
    window.run(event, state, true, EVENT_PHASE_CAPTURING)
    if (!state.propagationStopped) document.run(event, state, true, EVENT_PHASE_CAPTURING)
  }

  if (payload.path && payload.path.length > 0) {
    let seen: DomEvent | undefined
    const result = dispatchDomEvent(root, payload, {
      beforeCapture(event, state) {
        seen = event
        capture(event, state)
      },
      afterBubble: bubble,
    })
    if (!seen) throw new Error(`Event "${payload.eventType}" was dispatched without an event object.`)
    return { ...result, event: seen }
  }

  const { event, state } = createDomEvent(payload, undefined)
  capture(event, state)
  if (event.bubbles && !state.propagationStopped) bubble(event, state)
  event.currentTarget = null
  event.eventPhase = EVENT_PHASE_NONE
  return {
    defaultPrevented: event.defaultPrevented,
    propagationStopped: state.propagationStopped,
    event,
  }
}
