/// What `motion.span` means to the adapters, shared by React and Solid: the
/// properties an inline span can animate, `variants`, and the props that make no
/// sense on a box a paragraph paints.

import type { MotionStyle, MotionTransition } from "./host.js"
import { isMotionValue } from "./motion-value.js"

/** The properties a paragraph repaints without laying out again. Mirrors `Scope::InlineBox`. */
const MOTION_SPAN_KEYS = [
  "opacity",
  "color",
  "backgroundColor",
  "textDecorationColor",
  "outlineColor",
  "borderColor",
  "borderTopColor",
  "borderRightColor",
  "borderBottomColor",
  "borderLeftColor",
] as const

export type MotionSpanKey = (typeof MOTION_SPAN_KEYS)[number]

/**
 * What a `motion.span` can animate: colours, and the opacity they are painted
 * with. A span is not a box of its own but text a paragraph paints, so a size,
 * padding, font or `layout` has nothing to move.
 */
export type MotionSpanStyle = Pick<MotionStyle, MotionSpanKey>

/** The name of a variant, or several: later ones win a property they share. */
export type MotionVariantLabels = string | string[]

/** A variant's `transition` is the one its animation plays with, in place of the component's. */
export type MotionSpanVariant = MotionSpanStyle & { transition?: MotionTransition }

export type MotionSpanVariants = Record<
  string,
  // A method type so that `(direction: number) => ...` is assignable: `custom` is whatever the app passes.
  MotionSpanVariant | { variant(custom: unknown): MotionSpanVariant }["variant"]
>

export interface MotionSpanAnimation {
  /** Where the animation starts; `false` starts it at `animate`. */
  initial?: MotionSpanStyle | MotionVariantLabels | false
  animate?: MotionSpanStyle | MotionVariantLabels
  /** Played while the span leaves `AnimatePresence`. */
  exit?:
    | MotionSpanStyle
    | MotionVariantLabels
    | { exit(custom: unknown): MotionSpanStyle | MotionVariantLabels }["exit"]
  variants?: MotionSpanVariants
  transition?: MotionTransition
  /** Handed to a variant or `exit` function. */
  custom?: unknown
}

/** Props of Framer's `motion.*` that need a box to measure or to drag. Typed `never` so a `motion.span` rejects them when compiled too. */
export type MotionSpanUnsupportedProps = {
  [Prop in (typeof LAYOUT_PROPS)[number] | (typeof DRAG_PROPS)[number] | (typeof WHILE_PROPS)[number]]?: never
}

const LAYOUT_PROPS = [
  "layout",
  "layoutId",
  "layoutDependency",
  "layoutScroll",
  "layoutRoot",
  "layoutDrag",
  "onLayoutAnimationStart",
  "onLayoutAnimationComplete",
  "onLayoutMeasure",
] as const

const DRAG_PROPS = [
  "drag",
  "dragConstraints",
  "dragElastic",
  "dragMomentum",
  "dragTransition",
  "dragControls",
  "dragListener",
  "dragSnapToOrigin",
  "dragPropagation",
  "dragDirectionLock",
  "onDirectionLock",
  "onDragTransitionEnd",
] as const

const WHILE_PROPS = ["whileHover", "whileTap", "whileFocus", "whileInView", "whileDrag"] as const

function unsupportedReason(prop: string): string | undefined {
  if ((LAYOUT_PROPS as readonly string[]).includes(prop)) {
    return "an inline span has no box to measure, so it cannot take part in layout animation"
  }
  if ((DRAG_PROPS as readonly string[]).includes(prop)) {
    return "an inline span has no box to drag"
  }
  if ((WHILE_PROPS as readonly string[]).includes(prop)) {
    return "a span takes its pointer and focus states from the hover, active, focusVisible and focusWithin entries of its style, which transition its colours"
  }
  return undefined
}

/**
 * Throws for a prop that a `motion.span` cannot honour, and for a motion value
 * in its style: nothing samples one for the text a paragraph paints.
 */
