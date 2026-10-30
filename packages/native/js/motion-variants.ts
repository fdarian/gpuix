/// What the `motion.*` components mean by `variants` and the `while*` gesture props, shared by
/// React and Solid and by every element they animate: the targets a variant name stands for, the
/// variant names a controlling component hands down to the components inside it, the gesture
/// part of the description native takes, and the gesture callbacks.

import type { DomEvent } from "./events/dispatch.js"
import { window } from "./events/window-target.js"
import { IntersectionObserver, type IntersectionObserverEntry } from "./observers.js"
import type { MotionTransition } from "./host.js"

/** The name of a variant, or several: later ones win a property they share. */
export type MotionVariantLabels = string | string[]

/**
 * A style with the `transition` its animation plays with. As in Motion it replaces the component's
 * `transition` as a whole (the two are not merged), and a target without one plays with the
 * component's.
 */
export type MotionAnimationTarget<Style, Transition = MotionTransition> = Style & { transition?: Transition }

export type MotionVariants<Style, Transition = MotionTransition> = Record<
  string,
  // A method type so that `(direction: number) => ...` is assignable: `custom` is whatever the app passes.
  MotionAnimationTarget<Style, Transition> | { variant(custom: unknown): MotionAnimationTarget<Style, Transition> }["variant"]
>

/** Framer's `viewport` option of `whileInView`. */
export interface MotionViewport {
  /** Stay in the in-view state once it has been reached. */
  once?: boolean
  /** The element whose box is the viewport; the window when it has none. */
  root?: { current: object | null }
  /** `rootMargin` of the `IntersectionObserver`. */
  margin?: string
  /** How much of the element has to show: `"some"` (any), `"all"`, or a fraction. */
  amount?: "some" | "all" | number
}

/** The animation props every `motion.*` takes, for the style it can animate. */
export interface MotionAnimation<Style, Transition = MotionTransition> {
  /**
   * Where the animation starts; `false` starts it at `animate`. Nothing animates to `initial`, so a
   * `transition` in it is accepted and ignored, as in Motion.
   */
  initial?: MotionAnimationTarget<Style, Transition> | MotionVariantLabels | false
  /** A `transition` in it replaces the component's for the animation to this state. */
  animate?: MotionAnimationTarget<Style, Transition> | MotionVariantLabels
  /**
   * Played while the element leaves `AnimatePresence`. A `transition` in it replaces the component's
   * for the exit; the one in `animate` plays no part in it.
   */
  exit?:
    | MotionAnimationTarget<Style, Transition>
    | MotionVariantLabels
    | { exit(custom: unknown): MotionAnimationTarget<Style, Transition> | MotionVariantLabels }["exit"]
  /**
   * While the pointer is over the element (`:hover`). The way back plays with the `transition` of
   * `animate`, else the component's, not this one.
   */
  whileHover?: MotionAnimationTarget<Style, Transition> | MotionVariantLabels
  /** While a press that began on the element goes on (`:active`). */
  whileTap?: MotionAnimationTarget<Style, Transition> | MotionVariantLabels
  /** While the element has focus that `:focus-visible` matches; it needs a `tabIndex` to take focus. */
  whileFocus?: MotionAnimationTarget<Style, Transition> | MotionVariantLabels
  /** While any of the element (see `viewport`) is in the viewport. */
  whileInView?: MotionAnimationTarget<Style, Transition> | MotionVariantLabels
  viewport?: MotionViewport
  variants?: MotionVariants<Style, Transition>
  /** `false` ignores the variant names of the components above, and hands them nothing on. */
  inherit?: boolean
  transition?: Transition
  /** Handed to a variant or `exit` function. */
  custom?: unknown
}

/** Where a pointer gesture was, in window coordinates as Framer's `info.point`. */
export interface MotionGestureInfo {
  point: { x: number; y: number }
}

/** Framer's gesture callbacks. */
export interface MotionGestureCallbacks {
  /** The pointer entered the element (`pointerenter`). */
  onHoverStart?(event: DomEvent, info: MotionGestureInfo): void
  /** The pointer left the element and what it holds (`pointerleave`). */
  onHoverEnd?(event: DomEvent, info: MotionGestureInfo): void
  /** A primary press began on the element. */
  onTapStart?(event: DomEvent, info: MotionGestureInfo): void
  /** The press that began on the element ended on it or on what it holds. */
  onTap?(event: DomEvent, info: MotionGestureInfo): void
  /** The press that began on the element ended elsewhere, or was cancelled. */
  onTapCancel?(event: DomEvent, info: MotionGestureInfo): void
  /** The element reached the viewport `viewport` describes. */
  onViewportEnter?(entry: IntersectionObserverEntry): void
  /** The element left it; never called with `viewport.once`. */
  onViewportLeave?(entry: IntersectionObserverEntry): void
}

