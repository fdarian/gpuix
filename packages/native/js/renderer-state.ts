import type { EventPayload } from "../index.js"
import { dispatchDomEvent, type DomDispatchResult, type DomEvent } from "./events/dispatch.js"
import { createDragDispatcher, isDragEvent } from "./events/drag.js"
import { handlerEventType, hasDomListener } from "./events/route.js"
import { dispatchPropagating, GpuixEventTarget, window, document } from "./events/window-target.js"
import type {
  ElementIdAllocator,
  EventHandlerMap,
  HostEventHandler,
  KeyEvent,
  NativeRenderer,
  WindowEventHandler,
  WindowKeyEventHandler,
} from "./host.js"

export interface RendererRootHandlers {
  eventHandlers?: EventHandlerMap
  onWindowKeyDown?: WindowKeyEventHandler
  onWindowKeyUp?: WindowKeyEventHandler
  onSelectionChange?: WindowEventHandler
  onAppearanceChange?: WindowEventHandler
  onPointerLockChange?: WindowEventHandler
  onPointerLockError?: WindowEventHandler
  /** See `WindowKeyEventHandlers.tabNavigation`. Defaults to true. */
  tabNavigation?: boolean
  onEvent?: (event: EventPayload) => void
  schedule?: (dispatch: () => void) => void
}

export interface RendererRootBinding {
  readonly ids: ElementIdAllocator
  readonly eventHandlers: EventHandlerMap
  readonly windowKeyEventId: number
  readonly windowSelectionEventId: number
  readonly windowAppearanceEventId: number
  detach(): boolean
}

/** What a synchronous handler tells native about an event. */
export interface SyncDispatchResult {
  defaultPrevented: boolean
  propagationStopped: boolean
}

/**
 * One open overlay: a dialog, menu, popover or tooltip. Only the top layer
 * closes on Escape. A layer is above its parent and above every layer that
 * opened before it.
 */
export interface DismissLayer {
  /** The layer this one is nested in, such as the Dialog around a Select. */
  readonly parent?: DismissLayer
  /** Escape reached the window and no handler prevented it. */
  onEscapeKeyDown(event: KeyEvent): void
  /**
   * Element to focus when the layer opens, or null to leave focus. Called
   * only when the layer lands on top, so the innermost of several layers that
   * open together takes focus.
   */
  initialFocus?(): number | null
  /**
   * Element to focus when the layer closes, or null to leave focus.
   * `previous` is the focus from before the layer opened. Not called when the
   * parent layer closed first, because the parent restores for both.
   */
  finalFocus?(previous: number | null): number | null
}

export interface PushLayerOptions {
  /**
   * The focused element from before this layer's elements were committed.
   * Read it before the commit: once committed, an `autoFocus` inside the
   * layer may already have taken focus.
   */
  previousFocus?: number | null
}

interface LayerEntry {
  layer: DismissLayer
  previous: number | null
}

function isAncestor(ancestor: DismissLayer, layer: DismissLayer): boolean {
  for (let current = layer.parent; current; current = current.parent) {
    if (current === ancestor) return true
  }
  return false
}

export interface RendererState {
  readonly ids: ElementIdAllocator
  attach(handlers?: RendererRootHandlers): RendererRootBinding
  /**
   * Handles an event native queued. Returns whether anything was there to
   * handle it.
   */
  dispatch(event: EventPayload): boolean
  /**
   * Handles an event native is waiting on and returns what its handlers
   * decided. The handler for `setSyncEventHandler`. Everything `dispatch` does,
   * but now: native acts on the outcome as soon as this returns.
   */
  dispatchSync(event: EventPayload): SyncDispatchResult
  current(): RendererRootBinding | undefined
  /** Open a layer. Call the returned function when it closes. */
  pushLayer(layer: DismissLayer, options?: PushLayerOptions): () => void
}

type ActiveRoot = RendererRootBinding & {
  handlers: RendererRootHandlers
  /** Listens at the window level for this root only: the `onKeyDown` / `onKeyUp` options. */
  scoped: GpuixEventTarget
}

const STATE_KEY = Symbol.for("@gpuix/native/renderer-states")

/** Register an open overlay on this renderer's layer stack. */
export function pushDismissLayer(
  renderer: NativeRenderer,
  layer: DismissLayer,
  options?: PushLayerOptions
): () => void {
  return createRendererState(renderer).pushLayer(layer, options)
}

