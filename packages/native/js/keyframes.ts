import type { GpuixRenderer } from "../index.js"
import type { MotionStyleTargets } from "./host.js"

/**
 * The SVG properties CSS animates, for the shapes inside an `<svg>` and for the
 * `<svg>` itself (a value on the `<svg>` is inherited by the shapes in it). A
 * property applies only to the elements it does in a browser (`r` to a
 * `<circle>`, `stopColor` to a `<stop>`...). `opacity`, `width`, `height` and
 * `fontSize` are the shape's own on a shape and the box's on any other element.
 * `pathLength`, `pathOffset`, `pathSpacing`, `x1`..`y2`, `stdDeviation`, `dx`,
 * `dy`, `refX` and `refY` are attributes, not CSS properties: keyframes refuse them.
 */
export interface SvgKeyframeStyle {
  strokeDasharray?: number | string | number[]
  strokeDashoffset?: number
  strokeWidth?: number
  strokeOpacity?: number
  strokeMiterlimit?: number
  fill?: string
  stroke?: string
  fillOpacity?: number
  stopColor?: string
  stopOpacity?: number
  floodColor?: string
  floodOpacity?: number
  cx?: number
  cy?: number
  r?: number
  rx?: number
  ry?: number
  x?: number
  y?: number
  letterSpacing?: number
  /** Path data, or CSS `path("M0 0 L10 10")`. Paths morph when their commands match. */
  d?: string
}

/**
 * The properties one keyframe can set: the ones `MotionStyle` animates and the
 * SVG ones, each with a single value, plus the easing from that keyframe to
 * the next.
 */
export type KeyframeStyle = Omit<MotionStyleTargets, "scale"> &
  SvgKeyframeStyle &
  TransformKeyframeStyle & {
    /** A CSS easing function (`ease-in`, `cubic-bezier()`, `steps()`...). */
    animationTimingFunction?: string
  }

/**
 * The CSS transform properties a keyframe sets. Lists blend function by function while they pair up, and
 * as 2D matrices from the first function that does not; `none` blends as the identity of the other
 * list; `translate`, `rotate` and `scale` blend as their own values. `scale` is the CSS property here, a
 * number or a string, and not Motion's.
 */
export interface TransformKeyframeStyle {
  transform?: string
  translate?: string
  scale?: number | string
  transformOrigin?: string
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

/** Throws when the definition is bad. The native addon's `validateKeyframes`. */
export type KeyframesValidator = (name: string, framesJson: string) => void

interface KeyframesRegistry {
  /** Name to the frames as JSON, which is what a renderer takes. */
  readonly definitions: Map<string, string>
  readonly hosts: Set<WeakRef<KeyframesHost>>
  validator?: KeyframesValidator
}

// `@keyframes` is document-global, so one registry serves every renderer in the
// process. It lives on `globalThis` so two copies of this module share it.
const REGISTRY_KEY = Symbol.for("@ghia/native/keyframes")

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
 * it, and animations already running on it show the new frames from the next
 * frame on, keeping their start time, iteration, direction and fill.
 *
 * ```ts
 * defineKeyframes("fade-in", {
 *   from: { opacity: 0 },
 *   to: { opacity: 1 },
 * })
 * ```
 *
 * A bad definition throws from here and keeps the earlier one, whether or not a
 * renderer exists yet. A renderer that throws for a good definition (its window
 * closed) does not stop the others: every renderer is tried, the definition is
 * kept, and the error is thrown afterwards. The check needs the native addon, so it runs once
 * `@ghia/native/runtime` or `/testing` is loaded, which every renderer package
 * does; the web build has no motion engine and checks nothing.
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
  shared.validator?.(name, framesJson)
  // Recorded before any host hears it: the validator accepted it, so a host
  // that throws is a broken renderer (a closed window), not a bad definition.
  // Stopping at that host would leave the registry, the hosts before it and
  // the hosts after it each holding a different definition.
  shared.definitions.set(name, framesJson)
  const failures: unknown[] = []
  for (const host of liveHosts(shared)) {
    try {
      host.defineKeyframes(name, framesJson)
    } catch (error) {
      failures.push(error)
    }
  }
  if (failures.length === 1) throw failures[0]
  if (failures.length > 1) {
    throw new AggregateError(failures, `${failures.length} renderers rejected the keyframes ${JSON.stringify(name)}`)
  }
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

/**
 * Lets `defineKeyframes` check a definition before any renderer exists. The
 * native entry points call this; `keyframes.ts` cannot import the addon itself
 * because `@ghia/native/host` is also loaded by the browser build.
 */
export function registerKeyframesValidator(validator: KeyframesValidator): void {
  registry().validator = validator
}

/**
 * Stops sending definitions to `host`. Call it when the renderer is destroyed
 * or its window closes, so a dead renderer is not asked to take one.
 */
export function unregisterKeyframesHost(host: KeyframesHost): void {
  const shared = registry()
  for (const reference of shared.hosts) {
    if (reference.deref() === host) shared.hosts.delete(reference)
  }
}
