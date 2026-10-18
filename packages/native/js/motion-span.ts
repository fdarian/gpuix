/// What `motion.span` means to the adapters, shared by React and Solid: the
/// properties an inline span can animate, `variants`, the gesture targets, and the
/// props that have no effect on a box a paragraph paints.

import type { LayoutDrag, MotionLayoutProps, MotionStyle, MotionTransition } from "./host.js"
import type { MotionValue } from "./motion-value.js"

/**
 * What CSS lets an inline box show, and gpuix's inline model draws: the paragraph reads these
 * from the style sampled for the frame, and lays itself out again when one changes the width of
 * the text or the space along the line. Mirrors `Scope::InlineBox`.
 */
const MOTION_SPAN_KEYS = [
  "opacity",
  "color",
  "backgroundColor",
  "textDecorationColor",
  "outlineColor",
  "outlineWidth",
  "outlineOffset",
  "borderColor",
  "borderTopColor",
  "borderRightColor",
  "borderBottomColor",
  "borderLeftColor",
  "borderWidth",
  "borderTopWidth",
  "borderRightWidth",
  "borderBottomWidth",
  "borderLeftWidth",
  "borderRadius",
  "borderTopLeftRadius",
  "borderTopRightRadius",
  "borderBottomLeftRadius",
  "borderBottomRightRadius",
  "padding",
  "paddingTop",
  "paddingRight",
  "paddingBottom",
  "paddingLeft",
  "margin",
  "marginTop",
  "marginRight",
  "marginBottom",
  "marginLeft",
  "fontSize",
  "fontWeight",
  "fontVariationSettings",
  "lineHeight",
  "textUnderlineOffset",
  "textDecorationThickness",
] as const

export type MotionSpanKey = (typeof MOTION_SPAN_KEYS)[number]

/**
 * Framer's transform keys. A transform does not apply to a non-atomic inline box in CSS, so
 * they are accepted, as Framer accepts them on any element, and have no effect.
 */
const IGNORED_STYLE_KEYS = [
  "x",
  "y",
  "z",
  "translateX",
  "translateY",
  "translateZ",
  "scale",
  "scaleX",
  "scaleY",
  "scaleZ",
  "rotate",
  "rotateX",
  "rotateY",
  "rotateZ",
  "skew",
  "skewX",
  "skewY",
  "originX",
  "originY",
  "originZ",
  "transformPerspective",
] as const

export type MotionSpanTransformStyle = { [Key in (typeof IGNORED_STYLE_KEYS)[number]]?: number | string }

/**
 * What a `motion.span` can animate: colours and the opacity they are painted with, the font of its
 * text, its padding, margin, border and outline, the corners of its background and the lines drawn
 * over its text. Width, height, position and the like are not a span's. The transform keys are
 * accepted and do nothing, as a transform does nothing to a non-atomic inline box.
 */
export type MotionSpanStyle = Pick<MotionStyle, MotionSpanKey> & MotionSpanTransformStyle

/** The name of a variant, or several: later ones win a property they share. */
export type MotionVariantLabels = string | string[]

/** A variant's `transition` is the one its animation plays with, in place of the component's. */
export type MotionSpanVariant = MotionSpanStyle & { transition?: MotionTransition }

export type MotionSpanVariants = Record<
  string,
  // A method type so that `(direction: number) => ...` is assignable: `custom` is whatever the app passes.
  MotionSpanVariant | { variant(custom: unknown): MotionSpanVariant }["variant"]
>

/** What a span animates to while a gesture is on it, with the transition that animation plays with. */
export type MotionSpanGestureTarget = MotionSpanStyle & { transition?: MotionTransition }

/** Framer's `viewport` option of `whileInView`. */
export interface MotionSpanViewport {
  /** Stay in the in-view state once it has been reached. */
  once?: boolean
  /** The element whose box is the viewport; the window when it has none. */
  root?: { current: object | null }
  /** `rootMargin` of the `IntersectionObserver`. */
  margin?: string
  /** How much of the span has to show: `"some"` (any), `"all"`, or a fraction. */
  amount?: "some" | "all" | number
}

export interface MotionSpanAnimation {
  /** Where the animation starts; `false` starts it at `animate`. */
  initial?: MotionSpanStyle | MotionVariantLabels | false
  animate?: MotionSpanStyle | MotionVariantLabels
  /** Played while the span leaves `AnimatePresence`. */
  exit?:
    | MotionSpanStyle
    | MotionVariantLabels
    | { exit(custom: unknown): MotionSpanStyle | MotionVariantLabels }["exit"]
  /** While the pointer is over the span (`:hover`). */
  whileHover?: MotionSpanGestureTarget | MotionVariantLabels
  /** While a press that began on the span goes on (`:active`). */
  whileTap?: MotionSpanGestureTarget | MotionVariantLabels
  /** While the span has focus that `:focus-visible` matches; it needs a `tabIndex` to take focus. */
  whileFocus?: MotionSpanGestureTarget | MotionVariantLabels
  /** While any of the span (see `viewport`) is in the viewport. */
  whileInView?: MotionSpanGestureTarget | MotionVariantLabels
  viewport?: MotionSpanViewport
  variants?: MotionSpanVariants
  transition?: MotionTransition
  /** Handed to a variant or `exit` function. */
  custom?: unknown
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
  "whileDrag",
] as const

const IGNORED_PROP_NAMES = [...LAYOUT_PROPS, ...DRAG_PROPS] as const

/** The props a `motion.span` takes and ignores. */
export const IGNORED_SPAN_PROPS: readonly string[] = IGNORED_PROP_NAMES

/**
 * Framer's layout and drag props. A layout animation moves a box with a transform and a drag moves it
 * the same way, which does nothing to a non-atomic inline box, so a `motion.span` takes them and they
 * have no effect: no layout animation runs and no callback of one is called.
 */
