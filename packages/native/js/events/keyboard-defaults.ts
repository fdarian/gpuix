import type { EventPayload } from "../../index.js"
import type { NativeRenderer } from "../host.js"
import type { ActiveRoot, DismissLayer } from "../renderer-state.js"
import {
  dispatchDomEvent,
  type DomDispatchResult,
  type DomEvent,
} from "./dispatch.js"
import { scrollKeyName } from "../scroll.js"
import { isDragEvent } from "./drag.js"
import { handlerEventType, hasDomListener, isOutsidePress } from "./route.js"
import { dispatchPropagating, document, window } from "./window-target.js"

export interface RootEventContext {
  readonly renderer: NativeRenderer
  topLayer(): DismissLayer | undefined
  dispatchDrag(root: ActiveRoot, event: EventPayload): void
}

const notHandled: DomDispatchResult = { defaultPrevented: false, propagationStopped: false }

function hasCommandModifier(event: EventPayload): boolean {
  const modifiers = event.modifiers
  return Boolean(modifiers?.ctrl || modifiers?.alt || modifiers?.cmd)
}

const KEY_EVENT_TYPES: ReadonlySet<string> = new Set(["keyDown", "keyUp"])

/**
 * Native may still emit the old camelCase pointer names; handlers are
 * registered under the DOM's lowercase ones.
 */
export function normalizeEventType(event: EventPayload): EventPayload {
  const eventType = handlerEventType(event.eventType)
  return eventType === event.eventType ? event : { ...event, eventType }
}

/** Whether `event` was meant for the live root, and something is there to hear it. */
export function handlesEvent(root: ActiveRoot, event: EventPayload): boolean {
  if (event.eventType === "selectionChange") {
    return event.elementId === root.windowSelectionEventId && Boolean(root.handlers.onSelectionChange)
  }
  if (event.eventType === "appearanceChange") {
    return event.elementId === root.windowAppearanceEventId && Boolean(root.handlers.onAppearanceChange)
  }
  // Document-level in the DOM, so no element id or listener flag: native
  // fires them only for a lock JS asked for or held.
  if (event.eventType === "pointerlockchange") return Boolean(root.handlers.onPointerLockChange)
  if (event.eventType === "pointerlockerror") return Boolean(root.handlers.onPointerLockError)
  if (KEY_EVENT_TYPES.has(event.eventType)) {
    // Native aims a key at the window when nothing focused listens; that
    // address goes stale when the root is replaced.
    return event.path != null || event.elementId === root.windowKeyEventId
  }
  if (isDragEvent(event)) return true
  if (event.path == null || isOutsidePress(event.eventType)) {
    return root.eventHandlers.get(event.elementId)?.has(event.eventType) === true
  }
  return (
    hasDomListener(root.eventHandlers, event) ||
    window.hasListeners(event.eventType) ||
    document.hasListeners(event.eventType)
  )
}

export function runRootEvent(
  root: ActiveRoot,
  event: EventPayload,
  context: RootEventContext
): DomDispatchResult {
  if (event.eventType === "selectionChange") {
    root.handlers.onSelectionChange?.({ ...event, elementId: 0 }, context.renderer)
    return notHandled
  }
  if (event.eventType === "appearanceChange") {
    root.handlers.onAppearanceChange?.({ ...event, elementId: 0 }, context.renderer)
    return notHandled
  }
  if (event.eventType === "pointerlockchange") {
    root.handlers.onPointerLockChange?.({ ...event, elementId: 0 }, context.renderer)
    return notHandled
  }
  if (event.eventType === "pointerlockerror") {
    root.handlers.onPointerLockError?.({ ...event, elementId: 0 }, context.renderer)
    return notHandled
  }
  if (isDragEvent(event)) {
    context.dispatchDrag(root, event)
    return notHandled
  }
  if (isOutsidePress(event.eventType)) {
    root.eventHandlers.get(event.elementId)?.get(event.eventType)?.(event)
    return notHandled
  }
  if (event.path == null && !KEY_EVENT_TYPES.has(event.eventType)) {
    // Aimed at one element, with no path to walk, but still cancelable.
    return dispatchDomEvent(root, event)
  }
  const { event: domEvent, ...result } = dispatchPropagating(root, event, root.scoped)
  if (event.eventType === "keyDown" && !result.defaultPrevented) {
    runKeyDefault(root, domEvent, context)
  }
  return result
}

// The default actions of a key run after every handler had the chance to
// call preventDefault().
function runKeyDefault(root: ActiveRoot, event: DomEvent, context: RootEventContext) {
  if (hasCommandModifier(event)) return
  if (event.key === "Escape") {
    context.topLayer()?.onEscapeKeyDown(event)
  } else if (event.key === "Tab" && root.handlers.tabNavigation !== false) {
    if (event.modifiers?.shift) context.renderer.focusPrevious?.()
    else context.renderer.focusNext?.()
  } else {
    const scrollKey = scrollKeyName(event.key)
    if (scrollKey) context.renderer.scrollByKey?.(scrollKey, Boolean(event.modifiers?.shift))
  }
}