function allStates(): WeakMap<NativeRenderer, RendererState> {
  const existing = Reflect.get(globalThis, STATE_KEY) as
    | WeakMap<NativeRenderer, RendererState>
    | undefined
  if (existing) return existing
  const states = new WeakMap<NativeRenderer, RendererState>()
  Reflect.set(globalThis, STATE_KEY, states)
  return states
}

function hasCommandModifier(event: EventPayload): boolean {
  const modifiers = event.modifiers
  return Boolean(modifiers?.ctrl || modifiers?.alt || modifiers?.cmd)
}

const KEY_EVENT_TYPES: ReadonlySet<string> = new Set(["keyDown", "keyUp"])

/**
 * Native may still emit the old camelCase pointer names; handlers are
 * registered under the DOM's lowercase ones.
 */
function normalizeEventType(event: EventPayload): EventPayload {
  const eventType = handlerEventType(event.eventType)
  return eventType === event.eventType ? event : { ...event, eventType }
}

/** Whether `event` was meant for the live root, and something is there to hear it. */
function handlesEvent(root: ActiveRoot, event: EventPayload): boolean {
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
  if (event.path == null) {
    return root.eventHandlers.get(event.elementId)?.has(event.eventType) === true
  }
  return (
    hasDomListener(root.eventHandlers, event) ||
    window.hasListeners(event.eventType) ||
    document.hasListeners(event.eventType)
  )
}

export function createRendererState(renderer: NativeRenderer): RendererState {
  const existing = allStates().get(renderer)
  if (existing) return existing

  const ids = { nextElementId: 0 }
  let generation = 0
  let active: ActiveRoot | undefined
  const layers: LayerEntry[] = []
  const dispatchDrag = createDragDispatcher(() => {
    if (!renderer.cancelDrag) throw new Error("This renderer does not support cancelDrag")
    renderer.cancelDrag()
  })
  const notHandled: DomDispatchResult = { defaultPrevented: false, propagationStopped: false }

  const runEvent = (root: ActiveRoot, event: EventPayload): DomDispatchResult => {
    if (event.eventType === "selectionChange") {
      root.handlers.onSelectionChange?.({ ...event, elementId: 0 }, renderer)
      return notHandled
    }
    if (event.eventType === "appearanceChange") {
      root.handlers.onAppearanceChange?.({ ...event, elementId: 0 }, renderer)
      return notHandled
    }
    if (event.eventType === "pointerlockchange") {
      root.handlers.onPointerLockChange?.({ ...event, elementId: 0 }, renderer)
      return notHandled
    }
    if (event.eventType === "pointerlockerror") {
      root.handlers.onPointerLockError?.({ ...event, elementId: 0 }, renderer)
      return notHandled
    }
    if (isDragEvent(event)) {
      dispatchDrag(root, event)
      return notHandled
    }
    if (event.path == null && !KEY_EVENT_TYPES.has(event.eventType)) {
      // Aimed at one element, with no path to walk, but still cancelable.
      return dispatchDomEvent(root, event)
    }
    const { event: domEvent, ...result } = dispatchPropagating(root, event, root.scoped)
    if (event.eventType === "keyDown" && !result.defaultPrevented) {
      runKeyDefault(root, domEvent)
    }
    return result
  }

  // The default actions of a key run after every handler had the chance to
  // call preventDefault().
  const runKeyDefault = (root: ActiveRoot, event: DomEvent) => {
    if (hasCommandModifier(event)) return
    if (event.key === "escape") {
      layers.at(-1)?.layer.onEscapeKeyDown(event)
    } else if (event.key === "tab" && root.handlers.tabNavigation !== false) {
      if (event.modifiers?.shift) renderer.focusPrevious?.()
      else renderer.focusNext?.()
    }
  }

  const state: RendererState = {
    ids,
    attach(handlers = {}) {
      if (active) {
        throw new Error(
          "This renderer already drives a mounted GPUIX root. One renderer owns one window, one native root id, and one event map, so a second root would silently take both over. Unmount the first root first."
        )
      }
      generation += 1
      const eventHandlers = handlers.eventHandlers ?? new Map()
      const scoped = new GpuixEventTarget()
      if (handlers.onWindowKeyDown) {
        const onKeyDown = handlers.onWindowKeyDown
        scoped.addEventListener("keydown", (event) => onKeyDown(event, renderer))
      }
      if (handlers.onWindowKeyUp) {
        const onKeyUp = handlers.onWindowKeyUp
        scoped.addEventListener("keyup", (event) => onKeyUp(event, renderer))
      }
      const binding: ActiveRoot = {
        ids,
        handlers,
        eventHandlers,
        scoped,
        windowKeyEventId: generation,
        windowSelectionEventId: generation,
        windowAppearanceEventId: generation,
        detach() {
          if (active !== binding) return false
          active = undefined
          return true
        },
      }
      active = binding
      return binding
    },
    dispatch(received) {
      const root = active
      if (!root) return false
      const event = normalizeEventType(received)
      if (!handlesEvent(root, event)) return false
      const run = () => {
        runEvent(root, event)
        root.handlers.onEvent?.(received)
      }
      if (root.handlers.schedule) root.handlers.schedule(run)
      else run()
      return true
    },
    dispatchSync(received) {
      const root = active
      const event = normalizeEventType(received)
      if (!root || !handlesEvent(root, event)) return notHandled
      const result = runEvent(root, event)
      root.handlers.onEvent?.(received)
      return result
    },
    current: () => active,
    pushLayer(layer, options = {}) {
      const entry: LayerEntry = {
        layer,
        previous: options.previousFocus ?? renderer.getFocusedElementId?.() ?? null,
      }
      // A framework can open a nested layer before its parent in one commit
      // (React runs child effects first), so nesting decides, not call order.
      const firstChild = layers.findIndex((open) => isAncestor(layer, open.layer))
      if (firstChild < 0) {
        layers.push(entry)
        const target = layer.initialFocus?.() ?? null
        if (target !== null) renderer.focusElement?.(target)
      } else {
        layers.splice(firstChild, 0, entry)
        // The children opened in the same commit and captured the same
        // pre-open focus. Closing one alone must return into this layer.
        const inside = layer.initialFocus?.() ?? null
        for (const child of layers.slice(firstChild + 1)) {
          if (isAncestor(layer, child.layer) && child.previous === entry.previous) {
            child.previous = inside
          }
        }
      }
      return () => {
        const index = layers.indexOf(entry)
        if (index < 0) return
        layers.splice(index, 1)
        // A parent that closed first already restored focus for its subtree.
        if (layer.parent && !layers.some((open) => open.layer === layer.parent)) return
        const target = layer.finalFocus?.(entry.previous) ?? null
        if (target !== null) renderer.focusElement?.(target)
      }
    },
  }
  allStates().set(renderer, state)
  return state
}

