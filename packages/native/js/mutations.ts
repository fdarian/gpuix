import { DOM_DISPATCHED_HANDLER_PROPS, KEYBOARD_FOCUS_EVENT_PROPS } from "./events/event-props.js"
import { registerMutationQueue } from "./bounds.js"
import { createBlobImageLoader, type BlobImageHost } from "./blob-images.js"
import type { MutationHost, MutationRenderer } from "./host.js"
import { SVG_TEXT_PROPS } from "./svg.js"

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

  const requestFlush = () => {
    if (!scheduleFlush || flushScheduled) return
    flushScheduled = true
    scheduleFlush(() => {
      if (!flushScheduled) return
      flushScheduled = false
      mutations.flushMutations()
    })
  }

  const enqueue = (mutation: MutationTuple) => {
    queue.push(mutation)
    requestFlush()
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
    requestFlush,
    flushMutations() {
      flushScheduled = false
      if (queue.length === 0) return
      appliedBatches++
      let rejection: unknown
      for (;;) {
        try {
          const destroyed = renderer.applyBatch(JSON.stringify(queue))
          onDestroyed(destroyed)
          queue = []
          break
        } catch (error) {
          // A style native rejects is rejected the same way on every retry, so
          // keeping it would fail every later batch too. Native is atomic, so
          // the rest of the batch is intact: resend it without that op. It must
          // be dropped, not blanked: `set_style` replaces the whole style, so an
          // empty one would wipe the element's last valid style. Native numbers
          // ops by position in what it was sent, which is the queue as spliced,
          // so a second rejection names its index in the shortened batch.
          const index = rejectedStyleIndex(error)
          const mutation = index === undefined ? undefined : queue[index]
          if (index === undefined || mutation === undefined || mutation[0] !== "setStyle") {
            throw error
          }
          rejection ??= error
          queue.splice(index, 1)
        }
      }
      if (rejection !== undefined) throw rejection
    },
  }
  registerMutationQueue(renderer, mutations)
  return mutations
}

/** The batch index native names when it rejects a `setStyle` payload. */
function rejectedStyleIndex(error: unknown): number | undefined {
  const match = /^Batch op (\d+) setStyle parse error/.exec(
    error instanceof Error ? error.message : String(error)
  )
  return match ? Number(match[1]) : undefined
}

/**
 * Handler prop to the key its handler is stored under in `EventHandlerMap`.
 * Pointer, mouse, drag, wheel, transition and animation keys are lowercase DOM
 * types, and each has an `on…Capture` twin keyed `captureHandlerKey(type)`. Key
 * and focus keys are lowercase DOM types without a twin. The other keys are
 * native event names. See `nativeEventType` for the type native is asked to emit.
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
  ["onLayoutAnimationStart", "layoutAnimationStart"],
  ["onLayoutAnimationComplete", "layoutAnimationComplete"],
  ["onChange", "change"],
  ["onSubmit", "submit"],
  ["onInvalid", "invalid"],
  ...DOM_DISPATCHED_HANDLER_PROPS,
  ["onMouseDownOutside", "mousedownoutside"],
  ...KEYBOARD_FOCUS_EVENT_PROPS,
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

/**
 * Whether a prop of an element of this type reaches native. A host element
 * forwards only its universal props; an SVG `<text>` is the `text` host
 * element with SVG attributes, so it forwards those too.
 */
export function forwardsCustomProp(type: string, name: string): boolean {
  return (
    !BUILT_IN_TYPES.has(type) ||
    isUniversalProp(name) ||
    (type === "text" && SVG_TEXT_PROPS.has(name))
  )
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
