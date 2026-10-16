import {
  createComponent,
  createContext,
  createMemo,
  useContext,
  type Accessor,
  type JSX,
} from "solid-js"
import { isReducedMotion } from "@gpuix/native/host"
import type { MotionConfigValue, ReducedMotionSetting } from "@gpuix/native/host"
import { createReducedMotion } from "../primitives.js"

export type MotionConfigState = {
  transition: Accessor<MotionConfigValue["transition"]>
  reducedMotion: Accessor<ReducedMotionSetting | undefined>
  /** The setting resolved against the OS preference. */
  reduced: Accessor<boolean>
}

export const MotionConfigContext = createContext<MotionConfigState>()

export type MotionConfigProps = MotionConfigValue & {
  children?: JSX.Element
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
export function MotionConfig(props: MotionConfigProps): JSX.Element {
  const parent = useContext(MotionConfigContext)
  const prefersReduced = createReducedMotion()
  const transition = () => (props.transition !== undefined ? props.transition : parent?.transition())
  const reducedMotion = () => (props.reducedMotion !== undefined ? props.reducedMotion : parent?.reducedMotion())
  const reduced = createMemo(() => isReducedMotion(reducedMotion(), prefersReduced()))
  return createComponent(MotionConfigContext.Provider, {
    value: { transition, reducedMotion, reduced },
    get children() {
      return props.children
    },
  })
}

/** Whether `motion.*` components here are reduced, per the nearest `MotionConfig`. */
export function createReducedMotionConfig(): Accessor<boolean> {
  const config = useContext(MotionConfigContext)
  return () => config !== undefined && config.reduced()
}