export function registerEventHandler(
  eventHandlers: EventHandlerMap,
  elementId: number,
  eventType: string,
  handler: HostEventHandler
): void {
  const handlers = eventHandlers.get(elementId) ?? new Map()
  handlers.set(eventType, handler)
  eventHandlers.set(elementId, handlers)
}

export function unregisterEventHandler(
  eventHandlers: EventHandlerMap,
  elementId: number,
  eventType: string
): void {
  const handlers = eventHandlers.get(elementId)
  if (!handlers) return
  handlers.delete(eventType)
  if (handlers.size === 0) eventHandlers.delete(elementId)
}

export function unregisterEventHandlers(
  eventHandlers: EventHandlerMap,
  elementId: number
): void {
  eventHandlers.delete(elementId)
}

/** Anything a framework hands out for a mounted host element. */
export interface FocusableNode {
  readonly id: number
}

/**
 * Where an overlay moves focus when it opens or closes, like Base UI's
 * `initialFocus` / `finalFocus`:
 *
 * - `true` or unset: the overlay's default target
 * - `false`: focus does not move
 * - an element or a ref to one: that element
 * - a function: returns one of the above; `null` means the default
 */
type FocusTargetValue<Node extends FocusableNode> =
  | boolean
  | Node
  | { readonly current: Node | null }

export type FocusTarget<Node extends FocusableNode = FocusableNode> =
  | FocusTargetValue<Node>
  | (() => FocusTargetValue<Node> | null | undefined)

/** Resolve a `FocusTarget` to an element id, or null when focus must stay. */
export function resolveFocusTarget<Node extends FocusableNode>(
  target: FocusTarget<Node> | undefined,
  fallback: () => number | null
): number | null {
  const value = typeof target === "function" ? target() : target
  if (value === undefined && typeof target === "function") return null
  if (value === undefined || value === null || value === true) return fallback()
  if (value === false) return null
  if ("current" in value) return value.current?.id ?? fallback()
  return value.id
}
