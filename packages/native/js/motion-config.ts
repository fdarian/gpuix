/// What `MotionConfig` means to the adapters, shared by React and Solid: the
/// reduced-motion setting, how nested configs combine, and the reduction of
/// motion values that an element binds to `width`, `height` or an inset.

import type { MotionTransition } from "./host.js"
import { isMotionValue } from "./motion-value.js"

export type ReducedMotionSetting = "user" | "always" | "never"

export type MotionConfigValue = {
  /** Used by a `motion.*` component that has no `transition` of its own. */
  transition?: MotionTransition
  reducedMotion?: ReducedMotionSetting
}

/** A nested config only overrides what it sets. */
export function mergeMotionConfig(
  parent: MotionConfigValue | undefined,
  own: MotionConfigValue
): MotionConfigValue {
  const merged: MotionConfigValue = { ...parent }
  if (own.transition !== undefined) merged.transition = own.transition
  if (own.reducedMotion !== undefined) merged.reducedMotion = own.reducedMotion
  return merged
}

/** `prefersReduced` is the OS setting; `"never"` is the default, as in Framer. */
export function isReducedMotion(
  setting: ReducedMotionSetting | undefined,
  prefersReduced: boolean
): boolean {
  switch (setting) {
    case "always":
      return true
    case "user":
      return prefersReduced
    case "never":
    case undefined:
      return false
  }
}

/** Style keys whose animation reduced motion replaces with a jump. Mirrors `AnimProp::is_positional`. */
const REDUCED_STYLE_KEYS: ReadonlySet<string> = new Set(["width", "height", "top", "right", "bottom", "left"])

/**
 * Makes `animate()` jump on the motion values `style` binds to `width`,
 * `height`, `top`, `right`, `bottom` or `left`. Returns the release. A value
 * bound to padding, opacity, a colour and the like keeps animating, and so
 * does one that no element binds. Framer leaves a bare `animate(mv)` alone;
 * gpuix reduces it so a bound size or inset follows the setting.
 */
export function reduceMotionValues(style: unknown): () => void {
  if (typeof style !== "object" || style === null) return () => {}
  const releases: Array<() => void> = []
  for (const key of Object.keys(style)) {
    if (!REDUCED_STYLE_KEYS.has(key)) continue
    const value: unknown = Reflect.get(style, key)
    if (isMotionValue(value)) releases.push(value.reduceMotion())
  }
  return () => {
    for (const release of releases) release()
  }
}
