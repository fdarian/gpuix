import type { EventPayload } from "../../index.js"
import type { Animation } from "../web-animations.js"
import {
  createDomEvent,
  EVENT_PHASE_AT_TARGET,
  EVENT_PHASE_BUBBLING,
  EVENT_PHASE_CAPTURING,
  EVENT_PHASE_NONE,
  dispatchDomEvent,
  type DomDispatchResult,
  type DomEvent,
  type DomEventRoot,
  type DomEventState,
  reportHandlerErrors,
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

  /** @param onTypesChange Called when a type gains its first listener or loses its last. */
  constructor(private readonly onTypesChange?: () => void) {}

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
    if (registrations.length === 1) this.onTypesChange?.()
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
    if (registrations.length === 0) {
      this.registrations.delete(key)
      this.onTypesChange?.()
    }
  }

  /** Whether any listener is registered for `type` in either phase. */
  hasListeners(type: string): boolean {
    return this.registrations.has(type.toLowerCase())
  }

  /** @internal The types with a listener in either phase. */
  listenedTypes(): string[] {
    return [...this.registrations.keys()]
  }

  /**
   * @internal Calls the listeners of one phase. A listener that throws does not
   * end the phase: its error goes to `fail`.
   */
  run(
    event: DomEvent,
    state: DomEventState,
    capture: boolean,
    phase: number,
    fail: (error: unknown) => void
  ): void {
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
      try {
        entry.listener(event)
      } catch (error) {
        fail(error)
      }
    }
  }
}

const typeSubscribers = new Set<() => void>()
const notifyTypeSubscribers = () => {
  for (const subscriber of [...typeSubscribers]) subscriber()
}

/**
 * What a root tells the rest of the runtime about its elements: `document` asks
 * it for `activeElement`, and event dispatch asks it for `target`,
 * `currentTarget` and `relatedTarget`.
 */
type ElementSource = {
  /** Id of the element that holds focus in the root's window, or `null`. */
  focusedId(): number | null
  /** Id of the element that holds the pointer lock in the root's window, or `null`. */
  pointerLockId(): number | null
  /** The object the adapter hands out for that element (a ref's value), or `undefined` once the root has dropped it. */
  elementById(id: number): object | undefined
}

type ElementSourceEntry = {
  source: ElementSource
  /** The renderer's window was replaced or closed: it can still name elements but reports no focus and lists no animations. */
  windowGone: boolean
  /** Order in which the roots last heard a focus event; higher is more recent. */
  focusedAt: number
}

/**
 * Kept on `globalThis`, like the renderer states: under `bun --hot` this module
 * is evaluated again while a renderer's state, which asks it for elements on
 * every event, stays from the first evaluation.
 */
const ELEMENT_SOURCES_KEY = Symbol.for("@gpuix/native/element-sources")
const elementSources: Map<object, ElementSourceEntry> = (() => {
  const existing = Reflect.get(globalThis, ELEMENT_SOURCES_KEY) as Map<object, ElementSourceEntry> | undefined
  if (existing) return existing
  const created = new Map<object, ElementSourceEntry>()
  Reflect.set(globalThis, ELEMENT_SOURCES_KEY, created)
  return created
})()
let focusClock = 0

/**
 * The `document` of a GPUIX app: the `window` target plus `activeElement` and
 * `pointerLockElement`.
 *
 * `Element` is what the adapter hands out for an element: React's host
 * instance, Solid's element.
 */
export class GpuixDocument<Element = unknown> extends GpuixEventTarget {
  /**
   * The focused element, the same object a `ref` gives, or `null` when nothing
   * is focused. With several windows (one root each), the window that holds
   * focus decides: of the roots that have a focused element, the one that heard
   * a focus event last.
   */
  get activeElement(): Element | null {
    let chosen: { entry: ElementSourceEntry; id: number } | undefined
    for (const entry of elementSources.values()) {
      if (entry.windowGone) continue
      const id = entry.source.focusedId()
      if (id === null) continue
      if (!chosen || entry.focusedAt > chosen.entry.focusedAt) chosen = { entry, id }
    }
    if (!chosen) return null
    const element = chosen.entry.source.elementById(chosen.id)
    if (element === undefined) {
      throw new Error(`Element ${chosen.id} holds focus but its root has no element for it`)
    }
    return element as Element
  }

