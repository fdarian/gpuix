import type { EventPayload } from "../../index.js"
import type { EventHandlerMap } from "../host.js"
import { captureHandlerKey } from "./dispatch.js"
import { POINTER_EVENT_TYPES } from "./event-props.js"

/** gpuix's own outside-press event: not DOM, no capture phase, no propagation path. */
const MOUSE_DOWN_OUTSIDE = "mousedownoutside"

/**
 * Handler-map type for an event from native. Native may still emit the old
 * camelCase names (`mouseDown`); handlers are registered under lowercase DOM
 * names, so the two are matched case-insensitively. Other types are untouched.
 */
export function handlerEventType(eventType: string): string {
  const lower = eventType.toLowerCase()
  return POINTER_EVENT_TYPES.has(lower) || lower === MOUSE_DOWN_OUTSIDE ? lower : eventType
}

/** The payload to hand to `dispatchDomEvent`, or undefined for types that do not propagate. */
export function toDomEvent(event: EventPayload): EventPayload | undefined {
  const eventType = handlerEventType(event.eventType)
  if (!POINTER_EVENT_TYPES.has(eventType)) return undefined
  return eventType === event.eventType ? event : { ...event, eventType }
}

/** Whether any element on the event's path has a handler in either phase. */
export function hasDomListener(eventHandlers: EventHandlerMap, event: EventPayload): boolean {
  const captureKey = captureHandlerKey(event.eventType)
  return (event.path ?? [event.elementId]).some((id) => {
    const handlers = eventHandlers.get(id)
    return Boolean(handlers?.has(event.eventType) || handlers?.has(captureKey))
  })
}