export const MOTION_GESTURES = ["whileInView", "whileFocus", "whileHover", "whileTap"] as const
export type MotionGestureName = (typeof MOTION_GESTURES)[number]

type GestureLabels = { [Name in MotionGestureName]?: MotionVariantLabels }

/**
 * The variant names a component that controls variants hands down: Framer's variant context. A
 * component controls variants when any of its `initial`, `animate`, `exit` or `while*` is a name,
 * and what it hands down replaces what came from further up.
 */
export interface MotionVariantContext extends GestureLabels {
  initial?: false | MotionVariantLabels
  animate?: MotionVariantLabels
  exit?: MotionVariantLabels
  /** Whether the controlling component is in the viewport its `whileInView` describes. */
  inView: boolean
}

export function isVariantLabel(target: unknown): target is MotionVariantLabels {
  return typeof target === "string" || Array.isArray(target)
}

/** How a kind of element takes a target: the keys it can animate, and the ones it ignores. */
export interface MotionTargetScope {
  /** Names the component in messages: `motion.span`. */
  component: string
  /** Throws for a key the element cannot animate. */
  check(where: string, style: object): void
  /** `style` without the keys that do nothing. */
  shown(style: object): object
}

/** Everything native takes of an element's keys; it names the one it cannot move. */
export function elementScope(component: string): MotionTargetScope {
  return { component, check: () => {}, shown: (style) => style }
}

type Target = { style: object; transition: MotionTransition | undefined }

function labelsOf(target: MotionVariantLabels): string[] {
  return typeof target === "string" ? [target] : target
}

function variantEntry(variants: MotionVariants<object, object> | undefined, label: string) {
  return variants === undefined || !Object.hasOwn(variants, label) ? undefined : variants[label]
}

/**
 * A style object with the `transition` it carries, or the variants its names stand for, merged in
 * order. A name the component itself wrote must be one of its `variants` when it has any; a name
 * handed down is skipped by a component that does not define it, as in Framer.
 */
function resolveTarget(
  scope: MotionTargetScope,
  where: string,
  target: object | MotionVariantLabels,
  variants: MotionVariants<object, object> | undefined,
  custom: unknown,
  handedDown: boolean
): Target | undefined {
  if (!isVariantLabel(target)) {
    const own: Record<string, unknown> = { ...target }
    const transition = own.transition as MotionTransition | undefined
    delete own.transition
    scope.check(where, own)
    return { style: scope.shown(own), transition }
  }
  // A component with no `variants` of its own only hands the names down.
  if (variants === undefined) return undefined
  const style: Record<string, unknown> = {}
  let transition: MotionTransition | undefined
  let found = false
  for (const label of labelsOf(target)) {
    const entry = variantEntry(variants, label)
    if (entry === undefined) {
      if (handedDown) continue
      throw new Error(`${scope.component} ${where} names the variant "${label}", which its variants do not define`)
    }
    found = true
    const variant = typeof entry === "function" ? entry(custom) : entry
    const own: Record<string, unknown> = { ...variant }
    const variantTransition = own.transition as MotionTransition | undefined
    delete own.transition
    scope.check(`the variant "${label}"`, own)
    Object.assign(style, own)
    if (variantTransition !== undefined) transition = variantTransition
  }
  return found ? { style: scope.shown(style), transition } : undefined
}

export interface ResolvedMotion {
  initial: object | false | undefined
  animate: object | undefined
  exit: object | undefined
  /** The `transition` `animate` carries or its variants name, for the animation to the resting state. */
  animateTransition: MotionTransition | undefined
  /** The same for `exit`; the one in `animate` plays no part in an exit. */
  exitTransition: MotionTransition | undefined
  /** The target of each `while*` prop the element has, with the transition its animation plays with. */
  gestures: Partial<Record<MotionGestureName, Target>>
  /** The gestures whose target came down from a controlling ancestor, which they answer to. */
  inherited: MotionGestureName[]
  /** What this component hands down to the components inside it; `undefined` when it does not control variants. */
  controls: MotionVariantContext | undefined
}

