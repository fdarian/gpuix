import { POINTER_EVENT_HANDLER_PROPS } from "./events/event-props.js"
import { registerMutationQueue } from "./bounds.js"
import { createBlobImageLoader, type BlobImageHost } from "./blob-images.js"
import type { MutationHost, MutationRenderer } from "./host.js"

export type MutationValue = number | string | boolean | object | null
export type MutationTuple = [string, ...MutationValue[]]

let appliedBatches = 0

/**
 * How many batches of mutations have reached a renderer, across every queue.
 * It moves whenever the tree changes, which makes it the signal that an app is
 * still working: `collectGarbageWhenIdle` waits for it to hold still.
 */
export function appliedBatchCount(): number {
  return appliedBatches
}

/** Atomic mutation queue shared by all framework adapters. */
export interface MutationQueue extends MutationRenderer {
  readonly pending: number
}

export function createMutationQueue(
  renderer: MutationHost & BlobImageHost,
  onDestroyed: (ids: readonly number[]) => void = () => {},
  scheduleFlush?: (flush: () => void) => void
): MutationQueue {
  let queue: MutationTuple[] = []
  let flushScheduled = false

  const enqueue = (mutation: MutationTuple) => {
    queue.push(mutation)
    if (!scheduleFlush || flushScheduled) return
    flushScheduled = true
    scheduleFlush(() => {
      if (!flushScheduled) return
      flushScheduled = false
      mutations.flushMutations()
    })
  }

  const blobImages = createBlobImageLoader(renderer, {
    flush: () => mutations.flushMutations(),
    fail: (id, message, ticket) => {
      enqueue(["setCustomProp", id, "srcError", { message, ticket }])
      if (!scheduleFlush) mutations.flushMutations()
    },
  })

  const mutations: MutationQueue = {
    get pending() {
      return queue.length
    },
    createElement(id, elementType) {
      blobImages.created(id, elementType)
      enqueue(["createElement", id, elementType])
    },
    destroyElement(id) {
      blobImages.destroyed(id)
      enqueue(["destroyElement", id])
      return []
    },
    appendChild(parentId, childId) {
      enqueue(["appendChild", parentId, childId])
    },
    insertBefore(parentId, childId, beforeId) {
      enqueue(["insertBefore", parentId, childId, beforeId])
    },
    setStyle(id, style) {
      enqueue(["setStyle", id, style])
    },
    setText(id, content) {
      enqueue(["setText", id, content])
    },
    setEventListener(id, eventType, hasHandler) {
      enqueue(["setEventListener", id, eventType, hasHandler])
    },
    setRoot(id) {
      enqueue(["setRoot", id])
    },
    setKeyboardFocusDim(enabled) {
      enqueue(["setKeyboardFocusDim", enabled])
    },
    setCustomProp(id, key, value) {
      blobImages.prop(id, key, value)
      enqueue(["setCustomProp", id, key, value])
    },
    flushMutations() {
      flushScheduled = false
      if (queue.length === 0) return
      appliedBatches++
      const destroyed = renderer.applyBatch(JSON.stringify(queue))
      onDestroyed(destroyed)
      queue = []
    },
  }
  registerMutationQueue(renderer, mutations)
  return mutations
}

/**
 * Handler prop to the key its handler is stored under in `EventHandlerMap`.
 * Pointer, mouse and drag keys are lowercase DOM types, and each has an
 * `on…Capture` twin keyed `captureHandlerKey(type)`. The other keys are native
 * event names. See `nativeEventType` for the type native is asked to emit.
 */
export const EVENT_PROPS: readonly (readonly [string, string])[] = [
  ["onToggleFile", "toggleFile"],
  ["onShowMore", "showMore"],
  ["onLineClick", "lineClick"],
  ["onLinkClick", "linkClick"],
  ["onPlacement", "placement"],
  ["onVisibleRange", "visibleRange"],
  ["onHighlight", "highlight"],
  ["onMotionComplete", "motionComplete"],
  ["onTransitionRun", "transitionRun"],
  ["onTransitionStart", "transitionStart"],
  ["onTransitionEnd", "transitionEnd"],
  ["onTransitionCancel", "transitionCancel"],
  ["onAnimationStart", "animationStart"],
  ["onAnimationIteration", "animationIteration"],
  ["onAnimationEnd", "animationEnd"],
  ["onAnimationCancel", "animationCancel"],
  ["onLayoutAnimationStart", "layoutAnimationStart"],
  ["onLayoutAnimationComplete", "layoutAnimationComplete"],
  ["onChange", "change"],
  ["onSubmit", "submit"],
  ["onInvalid", "invalid"],
  ...POINTER_EVENT_HANDLER_PROPS,
  ["onMouseDownOutside", "mousedownoutside"],
  ["onKeyDown", "keyDown"],
  ["onKeyUp", "keyUp"],
  ["onFocus", "focus"],
  ["onBlur", "blur"],
  ["onFocusIn", "focusIn"],
  ["onFocusOut", "focusOut"],
  ["onWheel", "wheel"],
  ["onScroll", "scroll"],
  ["onLayout", "layout"],
  ["onLoad", "load"],
  ["onError", "error"],
]