  /**
   * The element that holds the pointer lock, the same object a `ref` gives, or
   * `null` while none does. Where the platform can lock (macOS) it is held as
   * soon as `requestPointerLock()` returns, before `pointerlockchange` arrives.
   */
  get pointerLockElement(): Element | null {
    for (const entry of elementSources.values()) {
      if (entry.windowGone) continue
      const id = entry.source.pointerLockId()
      if (id === null) continue
      const element = entry.source.elementById(id)
      if (element === undefined) {
        throw new Error(`Element ${id} holds the pointer lock but its root has no element for it`)
      }
      return element as Element
    }
    return null
  }

  /**
   * The CSS transitions, then the CSS animations, of every window, as `Animation` objects: for
   * each renderer in the order its root was installed, those of its elements in document order
   * (every transition, then every animation). The same objects `element.getAnimations()` gives.
   */
  getAnimations(): Animation[] {
    const list = Reflect.get(globalThis, DOCUMENT_ANIMATIONS_KEY) as ((renderer: object) => Animation[]) | undefined
    if (!list) throw new Error("Web Animations are not loaded: import @gpuix/native/host before document.getAnimations()")
    return [...elementSources].flatMap(([renderer, entry]) => (entry.windowGone ? [] : list(renderer)))
  }
}

/**
 * Kept on `globalThis` like the element sources: the module that makes `Animation` objects
 * (`web-animations`) imports this one, so it hands `document` the way to list a renderer's.
 */
const DOCUMENT_ANIMATIONS_KEY = Symbol.for("@gpuix/native/document-animations")

/** @internal Called by `web-animations` once, with how to list the animations of one renderer's window. */
export function setDocumentAnimations(list: (renderer: object) => Animation[]): void {
  Reflect.set(globalThis, DOCUMENT_ANIMATIONS_KEY, list)
}

export const window = new GpuixEventTarget(notifyTypeSubscribers)
export const document = new GpuixDocument(notifyTypeSubscribers)

/**
 * Lets the runtime see the elements of the root that drives `renderer`, through
 * `elementById`: the object the adapter gives for an element id, as a `ref`
 * does. `document.activeElement` and the `target`, `currentTarget` and
 * `relatedTarget` of every event read it. The returned function takes the root
 * out again. A renderer that cannot report focus still names its elements; it
 * just never has an `activeElement`.
 */
export function installElementSource(
  renderer: { getFocusedElementId?: () => number | null; pointerLockElement?: () => number | null },
  elementById: (id: number) => object | undefined
): () => void {
  const getFocusedElementId = renderer.getFocusedElementId
  const getPointerLockElement = renderer.pointerLockElement
  const entry: ElementSourceEntry = {
    source: {
      focusedId: () => (getFocusedElementId ? getFocusedElementId.call(renderer) : null),
      pointerLockId: () => (getPointerLockElement ? getPointerLockElement.call(renderer) : null),
      elementById,
    },
    windowGone: false,
    focusedAt: 0,
  }
  elementSources.set(renderer, entry)
  return () => {
    if (elementSources.get(renderer) === entry) elementSources.delete(renderer)
  }
}

/** Drops the root of `renderer` whose window is gone, so nothing asks it for elements. */
export function forgetElementSource(renderer: object): void {
  elementSources.delete(renderer)
}

/**
 * Stops asking `renderer` what is focused or animating, for a renderer whose
 * window another took over or that was closed: its root still names its
 * elements, but the window it would ask about is not its own any more.
 */
