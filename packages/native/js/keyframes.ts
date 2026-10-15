import type { GpuixRenderer } from "../index.js"
import type { MotionStyleTargets } from "./host.js"

/**
 * The properties one keyframe can set: the ones `MotionStyle` animates, each
 * with a single value, plus the easing from that keyframe to the next.
 */
export type KeyframeStyle = MotionStyleTargets & {
  /** A CSS easing function (`ease-in`, `cubic-bezier()`, `steps()`...). */
  animationTimingFunction?: string
}

/**
 * Keyframes by selector: `from`, `to`, a percentage (`"50%"`) or a comma list
 * of them (`"0%, 50%"`). Blocks at the same offset merge, later ones winning.
 */
export interface KeyframesDefinition {
  from?: KeyframeStyle
  to?: KeyframeStyle
  [selector: string]: KeyframeStyle | undefined
}

/** What a renderer needs to receive keyframes. */
export type KeyframesHost = Pick<GpuixRenderer, "defineKeyframes">

interface KeyframesRegistry {
  /** Name to the frames as JSON, which is what a renderer takes. */
  readonly definitions: Map<string, string>
  readonly hosts: Set<WeakRef<KeyframesHost>>
}

// `@keyframes` is document-global, so one registry serves every renderer in the
// process. It lives on `globalThis` so two copies of this module share it.
const REGISTRY_KEY = Symbol.for("@gpuix/native/keyframes")

function registry(): KeyframesRegistry {
  const existing = Reflect.get(globalThis, REGISTRY_KEY) as KeyframesRegistry | undefined
  if (existing) return existing
  const created: KeyframesRegistry = { definitions: new Map(), hosts: new Set() }
  Reflect.set(globalThis, REGISTRY_KEY, created)
  return created
}

function liveHosts(shared: KeyframesRegistry): KeyframesHost[] {
  const live: KeyframesHost[] = []
  for (const reference of shared.hosts) {
    const host = reference.deref()
    if (host) live.push(host)
    else shared.hosts.delete(reference)
  }
  return live
}

/**
 * Defines the keyframes `animation: name ...` plays, as `@keyframes name` does,
 * on every renderer, including ones created later. Redefining a name replaces
 * it: animations that start afterwards use the new frames, ones already running
 * keep the old.
 *
 * ```ts
 * defineKeyframes("fade-in", {
 *   from: { opacity: 0 },
 *   to: { opacity: 1 },
 * })
 * ```
 *
 * A bad definition throws and keeps the earlier one. With no renderer yet it
 * cannot be checked, so it throws when the first renderer receives it.
 */
export function defineKeyframes(name: string, frames: KeyframesDefinition): void {
  if (typeof name !== "string" || name === "") {
    throw new Error("defineKeyframes needs a keyframes name")
  }
  if (typeof frames !== "object" || frames === null || Array.isArray(frames)) {
    throw new Error(`defineKeyframes(${JSON.stringify(name)}) needs an object of keyframes`)
  }
  const shared = registry()
  const framesJson = JSON.stringify(frames)
  for (const host of liveHosts(shared)) host.defineKeyframes(name, framesJson)
  shared.definitions.set(name, framesJson)
}

/**
 * Sends every definition to `host` and every later one too. Call it once a
 * renderer can take them: a window is created by `init`, a test renderer by its
 * constructor. The host is held weakly.
 */
export function registerKeyframesHost(host: KeyframesHost): void {
  const shared = registry()
  for (const [name, framesJson] of shared.definitions) host.defineKeyframes(name, framesJson)
  for (const reference of shared.hosts) {
    if (reference.deref() === host) return
  }
  shared.hosts.add(new WeakRef(host))
}
