import type { MutationRenderer, StyleDesc } from "./host.js"
import { sameMutationValue } from "./mutations.js"

/** What a mounted universal style declares, and how to change or take it back. */
export interface UniversalStyleEntry {
  /** Declare `style` instead of what this entry declared. */
  update(style: StyleDesc): void
  /** Take the entry back. What the entries mounted before it declare shows again. */
  remove(): void
}

interface Entry {
  style: StyleDesc
}

interface Stack {
  /** In mount order. */
  entries: Entry[]
  /** What native holds, so a resend of the same layer costs no batch op. */
  sent: StyleDesc | null
}

// Keyed by the native renderer: the layer is one per window, however many roots
// and components declare it. Weak, so a discarded renderer takes its stack along.
const stacks = new WeakMap<object, Stack>()

/**
 * Declare `style` as part of the renderer's universal style, the layer under
 * every element's own style (CSS `* { ... }`). The layer native holds is the
 * entries' styles spread over each other in mount order, top-level properties
 * only, so a later entry wins a property it shares and removing one never
 * clears another. A property a later entry sets again moves to the end of the
 * layer: native reads the keys in written order, and a shorthand written after a
 * longhand covers it.
 *
 * The change goes through `mutations`, ordered with the element mutations it
 * queues. Flushing it is the caller's.
 */
export function pushUniversalStyle(
  renderer: object,
  mutations: Pick<MutationRenderer, "setUniversalStyle">,
  style: StyleDesc
): UniversalStyleEntry {
  let stack = stacks.get(renderer)
  if (!stack) {
    stack = { entries: [], sent: null }
    stacks.set(renderer, stack)
  }
  const mounted = stack
  const entry: Entry = { style }
  mounted.entries.push(entry)
  publish(mounted, mutations)
  return {
    update(next) {
      entry.style = next
      publish(mounted, mutations)
    },
    remove() {
      const index = mounted.entries.indexOf(entry)
      if (index === -1) return
      mounted.entries.splice(index, 1)
      publish(mounted, mutations)
    },
  }
}

function publish(stack: Stack, mutations: Pick<MutationRenderer, "setUniversalStyle">): void {
  const layer = stack.entries.length === 0 ? null : mergeInMountOrder(stack.entries)
  if (sameMutationValue(layer, stack.sent)) return
  stack.sent = layer
  mutations.setUniversalStyle(layer)
}

function mergeInMountOrder(entries: Entry[]): StyleDesc {
  const merged: Record<string, unknown> = {}
  for (const entry of entries) {
    for (const [key, value] of Object.entries(entry.style)) {
      delete merged[key]
      merged[key] = value
    }
  }
  return merged as StyleDesc
}
