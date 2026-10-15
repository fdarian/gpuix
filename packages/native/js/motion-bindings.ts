/// Style entries that are motion values do not travel as style: they bind the
/// value to the element's window and go to native as the `motionValues`
/// custom prop, which samples them every frame. The framework adapters share
/// this; each owns a `MotionBindingHost` on its root.

import type { MutationRenderer, NativeRenderer, StyleDesc } from "./host.js"
import { isMotionValue } from "./motion-value.js"
import type { MotionValue } from "./motion-value.js"

export type MotionBinding = {
  values: Record<string, MotionValue>
  releases: Array<() => void>
}

export interface MotionBindingHost {
  nativeRenderer: NativeRenderer
  /** Element id to the motion values its style binds. */
  motionBindings: Map<number, MotionBinding>
  /** Releases waiting for the mutation batch that stops the elements from reading the value. */
  motionReleases: Array<() => void>
}

export function splitMotionStyle(
  style: object | null | undefined
): { style: StyleDesc | undefined; motionValues: Record<string, MotionValue> | undefined } {
  if (style == null) return { style: undefined, motionValues: undefined }
  let motionValues: Record<string, MotionValue> | undefined
  const plain: Record<string, unknown> = {}
  for (const key of Object.keys(style)) {
    const value: unknown = Reflect.get(style, key)
    if (isMotionValue(value)) {
      motionValues ??= {}
      motionValues[key] = value
    } else {
      plain[key] = value
    }
  }
  // Untouched when it holds no motion value, so the common style is not copied.
  return { style: (motionValues ? plain : style) as StyleDesc, motionValues }
}

function sameBindings(
  left: Record<string, MotionValue> | undefined,
  right: Record<string, MotionValue> | undefined
): boolean {
  if (left === right) return true
  if (!left || !right) return false
  const keys = Object.keys(left)
  if (keys.length !== Object.keys(right).length) return false
  return keys.every((key) => left[key] === right[key])
}

/**
 * Brings the element's binding in line with its style. New values are bound
 * before the old are released, and the release waits for the batch that
 * rebinds the element, so a value shared by both is never freed in between.
 */
export function syncMotionBindings(
  host: MotionBindingHost,
  mutations: Pick<MutationRenderer, "setCustomProp">,
  id: number,
  next: Record<string, MotionValue> | undefined
): void {
  const previous = host.motionBindings.get(id)
  if (sameBindings(previous?.values, next)) return

  if (!next) {
    if (previous) host.motionReleases.push(...previous.releases)
    host.motionBindings.delete(id)
    mutations.setCustomProp(id, "motionValues", null)
    return
  }

  const binding: MotionBinding = { values: next, releases: [] }
  const ids: Record<string, number> = {}
  try {
    for (const [property, value] of Object.entries(next)) {
      binding.releases.push(value.bind(host.nativeRenderer))
      ids[property] = value.id
    }
  } catch (error) {
    for (const release of binding.releases) release()
    throw error
  }
  if (previous) host.motionReleases.push(...previous.releases)
  host.motionBindings.set(id, binding)
  mutations.setCustomProp(id, "motionValues", ids)
}

/** Queues the releases of the elements that are going away, without running them. */
export function retireMotionBindings(host: MotionBindingHost, ids: readonly number[]): void {
  for (const id of ids) {
    const binding = host.motionBindings.get(id)
    if (!binding) continue
    host.motionBindings.delete(id)
    host.motionReleases.push(...binding.releases)
  }
}

/** Runs once the mutation batch has been applied. */
export function runMotionReleases(host: MotionBindingHost): void {
  const releases = host.motionReleases.splice(0)
  for (const release of releases) release()
}
