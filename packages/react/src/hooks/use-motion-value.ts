import { useEffect, useRef, useState } from "react"
import {
  computedValue,
  isMotionValue,
  motionValue,
  sameMutationValue,
  springValue,
  transformValue,
  velocityValue,
} from "@gpuix/native/host"
import type {
  MotionData,
  MotionSpringConfig,
  MotionTransformOptions,
  MotionValue,
  MotionValueEvents,
  Widen,
} from "@gpuix/native/host"
import { reconciler } from "../reconciler/reconciler.js"
import { useGpuixRequired } from "./use-gpuix.js"

/**
 * Keeps the value this window's renderer samples for as long as the component
 * is mounted. A value with no element, listener or reader bound to it has no
 * node, so it could neither be read when it derives from other values nor be
 * animated.
 */
function useBound(value: MotionValue<MotionData>): void {
  const renderer = useGpuixRequired()
  useEffect(() => value.bind(renderer), [value, renderer])
}

/**
 * The value `create` made last time, while `deps` are unchanged. An inline
 * `[0, 100]` or `{ clamp: false }` is a new object on every render, so it is
 * compared by content: a new value each render would rebind its elements.
 */
function useStable<Value>(
  deps: readonly unknown[],
  create: (previous: Value | undefined) => Value
): Value {
  const cache = useRef<{ deps: readonly unknown[]; value: Value } | undefined>(undefined)
  const cached = cache.current
  if (cached && sameMutationValue(cached.deps, deps)) return cached.value
  const value = create(cached?.value)
  cache.current = { deps, value }
  return value
}

/** A motion value that lives as long as the component, like Framer's `useMotionValue`. */
export function useMotionValue<Data extends MotionData>(initial: Data): MotionValue<Widen<Data>> {
  const [value] = useState(() => motionValue(initial))
  useBound(value)
  return value
}

/** Maps `input` through the ranges natively, so it moves with `input` in the same frame. */
export function useTransform<Out extends MotionData>(
  input: MotionValue<number>,
  inputRange: number[],
  outputRange: Out[],
  options?: MotionTransformOptions
): MotionValue<Widen<Out>>
/** Computed in JS from `input`: it trails by a frame and is not native-linked. */
export function useTransform<In extends MotionData, Out extends MotionData>(
  input: MotionValue<In>,
  transform: (latest: In) => Out
): MotionValue<Widen<Out>>
/** Computed in JS from every value in `inputs`. */
export function useTransform<In extends MotionData, Out extends MotionData>(
  inputs: MotionValue<In>[],
  transform: (latest: In[]) => Out
): MotionValue<Widen<Out>>
/**
 * Computed in JS from the motion values `compute` reads with `get()`. State
 * and props it captures are not tracked: it re-runs when a value it read changes.
 */
export function useTransform<Out extends MotionData>(compute: () => Out): MotionValue<Widen<Out>>
export function useTransform(
  first: unknown,
  second?: unknown,
  third?: unknown,
  fourth?: unknown
): MotionValue<MotionData> {
  const fn = [first, second].find((arg) => typeof arg === "function") as
    | ((...args: unknown[]) => MotionData)
    | undefined
  // The latest render's function, so a computed value does not run a stale closure.
  const latest = useRef(fn)
  useEffect(() => {
    latest.current = fn
  })

  const value = useStable<MotionValue<MotionData>>(
    fn
      ? [isMotionValue(first) || Array.isArray(first) ? first : undefined]
      : [first, second, third, fourth],
    () => {
      const run = (...args: unknown[]) => {
        const current = latest.current
        if (!current) throw new Error("useTransform lost its function")
        return current(...args)
      }
      if (typeof first === "function") return computedValue(() => run())
      if (typeof second === "function") {
        if (Array.isArray(first)) {
          const inputs = first as MotionValue<MotionData>[]
          return computedValue(() => run(inputs.map((input) => input.get())))
        }
        if (isMotionValue(first)) return computedValue(() => run(first.get()))
      } else if (isMotionValue(first) && Array.isArray(second) && Array.isArray(third)) {
        return transformValue(
          first as MotionValue<number>,
          second as number[],
          third as MotionData[],
          fourth as MotionTransformOptions | undefined
        )
      }
      throw new Error(
        "useTransform takes (value, inputRange, outputRange, options?), (value | values, transform) or (compute)"
      )
    }
  )
  useBound(value)
  return value
}

/**
 * A spring that follows `source`. Given a plain value, a spring with its own
 * target that `set` moves. Changing `config` makes a new spring that starts
 * from where the old one was.
 */
export function useSpring<Data extends MotionData>(
  source: MotionValue<Data>,
  config?: MotionSpringConfig
): MotionValue<Data>
export function useSpring<Data extends MotionData>(
  initial: Data,
  config?: MotionSpringConfig
): MotionValue<Widen<Data>>
export function useSpring(
  source: MotionValue<MotionData> | MotionData,
  config?: MotionSpringConfig
): MotionValue<MotionData> {
  const value = useStable<MotionValue<MotionData>>(
    [isMotionValue(source) ? source : undefined, config],
    (previous) =>
      isMotionValue(source)
        ? springValue(source, config)
        : springValue(previous ? previous.get() : source, config)
  )
  useBound(value)
  return value
}

/** The velocity of `value` in units per second, as a motion value. */
export function useVelocity(value: MotionValue<number>): MotionValue<number> {
  const velocity = useStable([value], () => velocityValue(value))
  useBound(velocity)
  return velocity
}

/**
 * Calls `callback` when `value` emits `event`. The value is bound to this
 * window while listening, so a derived value with no element still reports.
 * The callback runs outside React's scheduler, in a batch, so its `setState`
 * calls from one frame render once.
 */
export function useMotionValueEvent<
  Data extends MotionData,
  Event extends keyof MotionValueEvents<Data>,
>(value: MotionValue<Data>, event: Event, callback: MotionValueEvents<Data>[Event]): void {
  const renderer = useGpuixRequired()
  const latest = useRef(callback)
  useEffect(() => {
    latest.current = callback
  })
  useEffect(() => {
    const release = value.bind(renderer)
    const stop = value.on(event, ((...args: unknown[]) => {
      reconciler.batchedUpdates(() => (latest.current as (...args: unknown[]) => void)(...args), undefined)
    }) as MotionValueEvents<Data>[Event])
    return () => {
      stop()
      release()
    }
  }, [value, event, renderer])
}