export interface MotionInput {
  initial: object | MotionVariantLabels | false | undefined
  animate: object | MotionVariantLabels | undefined
  /** Already resolved against `custom`, so not a function. */
  exit: object | MotionVariantLabels | undefined
  variants: MotionVariants<object, object> | undefined
  custom: unknown
  gestures?: Partial<Record<MotionGestureName, object | MotionVariantLabels | undefined>>
  /** What the nearest controlling ancestor hands down. */
  parent: MotionVariantContext | undefined
  /** Whether this component is in the viewport its own `whileInView` describes. */
  inView: boolean
}

function controlsVariants(input: MotionInput): boolean {
  return (
    isVariantLabel(input.initial) ||
    isVariantLabel(input.animate) ||
    isVariantLabel(input.exit) ||
    MOTION_GESTURES.some((name) => isVariantLabel(input.gestures?.[name]))
  )
}

/**
 * The style objects `animate`, `initial`, `exit` and the `while*` props stand for once their
 * variant names are looked up, with every key checked against what the element can animate. The
 * `variants` are checked whole, whether or not a target names them. A component with `variants`
 * that controls none of its own takes the names of the nearest controlling ancestor for the props
 * it leaves out.
 */
export function resolveMotion(scope: MotionTargetScope, input: MotionInput): ResolvedMotion {
  for (const label of Object.keys(input.variants ?? {})) {
    const entry = input.variants![label]!
    if (typeof entry === "function") continue
    const own: Record<string, unknown> = { ...entry }
    delete own.transition
    scope.check(`the variant "${label}"`, own)
  }
  const controlling = controlsVariants(input)
  const parent = !controlling && input.variants !== undefined ? input.parent : undefined
  const resolve = (where: string, own: object | MotionVariantLabels | undefined, inherited: MotionVariantLabels | undefined) => {
    if (own !== undefined) return resolveTarget(scope, where, own, input.variants, input.custom, false)
    if (inherited === undefined) return undefined
    return resolveTarget(scope, where, inherited, input.variants, input.custom, true)
  }
  const animate = resolve("animate", input.animate, parent?.animate)
  const exit = resolve("exit", input.exit, parent?.exit)
  const initialSource = input.initial !== undefined ? input.initial : parent?.initial
  const initial =
    input.initial !== undefined
      ? input.initial === false
        ? undefined
        : resolve("initial", input.initial, undefined)
      : resolve("initial", undefined, parent?.initial || undefined)
  const gestures: ResolvedMotion["gestures"] = {}
  const inherited: MotionGestureName[] = []
  for (const name of MOTION_GESTURES) {
    const own = input.gestures?.[name]
    if (own !== undefined) {
      const target = resolveTarget(scope, name, own, input.variants, input.custom, false)
      if (target) gestures[name] = target
      continue
    }
    const handedDown = parent?.[name]
    if (handedDown === undefined) continue
    const target = resolveTarget(scope, name, handedDown, input.variants, input.custom, true)
    if (target) {
      gestures[name] = target
      inherited.push(name)
    }
  }
  const label = <Value>(value: Value | object | undefined): Value | undefined => (isVariantLabel(value) ? (value as Value) : undefined)
  const controls: MotionVariantContext | undefined = controlling
    ? {
        initial: input.initial === false ? false : label<MotionVariantLabels>(input.initial),
        animate: label<MotionVariantLabels>(input.animate),
        exit: label<MotionVariantLabels>(input.exit),
        whileHover: label<MotionVariantLabels>(input.gestures?.whileHover),
        whileTap: label<MotionVariantLabels>(input.gestures?.whileTap),
        whileFocus: label<MotionVariantLabels>(input.gestures?.whileFocus),
        whileInView: label<MotionVariantLabels>(input.gestures?.whileInView),
        inView: input.inView,
      }
    : undefined
  return {
    initial: initialSource === false ? false : initial?.style,
    animate: animate?.style,
    exit: exit?.style,
    animateTransition: animate?.transition,
    exitTransition: exit?.transition,
    gestures,
    inherited,
    controls,
  }
}

/**
 * The fields of a motion description that the gestures add, in the shape native takes, or
 * `undefined` when there is none. `inView` is whether the component whose `whileInView` the
 * target answers to is in view.
 */
