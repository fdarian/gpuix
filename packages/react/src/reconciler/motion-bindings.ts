/// Style entries that are motion values do not travel as style: they bind the
/// value to the element's window and go to native as the `motionValues`
/// custom prop, which samples them every frame.

import { isMotionValue } from "@gpuix/native/host"
import type { StyleDesc, MotionValue } from "@gpuix/native/host"
import type { Container, MotionBinding, Props } from "../types/host.js"

type SplitStyle = {
  style: StyleDesc | undefined
  motionValues: Record<string, MotionValue> | undefined
}

export function splitStyle(style: Props["style"]): SplitStyle {
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
  return { style: motionValues ? (plain as StyleDesc) : (style as StyleDesc), motionValues }
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
export function syncMotionBindings(container: Container, id: number, style: Props["style"]): void {
  const next = splitStyle(style).motionValues
  const previous = container.motionBindings.get(id)
  if (sameBindings(previous?.values, next)) return

  if (!next) {
    if (previous) container.motionReleases.push(...previous.releases)
    container.motionBindings.delete(id)
    container.renderer.setCustomProp(id, "motionValues", null)
    return
  }

  const binding: MotionBinding = { values: next, releases: [] }
  const ids: Record<string, number> = {}
  try {
    for (const [property, value] of Object.entries(next)) {
      binding.releases.push(value.bind(container.nativeRenderer))
      ids[property] = value.id
    }
  } catch (error) {
    for (const release of binding.releases) release()
    throw error
  }
  if (previous) container.motionReleases.push(...previous.releases)
  container.motionBindings.set(id, binding)
  container.renderer.setCustomProp(id, "motionValues", ids)
}

/** Runs once the mutation batch has been applied, with the ids it destroyed. */
export function releaseMotionBindings(container: Container, destroyed: readonly number[]): void {
  for (const id of destroyed) {
    const binding = container.motionBindings.get(id)
    if (!binding) continue
    container.motionBindings.delete(id)
    container.motionReleases.push(...binding.releases)
  }
  const releases = container.motionReleases.splice(0)
  for (const release of releases) release()
}
