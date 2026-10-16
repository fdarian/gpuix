import type { EventPayload } from "../../index.js"
import type { NativeRenderer } from "../host.js"
import type { ActiveRoot, DismissLayer } from "../renderer-state.js"
import {
  createDomEvent,
  dispatchDomEvent,
  optionalElement,
  reportHandlerErrors,
  requireElement,
  toRootEvent,
  type DomDispatchResult,
  type DomEvent,
  type PublicEventFields,
  type RootEvent,
} from "./dispatch.js"
import { scrollKeyName } from "../scroll.js"
import { moveFocusByTab } from "../focus.js"
import { isDragEvent } from "./drag.js"
import { handlerEventType, hasDomListener, isOutsidePress } from "./route.js"
import { dispatchPropagating, document, isDocumentEvent, window } from "./window-target.js"

export interface RootEventContext {
  readonly renderer: NativeRenderer
  topLayer(): DismissLayer | undefined
  dispatchDrag(root: ActiveRoot, event: EventPayload): void
}

const notHandled: DomDispatchResult = { defaultPrevented: false, propagationStopped: false }

function hasCommandModifier(event: PublicEventFields): boolean {
  const modifiers = event.modifiers
  return Boolean(modifiers?.ctrl || modifiers?.alt || modifiers?.cmd)
}

const KEY_EVENT_TYPES: ReadonlySet<string> = new Set(["keydown", "keyup"])

const MOVE_EVENT_TYPES: ReadonlySet<string> = new Set(["pointermove", "mousemove"])

/** A move over no element: it has no path, and only `window` and `document` hear it. */
function isWindowMove(event: EventPayload): boolean {
  return event.path == null && MOVE_EVENT_TYPES.has(event.eventType)
}

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
  // A path whose target the root has dropped is an event native queued before
  // the element was destroyed: nobody can be told about it.
  if (event.path != null && event.path.length > 0 && root.elementById(event.path[0]) === undefined) {
    return false
  }
  // Document-level in the DOM, so no element id or listener flag: native
  // fires them only for a lock JS asked for or held.
  if (isDocumentEvent(event.eventType)) {
    return window.hasListeners(event.eventType) || document.hasListeners(event.eventType)
  }
  if (KEY_EVENT_TYPES.has(event.eventType)) {
    // Native aims a key at the window when nothing focused listens; that
    // address goes stale when the root is replaced.
    return event.path != null || event.elementId === root.windowKeyEventId
  }
  if (isDragEvent(event)) return true
  if (isWindowMove(event)) {
    return window.hasListeners(event.eventType) || document.hasListeners(event.eventType)
  }
  if (event.path == null || isOutsidePress(event.eventType)) {
    return root.eventHandlers.get(event.elementId)?.has(event.eventType) === true
  }
  return (
    hasDomListener(root.eventHandlers, event) ||
    window.hasListeners(event.eventType) ||
    document.hasListeners(event.eventType)
  )
}

/** An event of the window (selection, appearance), with `target` as the element when it names one. */
function windowLevelEvent(root: ActiveRoot, event: EventPayload): RootEvent {
  return { ...toRootEvent(root, event, optionalElement(root, event.target)), elementId: 0 }
}

export function runRootEvent(
  root: ActiveRoot,
  event: EventPayload,
  context: RootEventContext
): DomDispatchResult {
  if (event.eventType === "selectionChange") {
    root.handlers.onSelectionChange?.(windowLevelEvent(root, event), context.renderer)
    return notHandled
  }
  if (event.eventType === "appearanceChange") {
    root.handlers.onAppearanceChange?.(windowLevelEvent(root, event), context.renderer)
    return notHandled
  }
  if (isDocumentEvent(event.eventType)) {
    dispatchPropagating(root, event, root.scoped)
    return notHandled
  }
  if (isDragEvent(event)) {
    context.dispatchDrag(root, event)
    return notHandled
  }
  if (isOutsidePress(event.eventType)) {
    try {
      const path = (event.path ?? [event.elementId]).map((id) => requireElement(root, id, event.eventType))
      const outside = createDomEvent(root, event, path, null).event
      outside.currentTarget = requireElement(root, event.elementId, event.eventType)
      root.eventHandlers.get(event.elementId)?.get(event.eventType)?.(outside)
    } catch (error) {
      reportHandlerErrors(root, [error])
    }
    return notHandled
  }
  if (event.path == null && !KEY_EVENT_TYPES.has(event.eventType) && !isWindowMove(event)) {
    // Aimed at one element, with no path to walk, but still cancelable.
    return dispatchDomEvent(root, event)
  }
  const { event: domEvent, ...result } = dispatchPropagating(root, event, root.scoped)
  if (event.eventType === "keydown" && !result.defaultPrevented) {
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
    moveFocusByTab(context.renderer, Boolean(event.modifiers?.shift))
  } else {
    const scrollKey = scrollKeyName(event.key)
    if (scrollKey) context.renderer.scrollByKey?.(scrollKey, Boolean(event.modifiers?.shift))
  }
}