export function gestureDescription(
  resolved: Pick<ResolvedMotion, "gestures" | "inherited" | "controls">,
  wire: (transition: MotionTransition | undefined) => unknown,
  inView: boolean
): Record<string, unknown> | undefined {
  const description: Record<string, unknown> = {}
  for (const name of MOTION_GESTURES) {
    const target = resolved.gestures[name]
    if (target === undefined) continue
    const transition = wire(target.transition)
    description[name] = transition === undefined ? { ...target.style } : { ...target.style, transition }
  }
  if (resolved.gestures.whileInView !== undefined) description.inView = inView
  if (resolved.inherited.length > 0) description.inherit = resolved.inherited
  if (resolved.controls !== undefined) {
    // Native finds the nearest ancestor with this field: the one this component's names came from.
    description.controls = MOTION_GESTURES.filter((name) => resolved.controls![name] !== undefined)
  }
  return Object.keys(description).length === 0 ? undefined : description
}

/** The `IntersectionObserver` options Framer's `viewport` stands for. */
export function viewportObserverInit(viewport: MotionViewport | undefined): {
  root: object | undefined
  rootMargin: string | undefined
  threshold: number
} {
  const amount = viewport?.amount ?? "some"
  return {
    root: viewport?.root?.current ?? undefined,
    rootMargin: viewport?.margin,
    threshold: amount === "all" ? 1 : amount === "some" ? 0 : amount,
  }
}

function pointOf(event: DomEvent): MotionGestureInfo {
  if (event.clientX === undefined || event.clientY === undefined) {
    throw new Error(`a ${event.type} event carries no pointer position`)
  }
  return { point: { x: event.clientX, y: event.clientY } }
}

type PointerHandler = (event: DomEvent) => void

/** The pointer handlers Framer's hover and tap callbacks stand for. */
export interface MotionGestureHandlers {
  onPointerEnter?: PointerHandler
  onPointerLeave?: PointerHandler
  onPointerDown?: PointerHandler
}

/**
 * The `onPointerEnter`, `onPointerLeave` and `onPointerDown` the gesture callbacks of
 * `callbacks()` need (read when each event arrives), which run the handler the app wrote in
 * `own` as well; a handler no callback needs is left out. A press listens on the window for
 * where it ends, so a release outside the element is a cancel.
 */
export function gestureHandlers(
  callbacks: () => MotionGestureCallbacks,
  own: MotionGestureHandlers
): MotionGestureHandlers {
  const present = callbacks()
  const handlers: MotionGestureHandlers = {}
  if (present.onHoverStart) {
    handlers.onPointerEnter = (event) => {
      callbacks().onHoverStart?.(event, pointOf(event))
      own.onPointerEnter?.(event)
    }
  }
  if (present.onHoverEnd) {
    handlers.onPointerLeave = (event) => {
      callbacks().onHoverEnd?.(event, pointOf(event))
      own.onPointerLeave?.(event)
    }
  }
  if (present.onTapStart || present.onTap || present.onTapCancel) {
    handlers.onPointerDown = (event) => {
      own.onPointerDown?.(event)
      if (event.button !== 0) return
      const element = event.currentTarget
      callbacks().onTapStart?.(event, pointOf(event))
      const finish = (end: DomEvent) => {
        window.removeEventListener("pointerup", finish, true)
        window.removeEventListener("pointercancel", finish, true)
        const landed = end.type === "pointerup" && element !== null && end.composedPath().includes(element)
        const after = callbacks()
        if (landed) after.onTap?.(end, pointOf(end))
        else after.onTapCancel?.(end, pointOf(end))
      }
      window.addEventListener("pointerup", finish, true)
      window.addEventListener("pointercancel", finish, true)
    }
  }
  return handlers
}

/**
 * Watches `target` for `whileInView` and the viewport callbacks, with Framer's `viewport`
 * options. `change` is told when it enters and leaves; returns the function that stops it.
 */
export function observeViewport(
  target: object,
  viewport: MotionViewport | undefined,
  change: (inView: boolean, entry: IntersectionObserverEntry) => void
): () => void {
  const init = viewportObserverInit(viewport)
  let inView = false
  const observer = new IntersectionObserver(
    (entries) => {
      const latest = entries.at(-1)
      if (!latest || latest.isIntersecting === inView) return
      if (latest.isIntersecting) {
        inView = true
        change(true, latest)
        if (viewport?.once) observer.disconnect()
      } else if (!viewport?.once) {
        inView = false
        change(false, latest)
      }
    },
    { root: init.root as never, rootMargin: init.rootMargin, threshold: init.threshold }
  )
  observer.observe(target as never)
  return () => observer.disconnect()
}
