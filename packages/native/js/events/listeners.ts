import type { EventHandlerMap, HostEventHandler, MutationRenderer } from "../host.js"
import { registerEventHandler, unregisterEventHandler } from "../renderer-state.js"
import { captureHandlerKey, isEnterLeaveType } from "./dispatch.js"

const CAPTURE_SUFFIX = "Capture"

/**
 * The type native listens for: a capture handler key without its `Capture`
 * suffix, so native hears of a type once while either phase has a handler.
 * Enter and leave keep the suffix: an ancestor hears them in the capture phase
 * only, and native has to tell its capture listener from its bubble one.
 */
export function nativeEventType(handlerKey: string): string {
  const eventType = withoutCaptureSuffix(handlerKey)
  return isEnterLeaveType(eventType) ? handlerKey : eventType
}

function withoutCaptureSuffix(handlerKey: string): string {
  return handlerKey.endsWith(CAPTURE_SUFFIX)
    ? handlerKey.slice(0, -CAPTURE_SUFFIX.length)
    : handlerKey
}

/** Whether a handler is registered that native hears of as `nativeType`. */
function listening(handlers: Map<string, HostEventHandler> | undefined, nativeType: string) {
  if (!handlers) return false
  if (handlers.has(nativeType)) return true
  return (
    !isEnterLeaveType(withoutCaptureSuffix(nativeType)) &&
    handlers.has(captureHandlerKey(nativeType))
  )
}

type ListenerRenderer = Pick<MutationRenderer, "setEventListener">

/**
 * Registers a handler under `handlerKey` (a bare type or its capture key).
 * Native is told about the type once, when the first of its two phases appears.
 */
export function registerEventListener(
  eventHandlers: EventHandlerMap,
  renderer: ListenerRenderer,
  elementId: number,
  handlerKey: string,
  handler: HostEventHandler
): void {
  const eventType = nativeEventType(handlerKey)
  const wasListening = listening(eventHandlers.get(elementId), eventType)
  registerEventHandler(eventHandlers, elementId, handlerKey, handler)
  if (!wasListening) renderer.setEventListener(elementId, eventType, true)
}

/** Removes a handler. Native is told only when its type has no handler in either phase. */
export function unregisterEventListener(
  eventHandlers: EventHandlerMap,
  renderer: ListenerRenderer,
  elementId: number,
  handlerKey: string
): void {
  if (!eventHandlers.get(elementId)?.has(handlerKey)) return
  const eventType = nativeEventType(handlerKey)
  unregisterEventHandler(eventHandlers, elementId, handlerKey)
  if (!listening(eventHandlers.get(elementId), eventType)) {
    renderer.setEventListener(elementId, eventType, false)
  }
}
