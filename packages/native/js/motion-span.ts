/// What `motion.span` means to the adapters, shared by React and Solid: the
/// properties an inline span can animate and the props that have no effect on a box a
/// paragraph paints. Its variants and gestures are `motion-variants.ts`'s.

import type { LayoutDrag, MotionLayoutProps, MotionStyle, MotionTransition } from "./host.js"
import type { MotionTargetScope, MotionAnimation, MotionGestureTarget, MotionVariants, MotionViewport } from "./motion-variants.js"
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

export type MotionSpanVariant = MotionGestureTarget<MotionSpanStyle>

export type MotionSpanVariants = MotionVariants<MotionSpanStyle>

/** What a span animates to while a gesture is on it, with the transition that animation plays with. */
export type MotionSpanGestureTarget = MotionGestureTarget<MotionSpanStyle>

export type MotionSpanViewport = MotionViewport

/** The animation props of a `motion.span`. */
export type MotionSpanAnimation = MotionAnimation<MotionSpanStyle>

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

/** A span's targets hold the keys it shows and the transform keys it ignores. */
export const SPAN_SCOPE: MotionTargetScope = {
  component: "motion.span",
  check: assertSpanStyle,
  shown: (style) => withoutIgnoredSpanStyle(style),
}