export type MotionSpanIgnoredProps = {
  [Prop in Exclude<(typeof IGNORED_PROP_NAMES)[number], "layout" | "layoutId" | "layoutDrag">]?: unknown
} & {
  layout?: MotionLayoutProps["layout"]
  layoutId?: string
  layoutDrag?: LayoutDrag
}

/** `props` without the ones a span ignores, which must not reach the host as the props of an element. */
export function withoutIgnoredSpanProps<Props extends object>(props: Props): Props {
  const kept: Record<string, unknown> = {}
  for (const key of Object.keys(props)) {
    if (!IGNORED_SPAN_PROPS.includes(key)) kept[key] = Reflect.get(props, key)
  }
  return kept as Props
}

/** `style` without the transform keys, which a span ignores whatever they hold, a motion value too. */
export function withoutIgnoredSpanStyle<Style>(style: Style): Style {
  if (typeof style !== "object" || style === null) return style
  if (!IGNORED_STYLE_KEYS.some((key) => Object.hasOwn(style, key))) return style
  const kept: Record<string, unknown> = {}
  for (const key of Object.keys(style)) {
    if (!(IGNORED_STYLE_KEYS as readonly string[]).includes(key)) kept[key] = Reflect.get(style, key)
  }
  return kept as Style
}

/** Throws for a motion value bound to a property a span cannot show, naming it. */
export function assertSpanMotionValues(values: Record<string, MotionValue> | undefined): void {
  for (const key of Object.keys(values ?? {})) {
    if (!(MOTION_SPAN_KEYS as readonly string[]).includes(key)) {
      throw new Error(
        `A span cannot take a motion value in style.${key}: an inline box can only show ${MOTION_SPAN_KEYS.join(", ")}`
      )
    }
  }
}

function assertSpanStyle(where: string, style: object): void {
  for (const key of Object.keys(style)) {
    const known = (MOTION_SPAN_KEYS as readonly string[]).includes(key) || (IGNORED_STYLE_KEYS as readonly string[]).includes(key)
    if (!known) {
      throw new Error(
        `motion.span cannot animate "${key}" in ${where}: an inline box can only show ${MOTION_SPAN_KEYS.join(", ")}`
      )
    }
  }
}

/** `style` without the keys a span ignores. */
function shown(style: object): MotionSpanStyle {
  return withoutIgnoredSpanStyle(style) as MotionSpanStyle
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
    return { style: shown(target), transition: undefined }
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
  return { style: shown(style), transition }
}

/** A gesture target: a style object with a `transition` of its own, or variant names. */
function resolveGesture(
  where: string,
  target: MotionSpanGestureTarget | MotionVariantLabels,
  variants: MotionSpanVariants | undefined,
  custom: unknown
): SpanTarget {
  if (typeof target === "object" && !Array.isArray(target)) {
    const own: Record<string, unknown> = { ...target }
    const transition = own.transition as MotionTransition | undefined
    delete own.transition
    assertSpanStyle(where, own)
    return { style: shown(own), transition }
  }
  return resolveTarget(where, target, variants, custom)
}

export type SpanGestureName = "whileHover" | "whileTap" | "whileFocus" | "whileInView"

const GESTURES: readonly SpanGestureName[] = ["whileHover", "whileTap", "whileFocus", "whileInView"]

export interface ResolvedSpanMotion {
  initial: MotionSpanStyle | false | undefined
  animate: MotionSpanStyle | undefined
  exit: MotionSpanStyle | undefined
  /** The `transition` of the variants `animate` names, for the animation to the span's resting state. */
  animateTransition: MotionTransition | undefined
  /** The same for `exit`. */
  exitTransition: MotionTransition | undefined
  /** The target of each `while*` prop the span has, with the transition its animation plays with. */
  gestures: Partial<Record<SpanGestureName, SpanTarget>>
}

export interface SpanMotionInput {
  initial: MotionSpanAnimation["initial"]
  animate: MotionSpanAnimation["animate"]
  /** Already resolved against `custom`, so not a function. */
  exit: MotionSpanStyle | MotionVariantLabels | undefined
  variants: MotionSpanVariants | undefined
  custom: unknown
  gestures?: Partial<Record<SpanGestureName, MotionSpanGestureTarget | MotionVariantLabels | undefined>>
}

/**
 * The style objects `animate`, `initial`, `exit` and the `while*` props stand for once their
 * variant names are looked up, with every key checked against what a span can animate. The
 * `variants` are checked whole, whether or not a target names them.
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
  const gestures: ResolvedSpanMotion["gestures"] = {}
  for (const name of GESTURES) {
    const target = input.gestures?.[name]
    if (target !== undefined) gestures[name] = resolveGesture(name, target, input.variants, input.custom)
  }
  return {
    initial: input.initial === false ? false : initial?.style,
    animate: animate?.style,
    exit: exit?.style,
    animateTransition: animate?.transition,
    exitTransition: exit?.transition,
    gestures,
  }
}

/** The fields of a motion description that the gestures of a span add, in the shape native takes. */
export function gestureDescription(
  gestures: ResolvedSpanMotion["gestures"],
  wire: (transition: MotionTransition | undefined) => unknown,
  inView: boolean
): Record<string, unknown> {
  const description: Record<string, unknown> = {}
  for (const name of GESTURES) {
    const target = gestures[name]
    if (target === undefined) continue
    const transition = wire(target.transition)
    description[name] = transition === undefined ? { ...target.style } : { ...target.style, transition }
  }
  if (gestures.whileInView !== undefined) description.inView = inView
  return description
}

/** The `IntersectionObserver` options Framer's `viewport` stands for. */
export function viewportObserverInit(viewport: MotionSpanViewport | undefined): {
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
