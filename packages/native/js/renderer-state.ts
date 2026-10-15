import type { EventPayload } from "../index.js"
import { reportUncaught, type DomDispatchResult } from "./events/dispatch.js"
import { createDragDispatcher } from "./events/drag.js"
import { handlesEvent, normalizeEventType, runRootEvent } from "./events/keyboard-defaults.js"
import { GpuixEventTarget } from "./events/window-target.js"
import { deliverObservation } from "./observers.js"
import type {
  ElementIdAllocator,
  EventHandlerMap,
  HostEventHandler,
  KeyEvent,
  NativeRenderer,
  WindowEventHandler,
  WindowKeyEventHandler,
} from "./host.js"
import { shapeScrollEvent } from "./scroll.js"

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
  /**
   * Wraps each dispatch, for a framework that must flush around it. Synchronous
   * events need their outcome before native continues, so it has to call
   * `dispatch` before it returns.
   */
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

/**
 * Sends an error thrown while a synchronous handler ran to `report`, and tells
 * native the event was not handled. Errors a DOM handler throws never get this
 * far: the dispatch reports them itself and carries on.
 */
export function reportSyncErrors(
  dispatch: (event: EventPayload) => SyncDispatchResult,
  report: (error: unknown) => void
): (event: EventPayload) => SyncDispatchResult {
  return (event) => {
    try {
      return dispatch(event)
    } catch (error) {
      report(error)
      return { defaultPrevented: false, propagationStopped: false }
    }
  }
}

/** What a motion value hears from the renderer's per-frame sampling. */
export interface MotionValueSink {
  change(value: unknown): void
  animationStart(): void
  animationComplete(): void
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
  /**
   * Where an error thrown by an event handler goes: the renderer's `onError`.
   * Without one it is reported as uncaught, like a DOM listener's.
   */
  reportError(error: unknown): void
  setErrorHandler(onError: ((error: unknown) => void) | undefined): void
  current(): RendererRootBinding | undefined
  /** Open a layer. Call the returned function when it closes. */
  pushLayer(layer: DismissLayer, options?: PushLayerOptions): () => void
  /**
   * Listen for the window environment `matchMedia` reads changing: the OS
   * accessibility preferences, or the appearance. Independent of any mounted
   * root. The renderer sends `mediaChange` only while a listener exists.
   */
  subscribeMedia(listener: () => void): () => void
  /**
   * Receive the `motionValue*` events of one value. Independent of any mounted
   * root. A renderer holds one sink per value id.
   */
  subscribeMotionValue(id: number, sink: MotionValueSink): () => void
}

export type ActiveRoot = RendererRootBinding & {
  handlers: RendererRootHandlers
  /** Where an error a handler of this root throws goes. See `RendererState.reportError`. */
  reportError(error: unknown): void
  /** Listens at the window level for this root only: the `onKeyDown` / `onKeyUp` options. */
  scoped: GpuixEventTarget
}

const STATE_KEY = Symbol.for("@gpuix/native/renderer-states")
const MOUNTED_KEY = Symbol.for("@gpuix/native/mounted-renderer")

/** Mount order, oldest first. Shared through `globalThis` like the renderer states. */
function mountedRenderers(): NativeRenderer[] {
  const existing = Reflect.get(globalThis, MOUNTED_KEY) as NativeRenderer[] | undefined
  if (existing) return existing
  const created: NativeRenderer[] = []
  Reflect.set(globalThis, MOUNTED_KEY, created)
  return created
}

/**
 * The renderer of the most recently mounted root that is still mounted, which
 * is "the window" for code that has no component to ask, such as the global
 * `matchMedia`.
 */
export function mountedRenderer(): NativeRenderer | undefined {
  return mountedRenderers().at(-1)
}

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

  const runEvent = (root: ActiveRoot, event: EventPayload): DomDispatchResult =>
    runRootEvent(root, event, {
      renderer,
      topLayer: () => layers.at(-1)?.layer,
      dispatchDrag,
    })

  let onError: ((error: unknown) => void) | undefined
  const mediaListeners = new Set<() => void>()
  const motionValueSinks = new Map<number, MotionValueSink>()

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
        reportError: (error) => state.reportError(error),
        scoped,
        windowKeyEventId: generation,
        windowSelectionEventId: generation,
        windowAppearanceEventId: generation,
        detach() {
          if (active !== binding) return false
          active = undefined
          const mounted = mountedRenderers()
          mounted.splice(mounted.indexOf(renderer), 1)
          return true
        },
      }
      active = binding
      mountedRenderers().push(renderer)
      return binding
    },
    dispatch(received) {
      if (received.eventType === "mediaChange") {
        for (const listener of [...mediaListeners]) listener()
        return mediaListeners.size > 0
      }
      if (received.eventType.startsWith("motionValue")) {
        const sink = motionValueSinks.get(received.elementId)
        if (!sink) return false
        switch (received.eventType) {
          case "motionValueChange":
            if (received.value === undefined) {
              throw new Error("A motionValueChange event arrived without its value")
            }
            sink.change(JSON.parse(received.value))
            return true
          case "motionValueAnimationStart":
            sink.animationStart()
            return true
          case "motionValueAnimationComplete":
            sink.animationComplete()
            return true
        }
      }
      const root = active
      if (!root) return false
      const event = shapeScrollEvent(normalizeEventType(received))
      if (deliverObservation(renderer, event)) return true
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
      if (!root || !handlesEvent(root, event)) {
        return { defaultPrevented: false, propagationStopped: false }
      }
      let result: SyncDispatchResult | undefined
      const run = () => {
        result = runEvent(root, event)
        root.handlers.onEvent?.(received)
      }
      if (root.handlers.schedule) root.handlers.schedule(run)
      else run()
      if (!result) {
        throw new Error(
          "The schedule handler did not run the dispatch before returning. A synchronous event needs its outcome now, so schedule must call it synchronously."
        )
      }
      return result
    },
    reportError: (error) => (onError ?? reportUncaught)(error),
    setErrorHandler(handler) {
      onError = handler
    },
    current: () => active,
    subscribeMedia(listener) {
      mediaListeners.add(listener)
      if (mediaListeners.size === 1) {
        try {
          renderer.setWindowMediaChange?.(true)
        } catch (error) {
          mediaListeners.delete(listener)
          throw error
        }
      }
      return () => {
        if (!mediaListeners.delete(listener)) return
        if (mediaListeners.size === 0) renderer.setWindowMediaChange?.(false)
      }
    },
    subscribeMotionValue(id, sink) {
      if (motionValueSinks.has(id)) {
        throw new Error(`Motion value ${id} already has a sink on this renderer`)
      }
      motionValueSinks.set(id, sink)
      return () => {
        if (motionValueSinks.get(id) === sink) motionValueSinks.delete(id)
      }
    },
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