export function assertMotionSpanProps(props: Record<string, unknown>): void {
  for (const prop of Object.keys(props)) {
    if (props[prop] === undefined) continue
    const reason = unsupportedReason(prop)
    if (reason !== undefined) throw new Error(`motion.span does not support "${prop}": ${reason}`)
  }
  const style = props.style
  if (typeof style !== "object" || style === null) return
  for (const key of Object.keys(style)) {
    if (isMotionValue(Reflect.get(style, key))) {
      throw new Error(
        `motion.span does not take a motion value in style.${key}: a paragraph paints the text of a span and samples no motion value for it, so animate its colours with animate, initial and exit`
      )
    }
  }
}

function assertSpanStyle(where: string, style: object): void {
  for (const key of Object.keys(style)) {
    if (!(MOTION_SPAN_KEYS as readonly string[]).includes(key)) {
      throw new Error(
        `motion.span cannot animate "${key}" in ${where}: an inline span can only animate colours and opacity (${MOTION_SPAN_KEYS.join(", ")})`
      )
    }
  }
}

type SpanTarget = { style: MotionSpanStyle; transition: MotionTransition | undefined }

function labelsOf(target: string | string[]): string[] {
  return typeof target === "string" ? [target] : target
}

/** A style object, or the variants its labels name, merged in order. */
function resolveTarget(
  where: string,
  target: MotionSpanStyle | MotionVariantLabels,
  variants: MotionSpanVariants | undefined,
  custom: unknown
): SpanTarget {
  if (typeof target === "object" && !Array.isArray(target)) {
    assertSpanStyle(where, target)
    return { style: target, transition: undefined }
  }
  const style: Record<string, unknown> = {}
  let transition: MotionTransition | undefined
  for (const label of labelsOf(target)) {
    const entry = variants === undefined ? undefined : Object.hasOwn(variants, label) ? variants[label] : undefined
    if (entry === undefined) {
      throw new Error(`motion.span ${where} names the variant "${label}", which its variants do not define`)
    }
    const variant = typeof entry === "function" ? entry(custom) : entry
    const own: Record<string, unknown> = { ...variant }
    const variantTransition = own.transition as MotionTransition | undefined
    delete own.transition
    assertSpanStyle(`the variant "${label}"`, own)
    Object.assign(style, own)
    if (variantTransition !== undefined) transition = variantTransition
  }
  return { style: style as MotionSpanStyle, transition }
}

export interface ResolvedSpanMotion {
  initial: MotionSpanStyle | false | undefined
  animate: MotionSpanStyle | undefined
  exit: MotionSpanStyle | undefined
  /** The `transition` of the variants `animate` names, for the animation to the span's resting state. */
  animateTransition: MotionTransition | undefined
  /** The same for `exit`. */
  exitTransition: MotionTransition | undefined
}

export interface SpanMotionInput {
  initial: MotionSpanAnimation["initial"]
  animate: MotionSpanAnimation["animate"]
  /** Already resolved against `custom`, so not a function. */
  exit: MotionSpanStyle | MotionVariantLabels | undefined
  variants: MotionSpanVariants | undefined
  custom: unknown
}

/**
 * The style objects `animate`, `initial` and `exit` stand for once their
 * variant names are looked up, with every key checked against what a span can
 * animate. The `variants` are checked whole, whether or not a target names them.
 */
export function resolveSpanMotion(input: SpanMotionInput): ResolvedSpanMotion {
  for (const label of Object.keys(input.variants ?? {})) {
    const entry = input.variants![label]!
    if (typeof entry === "function") continue
    const own: Record<string, unknown> = { ...entry }
    delete own.transition
    assertSpanStyle(`the variant "${label}"`, own)
  }
  const resolve = (where: string, target: MotionSpanStyle | MotionVariantLabels | undefined) =>
    target === undefined ? undefined : resolveTarget(where, target, input.variants, input.custom)
  const animate = resolve("animate", input.animate)
  const exit = resolve("exit", input.exit)
  const initial = input.initial === false ? undefined : resolve("initial", input.initial)
  return {
    initial: input.initial === false ? false : initial?.style,
    animate: animate?.style,
    exit: exit?.style,
    animateTransition: animate?.transition,
    exitTransition: exit?.transition,
  }
}