export function forgetWindowSource(renderer: object): void {
  const entry = elementSources.get(renderer)
  if (entry) entry.windowGone = true
}

/**
 * @internal The element `renderer`'s root has for `id`, or `undefined` once it
 * dropped it. Throws when no adapter installed a source for `renderer`: the
 * ids of its events cannot be turned into elements.
 */
export function elementOf(renderer: object, id: number): object | undefined {
  const entry = elementSources.get(renderer)
  if (!entry) {
    throw new Error(
      "No element source is installed for this renderer: an adapter's root installs one with installElementSource before events arrive."
    )
  }
  return entry.source.elementById(id)
}

/** @internal Records that `owner`'s window just moved focus, for the tie-break of `activeElement`. */
export function noteFocusEvent(owner: object): void {
  const entry = elementSources.get(owner)
  if (entry) entry.focusedAt = ++focusClock
}

/**
 * Keeps `renderer` told which event types `window` or `document` listens for,
 * so native produces them even when no element on the event's path does (the
 * pointer over bare background). Sends the current set now and again whenever
 * it changes. The returned function clears it on the renderer and stops.
 */
export function syncWindowEventListeners(renderer: {
  setWindowEventListeners?: (types: string[]) => void
}): () => void {
  const send = () => {
    renderer.setWindowEventListeners?.([
      ...new Set([...window.listenedTypes(), ...document.listenedTypes()]),
    ])
  }
  send()
  typeSubscribers.add(send)
  return () => {
    typeSubscribers.delete(send)
    renderer.setWindowEventListeners?.([])
  }
}

const DOCUMENT_EVENT_TYPES: ReadonlySet<string> = new Set(["pointerlockchange", "pointerlockerror"])

/** Whether the DOM aims the event at `document` itself, with no element on the path. */
export function isDocumentEvent(eventType: string): boolean {
  return DOCUMENT_EVENT_TYPES.has(eventType)
}

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
  // When `document` is the target, both its capture and bubble listeners run at the target.
  const documentPhase = (event: DomEvent, phase: number) =>
    event.target === document ? EVENT_PHASE_AT_TARGET : phase
  const bubble = (event: DomEvent, state: DomEventState, fail: (error: unknown) => void) => {
    document.run(event, state, false, documentPhase(event, EVENT_PHASE_BUBBLING), fail)
    if (!state.propagationStopped) window.run(event, state, false, EVENT_PHASE_BUBBLING, fail)
    if (!state.propagationStopped) scoped?.run(event, state, false, EVENT_PHASE_BUBBLING, fail)
  }
  const capture = (event: DomEvent, state: DomEventState, fail: (error: unknown) => void) => {
    window.run(event, state, true, EVENT_PHASE_CAPTURING, fail)
    if (!state.propagationStopped) {
      document.run(event, state, true, documentPhase(event, EVENT_PHASE_CAPTURING), fail)
    }
  }

  if (payload.path && payload.path.length > 0) {
    let seen: DomEvent | undefined
    const result = dispatchDomEvent(root, payload, {
      beforeCapture(event, state, fail) {
        seen = event
        capture(event, state, fail)
      },
      afterBubble: bubble,
    })
    if (!seen) throw new Error(`Event "${payload.eventType}" was dispatched without an event object.`)
    return { ...result, event: seen }
  }

  const { event, state } = createDomEvent(
    root,
    payload,
    [],
    isDocumentEvent(payload.eventType) ? document : null
  )
  const errors: unknown[] = []
  const fail = (error: unknown) => errors.push(error)
  capture(event, state, fail)
  if (event.bubbles && !state.propagationStopped) bubble(event, state, fail)
  event.currentTarget = null
  event.eventPhase = EVENT_PHASE_NONE
  reportHandlerErrors(root, errors)
  return {
    defaultPrevented: event.defaultPrevented,
    propagationStopped: state.propagationStopped,
    event,
  }
}
