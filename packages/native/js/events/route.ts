import type { EventPayload } from "../../index.js"
import type { EventHandlerMap } from "../host.js"
import { captureHandlerKey, eventBubbles } from "./dispatch.js"
import { DOM_DISPATCHED_EVENT_TYPES } from "./event-props.js"

/** gpuix's own outside-press event: not DOM. Native delivers it to the listening element alone, with the press target in `target`. */
const MOUSE_DOWN_OUTSIDE = "mousedownoutside"

/** The outside press carries the DOM path of its press target, but is not walked along it. */
export function isOutsidePress(eventType: string): boolean {
  return eventType === MOUSE_DOWN_OUTSIDE
}

/**
 * Handler-map type for an event from native. Native may still emit the old
 * camelCase names (`mouseDown`); handlers are registered under lowercase DOM
 * names, so the two are matched case-insensitively. Other types are untouched.
 */
export function handlerEventType(eventType: string): string {
  const lower = eventType.toLowerCase()
  return DOM_DISPATCHED_EVENT_TYPES.has(lower) || lower === MOUSE_DOWN_OUTSIDE ? lower : eventType
}

/**
 * Whether anything on the event's path has a handler it would run: a handler of
 * either phase anywhere, except for the types that do not bubble, where an
 * ancestor counts only for a capture handler.
 */
export function hasDomListener(eventHandlers: EventHandlerMap, event: EventPayload): boolean {
  const captureKey = captureHandlerKey(event.eventType)
  const path = event.path ?? [event.elementId]
  const bubbles = eventBubbles(event.eventType)
  return path.some((id, index) => {
    const handlers = eventHandlers.get(id)
    if (!handlers) return false
    if (handlers.has(captureKey)) return true
    return (bubbles || index === 0) && handlers.has(event.eventType)
  })
}
