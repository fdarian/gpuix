import { createContext, createElement, useContext, useMemo } from "react"
import type { ReactElement, ReactNode } from "react"
import { isReducedMotion, mergeMotionConfig } from "@ghia/native/host"
import type { MotionConfigValue } from "@ghia/native/host"
import { useReducedMotion } from "../hooks/use-media-query.js"

type MotionConfigState = MotionConfigValue & {
  /** The setting resolved against the OS preference, so a consumer does not subscribe to it. */
  reduced: boolean
}

export const MotionConfigContext = createContext<MotionConfigState | undefined>(undefined)

export type MotionConfigProps = MotionConfigValue & {
  children?: ReactNode
}

/**
 * Defaults for the `motion.*` components below it. A nested config overrides
 * only what it sets.
 *
 * `reducedMotion` is `"never"` unless set. Reduced, layout animations and
 * `width`, `height`, `top`, `right`, `bottom` and `left` jump to their target
 * instead of animating, and so do motion values bound to those style keys;
 * nothing else changes. `"user"` follows the OS setting.
 */
export function MotionConfig(props: MotionConfigProps): ReactElement {
  const parent = useContext(MotionConfigContext)
  const prefersReduced = useReducedMotion()
  const value = useMemo((): MotionConfigState => {
    const merged = mergeMotionConfig(parent, {
      transition: props.transition,
      reducedMotion: props.reducedMotion,
    })
    return { ...merged, reduced: isReducedMotion(merged.reducedMotion, prefersReduced) }
  }, [parent, props.transition, props.reducedMotion, prefersReduced])
  return createElement(MotionConfigContext.Provider, { value }, props.children)
}

/** Whether `motion.*` components here are reduced, per the nearest `MotionConfig`. */
export function useReducedMotionConfig(): boolean {
  const config = useContext(MotionConfigContext)
  return config !== undefined && config.reduced
}
