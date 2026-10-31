import { batch, onCleanup } from "solid-js"
import {
  computedValue,
  isMotionValue,
  motionValue,
  springValue,
  transformValue,
  velocityValue,
} from "@ghia/native/host"
import type {
  MotionData,
  MotionSpringConfig,
  MotionTransformOptions,
  MotionValue,
  MotionValueEvents,
  Widen,
} from "@ghia/native/host"
import { useGpuixRequired } from "./root.js"

/**
 * Keeps the value this window's renderer samples for as long as the owner
 * lives. A value with no element, listener or reader bound to it has no node,
 * so it could neither be read when it derives from other values nor be animated.
 */
function bound<Value extends MotionValue<MotionData>>(value: Value): Value {
  onCleanup(value.bind(useGpuixRequired()))
  return value
}

/** A motion value that lives as long as the owner, like Framer's `useMotionValue`. */
export function createMotionValue<Data extends MotionData>(
  initial: Data
): MotionValue<Widen<Data>> {
  return bound(motionValue(initial))
}

/** Maps `input` through the ranges natively, so it moves with `input` in the same frame. */
export function createTransform<Out extends MotionData>(
  input: MotionValue<number>,
  inputRange: number[],
  outputRange: Out[],
  options?: MotionTransformOptions
): MotionValue<Widen<Out>>
/** Computed in JS from `input`: it trails by a frame and is not native-linked. */
export function createTransform<In extends MotionData, Out extends MotionData>(
  input: MotionValue<In>,
  transform: (latest: In) => Out
): MotionValue<Widen<Out>>
/** Computed in JS from every value in `inputs`. */
export function createTransform<In extends MotionData, Out extends MotionData>(
  inputs: MotionValue<In>[],
  transform: (latest: In[]) => Out
): MotionValue<Widen<Out>>
/**
 * Computed in JS from the motion values `compute` reads with `get()`. Signals
 * it reads are not tracked: it re-runs when a value it read changes.
 */
export function createTransform<Out extends MotionData>(
  compute: () => Out
): MotionValue<Widen<Out>>
export function createTransform(
  first: unknown,
  second?: unknown,
  third?: unknown,
  fourth?: unknown
): MotionValue<MotionData> {
  if (typeof first === "function") {
    return bound(computedValue(first as () => MotionData))
  }
  if (typeof second === "function") {
    const transform = second as (latest: unknown) => MotionData
    if (Array.isArray(first)) {
      const inputs = first as MotionValue<MotionData>[]
      return bound(computedValue(() => transform(inputs.map((input) => input.get()))))
    }
    if (isMotionValue(first)) return bound(computedValue(() => transform(first.get())))
  } else if (isMotionValue(first) && Array.isArray(second) && Array.isArray(third)) {
    return bound(
      transformValue(
        first as MotionValue<number>,
        second as number[],
        third as MotionData[],
        fourth as MotionTransformOptions | undefined
      )
    )
  }
  throw new Error(
    "createTransform takes (value, inputRange, outputRange, options?), (value | values, transform) or (compute)"
  )
}

/**
 * A spring that follows `source`. Given a plain value, a spring with its own
 * target that `set` moves.
 */
export function createSpring<Data extends MotionData>(
  source: MotionValue<Data>,
  config?: MotionSpringConfig
): MotionValue<Data>
export function createSpring<Data extends MotionData>(
  initial: Data,
  config?: MotionSpringConfig
): MotionValue<Widen<Data>>
export function createSpring(
  source: MotionValue<MotionData> | MotionData,
  config?: MotionSpringConfig
): MotionValue<MotionData> {
  // `springValue` is overloaded, so the union has to be split for the call to type.
  return bound(isMotionValue(source) ? springValue(source, config) : springValue(source, config))
}

/** The velocity of `value` in units per second, as a motion value. */
export function createVelocity(value: MotionValue<number>): MotionValue<number> {
  return bound(velocityValue(value))
}

/**
 * Calls `callback` when `value` emits `event`, until the owner is disposed. The
 * value is bound to this window while listening, so a derived value with no
 * element still reports. The callback runs in a `batch`, so its signal writes
 * from one frame update once.
 */
export function createMotionValueEvent<
  Data extends MotionData,
  Event extends keyof MotionValueEvents<Data>,
>(value: MotionValue<Data>, event: Event, callback: MotionValueEvents<Data>[Event]): void {
  const release = value.bind(useGpuixRequired())
  const stop = value.on(event, ((...args: unknown[]) => {
    batch(() => (callback as (...args: unknown[]) => void)(...args))
  }) as MotionValueEvents<Data>[Event])
  onCleanup(() => {
    stop()
    release()
  })
}