export const EVENT_PROP_NAMES: ReadonlySet<string> = new Set(
  EVENT_PROPS.map(([name]) => name)
)

/**
 * Event type for a handler prop. Adapters walk an element's own props and look
 * each one up here, which beats testing every entry of `EVENT_PROPS` against an
 * element that has two or three props.
 */
export const EVENT_TYPE_BY_PROP: ReadonlyMap<string, string> = new Map(EVENT_PROPS)

/**
 * `EVENT_TYPE_BY_PROP` for a framework whose prop means a different native
 * event. A `null` removes the prop. React DOM listens to `focusin` and
 * `focusout` for `onFocus` and `onBlur`, so those props bubble there; Solid
 * follows plain DOM, where they do not.
 */
export function eventTypesWithOverrides(
  overrides: Readonly<Record<string, string | null>>
): ReadonlyMap<string, string> {
  const types = new Map(EVENT_PROPS as readonly (readonly [string, string])[])
  for (const [prop, type] of Object.entries(overrides)) {
    if (type === null) types.delete(prop)
    else types.set(prop, type)
  }
  return types
}

export const RESERVED_PROPS: ReadonlySet<string> = new Set([
  "style",
  "className",
  "children",
  "key",
  "ref",
])

export const BUILT_IN_TYPES: ReadonlySet<string> = new Set(["div", "text", "label", "form", "span"])

const UNIVERSAL_PROPS: ReadonlySet<string> = new Set([
  "autoFocus",
  "tabIndex",
  "draggable",
  "inert",
  "motion",
  "layout",
  "testId",
  "highlight",
  "role",
  "id",
  "htmlFor",
])

/** Props a `div`, `text`, `label` or `form` forwards to native; every other prop is dropped. */
export function isUniversalProp(name: string): boolean {
  return UNIVERSAL_PROPS.has(name) || name.startsWith("aria-")
}

export function isReservedProp(name: string): boolean {
  return RESERVED_PROPS.has(name) || EVENT_PROP_NAMES.has(name)
}

/**
 * Structural equality for the JSON values a mutation carries: styles and
 * custom props. A key whose value is `undefined` counts as absent, because
 * `JSON.stringify` drops it, so `{ color: undefined }` and `{}` reach Rust as
 * the same payload and must compare equal here.
 *
 * Only plain objects, arrays and primitives are compared. A function or class
 * instance is equal to itself and nothing else.
 */
export function sameMutationValue(left: unknown, right: unknown): boolean {
  if (left === right) return true
  // NaN serializes to null on both sides, and -0 to 0, so neither is a change.
  if (typeof left === "number" && typeof right === "number") return left !== left && right !== right
  if (typeof left !== "object" || typeof right !== "object" || left === null || right === null) {
    return false
  }
  if (Array.isArray(left)) {
    if (!Array.isArray(right) || left.length !== right.length) return false
    for (let index = 0; index < left.length; index++) {
      if (!sameMutationValue(left[index], right[index])) return false
    }
    return true
  }
  if (Array.isArray(right)) return false
  const leftProto = Object.getPrototypeOf(left)
  if (leftProto !== Object.prototype && leftProto !== null) return false
  if (Object.getPrototypeOf(right) !== leftProto) return false
  const leftRecord = left as Record<string, unknown>
  const rightRecord = right as Record<string, unknown>
  let defined = 0
  for (const key in leftRecord) {
    const value = leftRecord[key]
    if (value === undefined) continue
    defined++
    if (!sameMutationValue(value, rightRecord[key])) return false
  }
  let rightDefined = 0
  for (const key in rightRecord) {
    if (rightRecord[key] !== undefined) rightDefined++
  }
  return defined === rightDefined
}

export function serializeCustomProp(
  value: object | string | number | boolean | null | undefined
): string | object | number | boolean | null {
  if (value === undefined || typeof value === "function") return null
  return value
}
