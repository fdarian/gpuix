import type { AriaProps } from "./aria.js"
import type {
  DebugFrameOverlayStats,
  EdgeInsets,
  ElementBounds,
  EventPayload,
  GpuixRenderer,
  HighlightMatch,
  PathPromptOptions,
  WindowInsets as NativeWindowInsets,
} from "../index.js"
import type { PointerEventProps } from "./events/types.js"
import type { ScrollEvent, WheelEvent } from "./scroll.js"
import type { MotionValue } from "./motion-value.js"

export type {
  DebugFrameOverlayStats,
  EdgeInsets,
  ElementBounds,
  HighlightMatch,
  NativeWindowInsets,
  PathPromptOptions,
}

export * from "./events/listeners.js"
export * from "./events/types.js"
export type { AriaProps }
export * from "./clipboard.js"
export {
  document,
  GpuixEventTarget,
  window,
  type AddEventListenerOptions,
  type WindowEventListener,
} from "./events/window-target.js"
export { preventEventDefault, type DomEvent } from "./events/dispatch.js"
export * from "./focus.js"
export * from "./forms.js"
export * from "./bounds.js"
export * from "./host-runtime.js"
export * from "./keyframes.js"
export * from "./layout-animation.js"
export * from "./match-media.js"
export * from "./motion-bindings.js"
export * from "./motion-config.js"
export * from "./motion-value.js"
export * from "./mutations.js"
export * from "./observers.js"
export * from "./renderer-state.js"
export * from "./text-selection.js"
export * from "./scroll.js"
export * from "./svg.js"

export type DimensionValue = number | string

/**
 * Every `StyleDesc` longhand whose value is a number, a length, a colour or a
 * shadow list can be animated, plus the `padding`, `margin`, `borderWidth`,
 * `borderRadius` and `gap` shorthands, which expand to their longhands.
 * Discrete properties (`display`, `position`, ...) are an error.
 *
 * `width`, `height` and the min/max sizes move between px and `%` (a share of
 * the parent's content box, or of the window for a root), and settle on the
 * unit they were given. `height` also takes `"auto"`, the height the content
 * wants. `width: "auto"` is an error: the content's width needs a second layout
 * pass to measure.
 *
 * Each property takes a target or keyframes (`opacity: [0, 1, 0.5]`); see
 * `MotionKeyframes`.
 */
export type MotionStyle = {
  [Property in keyof MotionStyleTargets]?: MotionKeyframes<NonNullable<MotionStyleTargets[Property]>>
}

/**
 * One target, or keyframes: a list the property animates through, over one
 * `duration` (spread by `times`). A `null` stands for the keyframe before it, or
 * for the value the property starts from when it comes first. A list is
 * keyframes unless it is itself one value of the property (`BoxShadow[]`, a
 * `strokeDasharray` list of numbers), in which case keyframes are a list of
 * those. Springs animate between two keyframes.
 */
export type MotionKeyframes<Value> = Value | Array<Value | null>

export type MotionStyleTargets = Pick<
  StyleDesc,
  | "top"
  | "right"
  | "bottom"
  | "left"
  | "flexGrow"
  | "flexShrink"
  | "flexBasis"
  | "gap"
  | "rowGap"
  | "columnGap"
  | "padding"
  | "paddingTop"
  | "paddingRight"
  | "paddingBottom"
  | "paddingLeft"
  | "margin"
  | "marginTop"
  | "marginRight"
  | "marginBottom"
  | "marginLeft"
  | "opacity"
  | "backgroundColor"
  | "color"
  | "borderWidth"
  | "borderTopWidth"
  | "borderRightWidth"
  | "borderBottomWidth"
  | "borderLeftWidth"
  | "borderColor"
  | "borderRadius"
  | "borderTopLeftRadius"
  | "borderTopRightRadius"
  | "borderBottomLeftRadius"
  | "borderBottomRightRadius"
  | "boxShadow"
  | "aspectRatio"
  | "outlineWidth"
  | "outlineColor"
  | "outlineOffset"
  | "fontSize"
  | "fontWeight"
  | "lineHeight"
  | "scrollbarColor"
  | "scrollbarWidth"
  | "selectionColor"
> & {
  width?: MotionLength
  height?: MotionLength | "auto"
  minWidth?: MotionLength
  minHeight?: MotionLength
  maxWidth?: MotionLength
  maxHeight?: MotionLength
}

/** Pixels, or a percentage string such as `"50%"`. */
export type MotionLength = number | `${number}%`

/**
 * `StyleDesc` where each animatable property may hold a motion value instead.
 * The adapter binds it to the element rather than sending it as style.
 */
export type MotionValueStyle = Omit<StyleDesc, keyof MotionStyle> & {
  [Property in keyof MotionStyle]?: StyleDesc[Property] | MotionValue
}

export type MotionEase =
  | "linear"
  | "ease"
  | "easeIn"
  | "easeOut"
  | "easeInOut"
  | [number, number, number, number]
  /** A CSS `<easing-function>`, as `transitionTimingFunction` takes. */
  | "ease-in"
  | "ease-out"
  | "ease-in-out"
  | "step-start"
  | "step-end"
  | `cubic-bezier(${string})`
  | `steps(${string})`
  | `linear(${string})`

/** How a transition plays again once it reaches its last keyframe. */
export interface MotionRepeatOptions {
  /**
   * Extra iterations after the first. `Infinity` loops for ever, and the
   * animation never completes.
   */
  repeat?: number
  /**
   * `"loop"` plays every iteration from the first keyframe; `"reverse"` plays
   * odd iterations backwards in time, so their easing runs backwards too;
   * `"mirror"` plays them through the keyframes in reverse order, with the same
   * timing and easing as the first.
   */
  repeatType?: "loop" | "reverse" | "mirror"
  /** Seconds held at the end of each iteration before the next. */
  repeatDelay?: number
}

export interface MotionTweenTransition extends MotionRepeatOptions {
  type?: "tween"
  /** Duration in seconds, of one iteration. */
  duration?: number
  /** Delay in seconds. */
  delay?: number
  /** One easing, or one per keyframe segment. */
  ease?: MotionEase | MotionEase[]
  /**
   * Where each keyframe sits in an iteration, from 0 to 1 and ascending: one
   * per keyframe. Evenly spaced when omitted.
   */
  times?: number[]
  stiffness?: never
  damping?: never
  mass?: never
  bounce?: never
  restSpeed?: never
  restDelta?: never
}

interface MotionSpringBase extends MotionRepeatOptions {
  type: "spring"
  /** Delay in seconds. The spring holds its start until the delay passes. */
  delay?: number
  ease?: never
  /** A spring only animates between two keyframes. */
  times?: never
  /** Speed, in units per second, below which the spring may settle. */
  restSpeed?: number
  /** Distance from the target, in units, within which the spring may settle. */
  restDelta?: number
}

/** A spring described by its physics. Defaults to stiffness 100, damping 10, mass 1. */
export interface MotionSpringPhysicsTransition extends MotionSpringBase {
  stiffness?: number
  damping?: number
  mass?: number
  duration?: never
  bounce?: never
}

/** A spring described by how long it takes to settle and how much it overshoots. */
export interface MotionSpringDurationTransition extends MotionSpringBase {
  /** Seconds to settle, 0.01 to 10. Defaults to 0.8. */
  duration?: number
  /** 0 is critically damped; toward 1 it oscillates longer. Defaults to 0.3. */
  bounce?: number
  stiffness?: never
  damping?: never
  mass?: never
}

export type MotionSpringTransition =
  | MotionSpringPhysicsTransition
  | MotionSpringDurationTransition

export type MotionCurve = MotionTweenTransition | MotionSpringTransition

/**
 * One curve for every property, or a curve per property as in Framer Motion.
 * A property resolves to `transition[property]`, then `transition.default`,
 * then the curve fields at the top level, then a 0.3 s `easeOut` tween. Each
 * level replaces the next as a whole. A shorthand key (`padding`) sets all its
 * longhands, and a longhand beside it wins.
 */
export type MotionTransition = MotionCurve & {
  default?: MotionCurve
  /** The glide of the `layout` and `layoutId` props. Falls back to the rest of this transition, then to a 0.45 s tween. */
  layout?: MotionCurve
} & { [Property in keyof MotionStyle]?: MotionCurve }

export interface MotionProps {
  initial?: MotionStyle | false
  /** Left out by an element that only has `layout`. */
  animate?: MotionStyle
  /** Target applied while this node is leaving `AnimatePresence`. */
  exit?: MotionStyle
  transition?: MotionTransition
}

/**
 * The attributes of an SVG shape (`motion.path`, `motion.circle`, ...) that can
 * animate. They go through the same engine as `MotionStyle`: tweens, springs,
 * per-key transitions, and Oklab colours.
 *
 * `pathLength`, `pathOffset` and `pathSpacing` are Framer's normalised draw
 * keys, each a fraction of the shape's length: `pathLength` is how much is
 * stroked, `pathOffset` shifts where the stroke starts, `pathSpacing` is the
 * gap after it (default 1, so one dash). They set the dash for you, so use them
 * instead of `strokeDasharray` and `strokeDashoffset`. A bare `pathLength`
 * *prop* is the DOM attribute instead: it declares a length that dash values
 * are measured in.
 *
 * `d` morphs when both paths use the same commands, after relative commands are
 * made absolute; otherwise it switches halfway. `strokeDasharray` lists of
 * different lengths are repeated to a common length.
 */
export type SvgMotionStyle = {
  [Attribute in keyof SvgMotionTargets]?: MotionKeyframes<NonNullable<SvgMotionTargets[Attribute]>>
}

interface SvgMotionTargets {
  pathLength?: number
  pathOffset?: number
  pathSpacing?: number
  strokeDasharray?: number | string | number[]
  strokeDashoffset?: number
  strokeWidth?: number
  fill?: string
  stroke?: string
  opacity?: number
  fillOpacity?: number
  strokeOpacity?: number
  cx?: number
  cy?: number
  r?: number
  rx?: number
  ry?: number
  x?: number
  y?: number
  width?: number
  height?: number
  x1?: number
  y1?: number
  x2?: number
  y2?: number
  d?: string
}

/** Same resolution as `MotionTransition`, keyed by `SvgMotionStyle`. */
export type SvgMotionTransition = MotionCurve & {
  default?: MotionCurve
} & { [Attribute in keyof SvgMotionStyle]?: MotionCurve }

export interface SvgMotionProps {
  initial?: SvgMotionStyle | false
  animate: SvgMotionStyle
  /** Target applied while this node is leaving `AnimatePresence`. */
  exit?: SvgMotionStyle
  transition?: SvgMotionTransition
}

/**
 * CSS `cursor` keywords GPUI can paint. An unlisted keyword is ignored, like
 * every other invalid style value.
 */
export type CursorValue =
  | "default"
  | "auto"
  | "pointer"
  | "text"
  | "vertical-text"
  | "crosshair"
  | "grab"
  | "grabbing"
  | "move"
  | "all-scroll"
  | "col-resize"
  | "row-resize"
  | "ew-resize"
  | "ns-resize"
  | "nwse-resize"
  | "nesw-resize"
  | "n-resize"
  | "e-resize"
  | "s-resize"
  | "w-resize"
  | "ne-resize"
  | "nw-resize"
  | "se-resize"
  | "sw-resize"
  | "not-allowed"
  | "no-drop"
  | "alias"
  | "copy"
  | "context-menu"
  | "none"

export interface BoxShadow {
  offsetX: number
  offsetY: number
  blurRadius: number
  spreadRadius: number
  color: string
  /** Draw inside the border box, like CSS `inset`. */
  inset?: boolean
}

export interface LinearGradientStop {
  color: string
  /** Position along the gradient from 0 to 1. */
  position: number
}

export interface LinearGradientBackground {
  type: "linear-gradient"
  /** CSS angle in degrees. 0 points up and values increase clockwise. */
  angle: number
  stops: [LinearGradientStop, LinearGradientStop]
  colorSpace?: "srgb" | "oklab"
}

export interface StyleDesc {
  /** `"flex"`, `"grid"`, `"block"` (the default), `"none"`, which builds no box at all, or
   *  `"contents"`, which builds no box either and lays its children out as the parent's own.
   *  The `inline-*` values are not supported. */
  display?: string
  visibility?: string
  /** `"row"`, `"row-reverse"`, `"column"` or `"column-reverse"`. */
  flexDirection?: string
  /** `"nowrap"`, `"wrap"` or `"wrap-reverse"`. */
  flexWrap?: string
  flexGrow?: number
  flexShrink?: number
  flexBasis?: number
  alignItems?: string
  alignSelf?: string
  alignContent?: string
  justifyContent?: string
  gap?: number
  rowGap?: number
  columnGap?: number
  /** Reorders the children of a flex or grid container, lowest first. Ties keep document
   *  order. Layout and paint follow it; focus order stays in the document order. */
  order?: number
  justifyItems?: string
  justifySelf?: string
  /** A CSS track list (`"1fr 200px"`, `"repeat(auto-fill, minmax(120px, 1fr))"`,
   *  `"[sidebar] 240px [main] 1fr"`, `"none"`) or a column count `n`, which means
   *  `repeat(n, minmax(0, 1fr))`. Units are px, `%`, `fr`; `auto`, `min-content`,
   *  `max-content`, `minmax()`, `fit-content()` and `repeat()` are supported. */
  gridTemplateColumns?: number | string
  gridTemplateRows?: number | string
  gridColumnMin?: "zero" | "min-content" | "max-content"
  gridRowMin?: "zero" | "min-content" | "max-content"
  /** Size of implicitly created columns / rows: a CSS track size list or a pixel count. */
  gridAutoColumns?: number | string
  gridAutoRows?: number | string
  /** `"row"`, `"column"`, optionally followed by `"dense"`. */
  gridAutoFlow?: string
  /** Quoted rows of area names: `'"header header" "side main"'`. Use `.` for an empty cell. */
  gridTemplateAreas?: string
  /** Item placement: a line number (negative counts from the end), `"span 2"`, a line or
   *  area name, or a pair such as `"1 / 3"`. */
  gridColumn?: number | string
  gridRow?: number | string
  gridColumnStart?: number | string
  gridColumnEnd?: number | string
  gridRowStart?: number | string
  gridRowEnd?: number | string
  /** An area name, or `"row-start / column-start / row-end / column-end"`. */
  gridArea?: number | string

  /** px, a percentage, `"auto"`, or on a direct child of `<anchored>` the trigger's border box:
   *  `"anchor-size(width)"` / `"anchor-size(height)"`. Accepted by the six size properties only,
   *  and resolved natively on every layout. */
  width?: DimensionValue
  height?: DimensionValue
  minWidth?: DimensionValue
  minHeight?: DimensionValue
  maxWidth?: DimensionValue
  maxHeight?: DimensionValue

  padding?: number
  paddingTop?: number
  paddingRight?: number
  paddingBottom?: number
  paddingLeft?: number

  /** px, a percentage of the containing block's width, or `"auto"` (which absorbs free
   *  space in flex layouts). */
  margin?: DimensionValue
  marginTop?: DimensionValue
  marginRight?: DimensionValue
  marginBottom?: DimensionValue
  marginLeft?: DimensionValue

  /** `"static"` (the default), `"relative"`, `"absolute"`, `"fixed"` or `"sticky"`. An absolute box
   *  resolves `top`/`right`/`bottom`/`left` and percentage sizes against the padding box
   *  of its nearest non-static ancestor (a sticky one counts), a fixed one against the window.
   *  `"sticky"` lays out like `"relative"` with the insets ignored and then stays inside the
   *  scrollport of its nearest scroll container (or the window), inset by `top`, `right`,
   *  `bottom` and `left`, without leaving its parent's content box. */
  position?: string
  /** Paint and hit-test order among positioned siblings (`position` other than static).
   *  Negative paints below in-flow siblings, positive above. Only reorders siblings of the same
   *  parent, never against cousins. */
  zIndex?: number
  top?: DimensionValue
  right?: DimensionValue
  bottom?: DimensionValue
  left?: DimensionValue

  /** The CSS shorthand (`"linear-gradient(...) center / cover no-repeat, #fff"`: layers
   *  then a final colour), a plain colour, or the two-stop object form. The
   *  `background*` longhands below override parts of the shorthand. */
  background?: string | LinearGradientBackground
  backgroundColor?: string
  /** Comma list of `linear-`, `radial-`, `conic-gradient()` and `repeating-` forms
   *  (any number of stops and colour hints, `in srgb | oklab`), `none`, or `url()`
   *  (valid but not drawn). The first layer is on top. */
  backgroundImage?: string
  /** CSS lists, one entry per layer, repeating to cover the layers. */
  backgroundSize?: string
  backgroundPosition?: string
  backgroundRepeat?: string
  backgroundOrigin?: "border-box" | "padding-box" | "content-box" | string
  /** `text` paints the layer through the glyphs of the element and its
   *  descendants. The box values clip the gradient layers only; `backgroundColor`
   *  is clipped to the glyphs when the last layer's clip is `text`. */
  backgroundClip?: "border-box" | "padding-box" | "content-box" | "text" | string
  /** Text colour, inherited. Every colour prop accepts CSS colours: hex, named,
   *  `rgb()`, `hsl()`, `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()`, relative
   *  colours, and the context-aware forms below, which nest freely:
   *  - `currentColor`: this element's computed `color`, so `borderColor:
   *    "currentColor"` follows the text.
   *  - `light-dark(a, b)`: `a` under a light `colorScheme`, `b` under a dark one.
   *  - `color-mix(in oklab, a 30%, b)`: any of srgb, srgb-linear, display-p3,
   *    a98-rgb, prophoto-rgb, rec2020, xyz, xyz-d50, xyz-d65, lab, lch, oklab,
   *    oklch, hsl, hwb, with `shorter|longer|increasing|decreasing hue` for the
   *    polar ones.
   *  - `color(display-p3 1 0 0)`: the same spaces, clipped to sRGB.
   *  An invalid colour is ignored. */
  color?: string
  /** CSS `color-scheme`, inherited: `"normal"` (light), `"light"`, `"dark"`,
   *  `"light dark"` (follows the window appearance), `"only light"`, `"only dark"`.
   *  It picks the side of `light-dark()` and sets the default text colour (CSS
   *  `CanvasText`): black under a light scheme, white under a dark one, until some
   *  element declares a `color`. */
  colorScheme?: string
  opacity?: number

  borderWidth?: number
  borderTopWidth?: number
  borderRightWidth?: number
  borderBottomWidth?: number
  borderLeftWidth?: number
  /** One to four colours, like CSS: `"red"`, `"red blue"` (top-bottom, left-right),
   *  `"red blue green"`, `"red blue green white"` (top, right, bottom, left). One
   *  invalid colour ignores the whole value. */
  borderColor?: string
  borderTopColor?: string
  borderRightColor?: string
  borderBottomColor?: string
  borderLeftColor?: string
  /** CSS shorthands: `"1px solid red"`, `"2px dashed"`, `"none"`. Width, style
   *  (`solid`, `dashed`, `none`) and colour in any order; no style means no border,
   *  a missing colour is `currentColor`, a missing width is `medium` (3px). Other
   *  line styles (`dotted`, `double`, ...) make the value invalid. `border` sets all
   *  sides, then `borderTop` ... `borderLeft` override one, then the longhands
   *  (`borderWidth`, `borderColor`, ...) override those. The line style is shared
   *  by every side, because GPUI has one. */
  border?: string
  borderTop?: string
  borderRight?: string
  borderBottom?: string
  borderLeft?: string
  borderRadius?: number
  borderTopLeftRadius?: number
  borderTopRightRadius?: number
  borderBottomLeftRadius?: number
  borderBottomRightRadius?: number
  /** One shadow, a list painted in the order given, or the CSS string
   *  (`"0 1px 2px rgba(0,0,0,.2), inset 0 0 0 1px red"`). In the string, lengths
   *  are `px` (or a bare `0`), `inset` and the colour may lead or trail the
   *  lengths, and a layer without a colour is `currentColor`, as in CSS. An
   *  invalid string paints no shadow. */
  boxShadow?: BoxShadow | BoxShadow[] | string
  /** CSS `filter`: a space-separated list of `blur()`, `brightness()`,
   *  `contrast()`, `grayscale()`, `hue-rotate()`, `invert()`, `opacity()`,
   *  `saturate()`, `sepia()` and `drop-shadow()`, applied in order to the element
   *  and everything in it, or `"none"`. Amounts are numbers or percentages;
   *  lengths are `px` (or a bare `0`); angles are `deg`, `rad`, `turn` or `grad`.
   *  A `drop-shadow()` colour defaults to `currentColor`. An invalid string is
   *  ignored, as in CSS. `url()` references are not supported. */
  filter?: string
  /** CSS `backdrop-filter`: the same functions as `filter`, applied to what is
   *  painted behind the element, within its border box. The element's own
   *  background should be translucent for the effect to show. */
  backdropFilter?: string
  /** CSS `mask-image`: a comma list of `linear-`, `radial-`, `conic-gradient()` and
   *  `repeating-` forms, `none`, or `url()` (valid but not drawn, so that layer is
   *  skipped). The element and everything in it is multiplied by the alpha of the
   *  layers, the first on top. */
  maskImage?: string
  /** CSS lists, one entry per layer, repeating to cover the layers. Unlike
   *  `backgroundOrigin`, `maskOrigin` starts at `border-box`. `fill-box` is
   *  `content-box` and `stroke-box` and `view-box` are `border-box`. `no-clip`
   *  and `margin-box` are not supported, so a list using them is ignored. */
  maskSize?: string
  maskPosition?: string
  maskRepeat?: string
  maskOrigin?: "border-box" | "padding-box" | "content-box" | string
  maskClip?: "border-box" | "padding-box" | "content-box" | string
  /** CSS `mask` shorthand: layers, `position / size`, repeat and origin / clip
   *  boxes. The longhands win over it. `mask-composite` and `mask-mode` have no
   *  props, so only their initial values (`add`, `alpha`, `match-source`) are
   *  accepted. An invalid string is ignored. */
  mask?: string
  /** CSS `clip-path`: `inset()`, `circle()`, `ellipse()` or `polygon()`, or `none`,
   *  with an optional `border-box` (default), `padding-box` or `content-box` before
   *  or after the shape. The element and everything in it is clipped to the shape.
   *  `margin-box`, `stroke-box` and `view-box` are `border-box` and `fill-box` is
   *  `content-box`. `url()`, `path()` and a box with no shape are invalid, so the
   *  value is ignored. Hit testing is not clipped yet. */
  clipPath?: string
  /** Applies to every side. */
  borderStyle?: "solid" | "dashed"
  /** Width divided by height, as a number or a CSS ratio (`"16 / 9"`, `"1"`). Fills in
   *  whichever axis has no definite size. An `<img>` uses its intrinsic ratio unless this is set. */
  aspectRatio?: number | string
  /** CSS `outline`: a line outside the border box. Takes no layout space, so it
   *  can appear on focus without moving anything. Width defaults to 1. */
  outlineWidth?: number
  outlineColor?: string
  /** Gap between the border box and the outline. Negative draws it inside. */
  outlineOffset?: number

  fontSize?: number
  fontFamily?: string
  fontWeight?: string | number
  /** CSS `font-style`, inherited. Applies to the element and to inline runs. */
  fontStyle?: "normal" | "italic" | "oblique"
  textAlign?: string
  lineHeight?: number
  whiteSpace?: "normal" | "nowrap"
  textOverflow?: "ellipsis" | "ellipsis-start"
  lineClamp?: number
  textDecoration?: "underline" | "line-through" | "none"
  /** CSS `font-variant-numeric`, inherited: `tabular-nums`, `proportional-nums`,
   *  `lining-nums`, `oldstyle-nums`, `slashed-zero`, `diagonal-fractions`,
   *  `stacked-fractions`, `ordinal`, or `normal`. One keyword per group (figure,
   *  spacing, fraction), combined with spaces. Needs a font with the feature.
   *  Not available inside a state style. */
  fontVariantNumeric?: string
  /** CSS `font-feature-settings`, inherited: `'"tnum" 1, "liga" 0'`. A tag is four
   *  characters; the value is an integer, `on`, `off`, or omitted (1). Wins over
   *  `fontVariantNumeric` on a tag both name. Not available inside a state style. */
  fontFeatureSettings?: string

  /** `"visible"` (default); `"clip"` clips; `"hidden"` clips and is a scroll container that
   *  only code scrolls (`scrollTo`, `scrollTop`, `scrollIntoView`), not the wheel or the keys;
   *  `"scroll"` and `"auto"` scroll for the user too (scrollbars are overlays, so they differ
   *  in nothing). Overflow never changes `display` or the element's children. */
  overflow?: string
  overflowX?: string
  overflowY?: string
  /** `"auto"` follows the system appearance (a 50% black thumb on light, 50% white on dark), or
   *  `"<thumb> <track>"` sets both colours, each any CSS colour. The track is the faint
   *  strip that shows while the pointer is over the bar. Anything else is ignored, as in CSS. */
  scrollbarColor?: string
  /** `"auto"` (the default) or `"thin"` for a narrower bar. `"none"` draws no bar and leaves
   *  the container scrollable. The bar is a macOS-style overlay: hidden at rest, shown while
   *  the offset moves, faded out after about a second, and thicker under the pointer. */
  scrollbarWidth?: "auto" | "thin" | "none"
  /** What a scroller at its limit does with the wheel it cannot use, like CSS. `"auto"`
   *  (default) passes it to the scroller around it, `"contain"` and `"none"` keep it. */
  overscrollBehavior?: "auto" | "contain" | "none"
  overscrollBehaviorX?: "auto" | "contain" | "none"
  overscrollBehaviorY?: "auto" | "contain" | "none"
  /** `"smooth"` animates `scrollTo` and `scrollIntoView` calls that name no `behavior`
   *  (about 300 to 450 ms, firing `onScroll` each frame). An explicit `behavior` wins. */
  scrollBehavior?: "auto" | "smooth"

  cursor?: CursorValue
  /** `"none"` makes the element never the target of a pointer event, so the
   *  pointer reaches what is under it. It inherits, and a descendant's `"auto"`
   *  restores it, like CSS. Every other element is a target where it paints,
   *  whatever its fill or position. */
  pointerEvents?: "auto" | "none"

  /** "none" opts this element and its subtree out of text selection.
   *  Inherited like the CSS property, so a toolbar can disable it once. */
  userSelect?: "text" | "none" | "auto"
  /** Selection wash colour for this subtree. Defaults to the theme accent at 35%. */
  selectionColor?: string

  /**
   * CSS transitions, in CSS syntax with kebab-case property names, as in a DOM
   * inline style: `"background-color 150ms ease-out, opacity .2s 50ms"`, `"all
   * 1s"`, `"none"`. When the value of an animatable property changes (a style
   * prop, or a `hover` / `active` / `focusVisible` state style applying or
   * ending) the element animates from the value it showed to the new one. An
   * interrupted transition continues from the visible value. The element's
   * first style never transitions. `animate` on a `motion` element beats a
   * transition on the same property. Events: `onTransitionRun`,
   * `onTransitionStart`, `onTransitionEnd`, `onTransitionCancel`.
   * An invalid value is an error. `transition-behavior` is not supported.
   * A change between a length and `auto` (a `height` that is `auto` or unset,
   * and an unset `minHeight` / `maxHeight`, whose value is `auto` / `none`)
   * applies at once, as in a browser, unless `interpolateSize` allows it.
   */
  transition?: string
  /** `none`, `all`, or a list of CSS property names. Wins over `transition`. */
  transitionProperty?: string
  /** A list of `<time>`s (`150ms`, `.2s`). Wins over `transition`. */
  transitionDuration?: string
  /** A list of `ease`, `linear`, `ease-in`, `ease-out`, `ease-in-out`,
   *  `cubic-bezier()`, `steps()`, `step-start`, `step-end` or `linear()`. */
  transitionTimingFunction?: string
  /** A list of `<time>`s. Negative starts the transition part-way through. */
  transitionDelay?: string
  /**
   * `"allow-keywords"` lets a CSS transition run between a length and a sizing
   * keyword: `height` to or from `auto`, and from an unset `minHeight` /
   * `maxHeight`. `"numeric-only"` (the default) makes those changes discrete.
   * Inherited like the CSS property, so set it once on a container. `width` to
   * or from `auto` stays discrete either way. `motion` animates to `height:
   * "auto"` whatever this is set to.
   */
  interpolateSize?: "numeric-only" | "allow-keywords"

  /**
   * CSS animations, in CSS syntax as in a DOM inline style: `"fade-in 300ms
   * ease-out 100ms 2 alternate both"`, `"spin 1s linear infinite"`, `"none"`.
   * `name` is a set of keyframes made with `defineKeyframes`. The parts come in
   * any order; the first time is the duration and the second the delay. A comma
   * list runs several animations. An invalid value is an error. A name with no
   * keyframes animates nothing, as in a browser. The longhands below win over
   * this shorthand. `animation-composition`, `animation-timeline` and
   * `animation-range` are not supported.
   */
  animation?: string
  /** `none` or a list of names given to `defineKeyframes`. */
  animationName?: string
  /** A list of `<time>`s, or `auto` (`0s`). */
  animationDuration?: string
  /** A list of the same easing functions as `transitionTimingFunction`. */
  animationTimingFunction?: string
  /** A list of `<time>`s. Negative starts the animation part-way through. */
  animationDelay?: string
  /** A list of `infinite` or non-negative numbers. */
  animationIterationCount?: string
  /** A list of `normal`, `reverse`, `alternate` or `alternate-reverse`. */
  animationDirection?: string
  /** A list of `none`, `forwards`, `backwards` or `both`. */
  animationFillMode?: string
  /** A list of `running` or `paused`. */
  animationPlayState?: string

  // Pseudo-selector styles — applied by GPUI natively (no JS round-trip).
  // Nesting is one level deep: a state style cannot contain another.
  hover?: StateStyleDesc
  /** Makes this element a group for `groupHover` and `groupActive` on its
   *  descendants, like a Tailwind `group`. `true` is the default group; a string
   *  names it, so groups can nest. */
  group?: string | true
  /** Applied while the nearest ancestor `group` is hovered (the pointer is over
   *  it or anything inside it). `{ group: "card", ... }` follows the nearest
   *  ancestor named `card` instead of the default group. */
  groupHover?: GroupStateStyleDesc
  /** Applied while the mouse is down inside the nearest ancestor `group`. Same
   *  `group` field as `groupHover`. */
  groupActive?: GroupStateStyleDesc
  active?: StateStyleDesc
  /** While the element matches `:focus-visible`: focused by the keyboard (Tab),
   *  or a text control, or focus moved there by script from an element that
   *  matched. A mouse press on a button never shows it. Needs a focusable
   *  element. */
  focusVisible?: StateStyleDesc
  /** While the element or any descendant is focused, like CSS `:focus-within`.
   *  Works on any element, focusable or not. */
  focusWithin?: StateStyleDesc
  /** `false` keeps this focusable element at full opacity while another
   *  control has keyboard focus. Default dims it. Not inherited. */
  keyboardFocusDim?: boolean
}

export type StateStyleDesc = Omit<
  StyleDesc,
  | "hover"
  | "active"
  | "focusVisible"
  | "focusWithin"
  | "keyboardFocusDim"
  | "group"
  | "groupHover"
  | "groupActive"
  | "fontVariantNumeric"
  | "fontFeatureSettings"
>

/** A `groupHover`/`groupActive` style. Its `group` picks which ancestor group to
 *  follow (default group when omitted). */
export type GroupStateStyleDesc = StateStyleDesc & { group?: string | true }

// Element types supported by GPUIX. `span` is inline: it, raw strings and a
// `text` inside a `text` or `span` are runs of one paragraph, see README
// "Inline text".
export type ElementType =
  | "div"
  | "text"
  | "span"
  | "img"
  | "svg"
  | "canvas"
  | "input"
  | "textarea"
  | "label"
  | "form"
  | "anchored"
  | "code"
  | "diff"
  | "markdown"
  | "virtual-list"
  | SvgChildType

// ── Theme ────────────────────────────────────────────────────────────

/** Colours for one syntax capture class each. Every field is a CSS colour. */
export interface SyntaxTheme {
  comment?: string
  keyword?: string
  string?: string
  stringSpecial?: string
  escape?: string
  number?: string
  boolean?: string
  typeName?: string
  typeBuiltin?: string
  constructor?: string
  function?: string
  functionBuiltin?: string
  macroName?: string
  property?: string
  constant?: string
  variable?: string
  variableSpecial?: string
  parameter?: string
  operator?: string
  punctuation?: string
  tag?: string
  attribute?: string
  label?: string
  invalid?: string
}

/**
 * Every number that decides layout in the native text components.
 *
 * These live in the theme, not in Rust constants, so tuning a row height or a
 * heading scale is a framework render and needs no native rebuild.
 */
export interface GpuixMetrics {
  // Code blocks. Shared by <code> and the markdown fenced block.
  codeTextSize?: number
  codeLineHeight?: number
  codeGutterDigitWidth?: number
  codeGutterPaddingRight?: number
  codeGutterMinWidth?: number

  // Diffs
  diffTextSize?: number
  diffLineHeight?: number
  diffFileHeaderHeight?: number
  diffHunkHeaderHeight?: number
  diffNoticeHeight?: number
  diffBodyBottomPad?: number
  diffGutterWidth?: number
  diffMarkerWidth?: number
  diffAccentBarWidth?: number
  diffRowPaddingX?: number

  // Markdown
  mdTextSize?: number
  mdLineHeight?: number
  mdBlockGap?: number
  /** `[h1, h2, h3, h4to6]`. A shorter array leaves the rest at their defaults. */
  mdHeadingSizes?: number[]
  mdHeadingLineHeights?: number[]
  mdTableCellPadding?: number
  mdTableMinColumnWidth?: number
  mdTableMinColumnContent?: number
  mdInlineCodeRadius?: number
  /**
   * The fenced-block card. `<code>` paints no card, so these are
   * markdown-only: style a `<code>` block with its own `style` prop instead.
   */
  mdCodePaddingX?: number
  mdCodePaddingY?: number
  mdCodeRadius?: number
  mdCodeHeaderPaddingY?: number
  mdCodeHeaderTextSize?: number
}

/**
 * Theme tokens for the native text components. Every field is optional and
 * layers on top of the built-in dark theme (or light, via `appearance`).
 */
export interface GpuixTheme {
  appearance?: "dark" | "light"
  bg?: string
  border?: string
  text?: string
  textMuted?: string
  textFaint?: string
  textDim?: string
  accent?: string
  caret?: string
  codeText?: string
  codeWash?: string
  diffAdd?: string
  diffDel?: string
  diffHunkBg?: string
  fontSans?: string
  fontMono?: string
  syntax?: SyntaxTheme
  metrics?: GpuixMetrics
}

/** One `highlight` entry. See `HostProps.highlight`. */
export interface HighlightSpec {
  /**
   * Substring to match. Case-insensitive unless `caseSensitive` is set.
   *
   * A match never crosses a line, exactly like browser find. It DOES cross the
   * several host nodes a JSX renderer makes for one interpolated line, so
   * `<text>Hello {name}!</text>` matches `Hello Tommy`.
   */
  query?: string
  caseSensitive?: boolean
  /** Only match when neither neighbour is alphanumeric or `_`. */
  wholeWord?: boolean
  /**
   * Explicit `[start, end)` pairs in UTF-16 code units, the units `indexOf` and
   * `RegExp.exec` return. They index the declaring subtree's text, with a
   * newline between lines.
   *
   * A pair that splits a surrogate pair is rejected, not snapped. Native text
   * (`<code>`, `<markdown>`, `<diff>`) is not part of that text; use `query`.
   */
  ranges?: Array<[number, number]>
  /** Any CSS colour. Defaults to the theme accent at 30% alpha. */
  color?: string
  /** Colour for the match at `activeIndex`. Defaults to accent at 65%. */
  activeColor?: string
  /** Index of the match to highlight differently, for a find-bar cursor. */
  activeIndex?: number
  /**
   * How many MATCHES come before this subtree in your document, so `activeIndex`
   * is compared against `matchIndexOffset + n` for the nth match here.
   *
   * It is a match count, not a row index. Rows hold different numbers of
   * matches, so a row index cannot stand in for it.
   *
   * Only needed for virtualized content: a `<virtual-list>` mounts a window of
   * its rows, so native can only number what that window contains. Sum
   * `findRanges` over the rows before `windowStart`. Defaults to 0.
   *
   * A negative or fractional value is refused and the whole spec is dropped,
   * because a bad offset silently marks the wrong match.
   */
  matchIndexOffset?: number
  /** Corner radius of the wash. Defaults to 2. */
  radius?: number
}

// Props passed to elements.
// Element IDs are auto-generated numeric IDs (not user-settable).
// Framework adapters add their own children, key, and ref fields.
export interface HostProps extends AriaProps, PointerEventProps {
  style?: StyleDesc

  // Pointer, mouse and drag events, with `on…Capture` twins: see PointerEventProps.
  // `onDragEnter`, `onDragOver`, `onDragLeave` and `onDrop` report files dragged in
  // from the OS, with `event.dataTransfer.files`. A drop is delivered only after a
  // `dragover` whose handler called `preventDefault()`; otherwise `dragleave` fires.
  /**
   * A button went down on an element outside this one's logical subtree, for any
   * button. Anchored descendants are inside; an overlay that merely paints above is
   * outside. Runs at this element only, after `pointerdown` and `mousedown`, with
   * `event.target` set to the pressed element. Use for "click outside to close".
   */
  onMouseDownOutside?: (event: EventPayload) => void

  // ── Keyboard events ────────────────────────────────────────────
  /** Fires on the focused element, then on ancestors that declare it. A key
   *  listener does not make an element focusable: give it a `tabIndex`. */
  onKeyDown?: (event: KeyEvent) => void
  onKeyUp?: (event: KeyEvent) => void

  // ── Focus events ───────────────────────────────────────────────
  // Focus changes dispatch in DOM order: blur and focusout on the element
  // losing focus, then focus and focusin on the element gaining it.
  // `event.focusVisible` says whether the element matches `:focus-visible`.
  // Only a focusable element (`tabIndex`, or an enabled input/textarea) takes
  // focus; a listener does not make one focusable.
  //
  // `onFocus`/`onBlur` follow each framework's web behaviour. Solid, like
  // plain DOM: `onFocus`/`onBlur` do not bubble, `onFocusIn`/`onFocusOut` do.
  // React DOM: `onFocus` and `onBlur` bubble (they listen to `focusin` and
  // `focusout`), and there is no `onFocusIn`/`onFocusOut`.
  /** `focus`. Does not bubble. */
  onFocus?: (event: EventPayload) => void
  /** `blur`. Does not bubble. */
  onBlur?: (event: EventPayload) => void
  /** `focusin`. Bubbles to ancestors. Solid only. */
  onFocusIn?: (event: EventPayload) => void
  /** `focusout`. Bubbles to ancestors. Solid only. */
  onFocusOut?: (event: EventPayload) => void

  // ── Scroll events ──────────────────────────────────────────────
  /**
   * One wheel or trackpad tick over the element, before any scroller applies it. Like
   * the DOM `wheel`: `deltaY` is positive for down, `deltaMode` is always `0` (a mouse
   * wheel's lines are converted to pixels), and it fires whether or not anything scrolls.
   */
  onWheel?: (event: WheelEvent) => void
  /**
   * The scroll offset of an `overflow: "scroll"` or `"auto"` element changed, however it
   * moved: wheel, scrollbar drag, keys, `scrollTo`, `scrollIntoView`, a smooth animation,
   * or content shrinking under it. `scrollTop` and `scrollLeft` are read from the target,
   * as on the DOM element. Does not bubble. The position at the first paint is not
   * reported, only movement after it. Not for `<virtual-list>`; use `onVisibleRange`.
   */
  onScroll?: (event: ScrollEvent) => void

  // ── Layout events ──────────────────────────────────────────────
  /**
   * The element's border box, reported after the first paint and each time its
   * size changes: `width` and `height`, plus `x` and `y` in window coordinates.
   * Moving without resizing reports nothing. Supported on `div` and `text`.
   */
  onLayout?: (event: EventPayload) => void

  // ── Image loading (`<img>`) ──────────────────────────────────────
  /** The `src` decoded. Fires once per `src` change, with `naturalWidth` and
   *  `naturalHeight` in pixels. An empty `src` fires nothing. */
  onLoad?: (event: EventPayload) => void
  /** The `src` could not be read or decoded. Fires once per `src` change, with
   *  the reason in `value`. */
  onError?: (event: EventPayload) => void

  // ── Text editor events ─────────────────────────────────────────
  onChange?: (event: EventPayload) => void
  /** The DOM `submit` event of a descendant `<form>`, bubbling: see
   *  `FormProps.onSubmit`. Not available on `<input>` or `<textarea>`: handle
   *  Enter in `onKeyDown` and call `preventDefault()` to keep it from submitting
   *  the form (`<input>`) or inserting a newline (`<textarea>`). */
  onSubmit?: (event: EventPayload) => void
  /** A `required` control is empty when its form is submitted. Does not bubble. */
  onInvalid?: (event: EventPayload) => void

  // ── Native component events ─────────────────────────────────────
  onToggleFile?: (event: EventPayload) => void
  onShowMore?: (event: EventPayload) => void
  onLineClick?: (event: EventPayload) => void
  onLinkClick?: (event: EventPayload) => void
  onVisibleRange?: (event: EventPayload) => void
  /** Match count changed for this element's `highlight`. See `matchCount`. */
  onHighlight?: (event: EventPayload) => void
  /** A native `motion` track reached its current target. */
  onMotionComplete?: (event: EventPayload) => void

  // ── CSS transition events (DOM `TransitionEvent`) ────────────────
  // `propertyName` is the CSS name (`background-color`), `elapsedTime` is in
  // seconds and `pseudoElement` is always `""`. Fired on the element that
  // transitions; they do not bubble yet.
  /** A transition was created, before its delay. */
  onTransitionRun?: (event: EventPayload) => void
  /** A transition's delay ended and it started animating. */
  onTransitionStart?: (event: EventPayload) => void
  /** A transition reached its end value. */
  onTransitionEnd?: (event: EventPayload) => void
  /** A transition was interrupted: its property changed again, left the
   *  transition list, or became owned by `motion`. */
  onTransitionCancel?: (event: EventPayload) => void

  // ── CSS animation events (DOM `AnimationEvent`) ──────────────────
  // `animationName` is the name given to `defineKeyframes`, `elapsedTime` is in
  // seconds and `pseudoElement` is always `""`. Fired on the element that
  // animates; they do not bubble yet.
  /** An animation's delay ended and it started playing. */
  onAnimationStart?: (event: EventPayload) => void
  /** An animation began another iteration. Not fired for the last one. */
  onAnimationIteration?: (event: EventPayload) => void
  /** An animation finished its last iteration. */
  onAnimationEnd?: (event: EventPayload) => void
  /** An animation was interrupted before it ended: its name left
   *  `animation-name` or the name changed. */
  onAnimationCancel?: (event: EventPayload) => void

  // ── Layout animation ───────────────────────────────────────────
  /** A `layout` glide began: the box moved, or took over a `layoutId`. */
  onLayoutAnimationStart?: (event: EventPayload) => void
  /** A `layout` glide reached the box's layout position. Not fired for one a
   *  newer glide replaced. */
  onLayoutAnimationComplete?: (event: EventPayload) => void

  // ── Highlight ──────────────────────────────────────────────────
  /**
   * Paint a background wash behind matched or explicitly given text ranges.
   *
   * Scoped by position: on the root it searches the window, on a container it
   * searches that container. The nearest declaration wins, so a nested
   * `highlight` replaces an ancestor's for its own subtree.
   */
  highlight?: HighlightSpec | HighlightSpec[] | null

  // ── Focus props ────────────────────────────────────────────────
  /** Take keyboard focus when the element first mounts. Required for `<input>`:
   *  without it, or a click, the field never receives key events. */
  autoFocus?: boolean
  /**
   * Makes the element focusable, like the DOM. `-1` is focusable by click or
   * `focus()` but not by Tab. `0` joins the tab order in document order.
   * Positive values come first, in ascending order, then the 0s. An enabled
   * `<input>`/`<textarea>` is focusable without it. Key and focus listeners do
   * not make an element focusable.
   */
  tabIndex?: number
  /**
   * Makes the element a drag source, like the DOM attribute. A primary press on
   * it or inside it that moves more than 4px starts a drag: `onDragStart` fires
   * on the element, `onDrag` follows the pointer, `onDragEnter`/`onDragOver`/
   * `onDragLeave`/`onDrop` fire on what is under it, and `onDragEnd` on the
   * element when the drag ends. No pointer or mouse event fires meanwhile.
   * `preventDefault()` in `onDragStart` cancels the drag. There is no drag image.
   */
  draggable?: boolean
  /**
   * Like the DOM `inert`: the element and everything below it cannot be
   * focused (no tab stop, `focus()` is a no-op, a focused descendant is
   * blurred), is left out of the accessibility tree, cannot be selected, and
   * takes no pointer input: no click, hover or wheel listener fires and
   * clicks reach whatever lies behind it. A descendant cannot opt back in
   * with `pointerEvents: "auto"`, `userSelect: "text"` or `tabIndex`.
   * Inside an inert subtree `<input>` and `<textarea>` behave as `disabled`.
   */
  inert?: boolean
  /**
   * AccessKit role, as an ARIA token (`"button"`, `"heading"`).
   * A node is in the accessibility tree only with both an id (always set)
   * and a role. `"none"` / `"presentation"` produce no node.
   */
  role?: string
  /**
   * DOM `id`. Exposed as `AXIdentifier` / UIA AutomationId, and what
   * `htmlFor` and the `aria-*` id references (`aria-labelledby`,
   * `aria-describedby`, ...) resolve against. An element without a `role`
   * that one of those props names gets a group node, named by its text.
   */
  id?: string
  /** Stable locator id for automation. */
  testId?: string
  /** Internal native animation description used by motion components. */
  motion?: MotionProps
}

// Props shared by the native text editor elements.
export interface TextControlProps extends Omit<HostProps, "onSubmit"> {
  /** External editor value. Native edits apply immediately and report through onChange. */
  value?: string
  placeholder?: string
  readOnly?: boolean
  /**
   * Like the DOM: not focusable, not a tab stop, no caret, no editing, no
   * mouse, click or key events, the text in the theme's muted colour, and
   * reported as disabled to assistive technology. Focus leaves the control
   * when it becomes disabled.
   */
  disabled?: boolean
  /**
   * The most UTF-16 code units the user can type or paste. Longer input is
   * cut to fit. A `value` set from code is kept whole, even when it is longer.
   */
  maxLength?: number
  /** The key this control's value is submitted under. A control without a
   *  name is not part of its form's `FormData`. */
  name?: string
  /** An empty control blocks its form's submission and gets `onInvalid`.
   *  Not enforced while `disabled` or `readOnly`. */
  required?: boolean
  theme?: GpuixTheme
}

/** Values of `<input type>`. */
export type InputType = "text" | "password" | "number" | "email" | "search" | "tel" | "url"

export interface InputProps extends TextControlProps {
  /**
   * Defaults to `"text"`; an unknown value is `"text"` too.
   *
   * - `"password"` paints one bullet per character and disables copy and cut.
   * - `"number"` makes ArrowUp and ArrowDown step the value by `step`, within
   *   `min` and `max`, and report it through `onChange`. Typing is not
   *   restricted: `onChange` reports exactly the text, even when it is not a
   *   number. There is no spinner.
   * - `"search"` is exposed to assistive technology as a search field.
   * - `"email"`, `"tel"` and `"url"` behave as `"text"`.
   */
  type?: InputType
  /** `type="number"` only. A number or numeric string. */
  min?: number | string
  /** `type="number"` only. A number or numeric string. */
  max?: number | string
  /** `type="number"` only: the ArrowUp and ArrowDown step. Defaults to 1. */
  step?: number | string
}

export interface TextareaProps extends TextControlProps {
  minRows?: number
  maxRows?: number
}

/** The `submit` event of a `<form>`. */
export type SubmitEvent = EventPayload & {
  /** The `name` and `value` of every named, enabled `<input>` and `<textarea>`
   *  in the form, in document order. */
  formData: FormData
  /** The button that submitted the form. Always null: there is no submit button. */
  submitter: null
}

/** `<form>`: groups `<input>` and `<textarea>` controls and submits them together. */
export interface FormProps extends Omit<HostProps, "onSubmit"> {
  /**
   * Enter in an `<input>` of this form, or `requestSubmit()`, once every
   * `required` control has a value. Cancelable and bubbling, like the DOM; there
   * is no navigation, so there is nothing to cancel. Enter in a `<textarea>`
   * inserts a newline and never submits.
   */
  onSubmit?: (event: SubmitEvent) => void
}

/** What a ref to a `<form>` carries. */
export interface FormMethods {
  /**
   * Validate the form's controls, then submit it. Each empty `required` control
   * gets `invalid`, and the first whose handlers did not call `preventDefault()`
   * takes focus; the form is not submitted. Otherwise `submit` fires. The events
   * are delivered before this returns.
   */
  requestSubmit(): void
}

/** `<label>`: names a control and forwards a click to it. */
export interface LabelProps extends HostProps {
  /** The `id` of the control to label. Without it, the first `<input>` or
   *  `<textarea>` inside the label is the control. */
  htmlFor?: string
}

export type VirtualListShared = AriaProps & {
  /** No `hover` or `active`: gpui's `List` has no pressed or hovered style
   *  state. Put those on a wrapping `<div>` instead. */
  style?: Omit<StyleDesc, "hover" | "active">
  alignment?: "top" | "bottom"
  followTail?: boolean
  overdraw?: number
  onVisibleRange?: (event: EventPayload) => void
  role?: string
  id?: string
  testId?: string
}

/** A variable-height list that builds only rows near its viewport. */
export type VirtualListProps =
  | (VirtualListShared & {
      estimatedItemHeight?: number
      itemCount?: never
      windowStart?: never
    })
  | (VirtualListShared & {
      itemCount: number
      estimatedItemHeight: number
      windowStart?: number
    })

// Props for native <img> rendering.
export interface ImgProps extends HostProps {
  /** Filesystem path, `file://` URL, data URL, `blob:` URL (from
   *  `URL.createObjectURL` in this process) or http(s) URL. */
  src?: string
  /** CSS `object-fit`. Default `"contain"`. */
  objectFit?: "fill" | "contain" | "cover" | "scaleDown" | "none"
  alt?: string
}

/** Byte order of a `setImagePixels` buffer. Alpha is straight in both. */
export type ImagePixelFormat = "rgba" | "bgra"

export interface ImagePixelsOptions {
  /** Default `"rgba"`. `"bgra"` is GPUI's native order and skips a per-pixel pass. */
  format?: ImagePixelFormat
}

/** The tags that are valid only inside an `<svg>`. */
export type SvgChildType =
  | "path"
  | "circle"
  | "ellipse"
  | "rect"
  | "line"
  | "polyline"
  | "polygon"
  | "g"

/**
 * SVG presentation attributes, camelCase as in React DOM: `strokeWidth`
 * becomes `stroke-width`. Lengths are user units; a string passes through.
 */
export interface SvgPresentationProps {
  fill?: string
  fillOpacity?: number | string
  fillRule?: "nonzero" | "evenodd"
  clipRule?: "nonzero" | "evenodd"
  stroke?: string
  strokeWidth?: number | string
  strokeOpacity?: number | string
  strokeLinecap?: "butt" | "round" | "square"
  strokeLinejoin?: "miter" | "round" | "bevel"
  strokeMiterlimit?: number | string
  /** A number, a list, or `"4 2"`. Measured in `pathLength` units when set. */
  strokeDasharray?: number | string | number[]
  strokeDashoffset?: number | string
  opacity?: number | string
  /** An SVG transform list, passed through: `"rotate(45 12 12)"`. */
  transform?: string
  display?: string
  visibility?: string
  shapeRendering?: "auto" | "optimizeSpeed" | "crispEdges" | "geometricPrecision"
}

/**
 * Props shared by the shapes inside an `<svg>`. A shape is part of its svg's
 * one image, so it has no `style`, layout box or events: size and place the
 * `<svg>`, and draw inside its `viewBox`.
 */
export interface SvgShapeProps extends SvgPresentationProps {
  testId?: string
  /** Internal native animation description used by `motion.*` shapes. */
  motion?: SvgMotionProps
  /**
   * The DOM attribute: the length the shape's dash values are measured in, so
   * `pathLength={1}` with `strokeDasharray="0.5 1"` strokes half the outline.
   * (Animate Framer's normalised `pathLength` through `motion`.)
   */
  pathLength?: number
}

export interface SvgPathProps extends SvgShapeProps {
  /** Path data. Relative commands are fine. */
  d?: string
}

export interface SvgCircleProps extends SvgShapeProps {
  cx?: number | string
  cy?: number | string
  r?: number | string
}

export interface SvgEllipseProps extends SvgShapeProps {
  cx?: number | string
  cy?: number | string
  rx?: number | string
  ry?: number | string
}

export interface SvgRectProps extends SvgShapeProps {
  x?: number | string
  y?: number | string
  width?: number | string
  height?: number | string
  rx?: number | string
  ry?: number | string
}

export interface SvgLineProps extends SvgShapeProps {
  x1?: number | string
  y1?: number | string
  x2?: number | string
  y2?: number | string
}

export interface SvgPolyProps extends SvgShapeProps {
  /** `"x,y x,y ..."` */
  points?: string
}

export type SvgGroupProps = SvgShapeProps

/**
 * Props for `<svg>`, a vector graphic rasterised in full colour at device
 * resolution as one image. `currentColor` is the element's computed `color`.
 *
 * Give it DOM-style children (`path circle ellipse rect line polyline polygon
 * g`) and a `viewBox`, or raw markup through `source` / `src`. Children win
 * when both are given. `width` and `height` size the element unless `style`
 * does; without either it has no size to paint into.
 */
export interface SvgProps extends HostProps, SvgPresentationProps {
  /** Desktop local path. Use source for portable browser rendering. */
  src?: string
  /** Raw SVG markup, rasterised in full colour at device resolution. */
  source?: string
  /** `"minX minY width height"`: the coordinate system the children draw in. */
  viewBox?: string
  preserveAspectRatio?: string
  width?: number | string
  height?: number | string
}

/**
 * Props for the <code> custom element — a syntax-highlighted code block.
 *
 * It paints **no surface of its own**: no fill, border, radius, padding or
 * language header. `style` is the surface, and `fontFamily`, `fontSize`,
 * `fontWeight`, `lineHeight` and `color` there beat the theme. Wrap it, or
 * style it, to get a card.
 *
 * Rows are a fixed height, so `fontSize` alone scales that height by the
 * theme's ratio. Lines never wrap and the block is its own horizontal
 * scroller, so `whiteSpace` and `overflowX` do nothing.
 */
export interface CodeProps extends HostProps {
  /** The source to display. Rendered one div per line at an exact line height. */
  code?: string
  /** Language alias such as "ts", "rust", "bash". Beats `path` for detection. */
  language?: string
  /** File path, used for extension-based language detection. */
  path?: string
  showLineNumbers?: boolean
  theme?: GpuixTheme
}

// Props for the <diff> custom element — a unified diff viewer.
export interface DiffProps extends HostProps {
  /** A unified git patch (the output of `git diff`). */
  patch?: string
  /** Highlight the words that changed inside paired +/- lines. */
  wordDiff?: boolean
  /** File paths rendered as a header only. Collapsed bodies cost one row. */
  collapsedPaths?: string[]
  /**
    * Use the virtualized `list()` scroller. Off by default so a parent
    * list can be the only scroll container. Requires a bounded height.
   */
  scroll?: boolean
  /** Paint this many line rows, then a Show more row. */
  maxLines?: number
  theme?: GpuixTheme
  /** Fires when a file header is clicked. `event.value` is the file path. */
  onToggleFile?: (event: EventPayload) => void
  /** Fires when Show more is clicked. `event.value` is the hidden line count. */
  onShowMore?: (event: EventPayload) => void
  /** Fires when a diff line is clicked. `event.value` is the line text,
   *  `event.oldLine` / `event.newLine` are its line numbers. */
  onLineClick?: (event: EventPayload) => void
}

// Props for the <markdown> custom element.
export interface MarkdownProps extends HostProps {
  /** GitHub-flavoured markdown. Tables, strikethrough and task lists are on. */
  source?: string
  theme?: GpuixTheme
  /** Fires when a block containing links is clicked. `event.value` is the URL. */
  onLinkClick?: (event: EventPayload) => void
}

/** Payload of `<anchored onPlacement>`. */
export interface AnchoredPlacementEvent extends EventPayload {
  side: "top" | "right" | "bottom" | "left"
  align: "start" | "center" | "end"
  /** Width the layer can take without leaving the window, minus `snapMargin`. */
  availableWidth: number
  /** Height the layer can take on its side of the trigger, minus `snapMargin` and `gap`. */
  availableHeight: number
  /** Border-box size of the trigger. 0 when placed at a `position` point. */
  anchorWidth: number
  anchorHeight: number
}

// Props for the <anchored> custom element.
export interface AnchoredProps extends HostProps {
  position?: { x: number; y: number }
  side?: "top" | "right" | "bottom" | "left"
  align?: "start" | "center" | "end"
  gap?: number
  anchor?:
    | "topLeft"
    | "topCenter"
    | "topRight"
    | "rightCenter"
    | "bottomRight"
    | "bottomCenter"
    | "bottomLeft"
    | "leftCenter"
  offset?: { x: number; y: number }
  /** `"flip"` (default, `"switch"` is the same) opens on the opposite side of the trigger,
   *  with the same `gap`, when the preferred side runs out of room, or on the side with more
   *  room when neither fits, then shifts along the other axis to stay inside the window.
   *  `"snap"` only shifts. Applies when `anchor` is not set. */
  fit?: "flip" | "switch" | "snap"
  /** Distance kept to the window edges, also taken off `available*`. */
  snapMargin?: number
  /** Cap the layer's `maxWidth`/`maxHeight` to the room left in the window, natively on every
   *  layout. Like floating-ui's `size` middleware. */
  constrainToAvailable?: boolean
  /** Fires when side, alignment or the available room change (whole pixels). */
  onPlacement?: (event: AnchoredPlacementEvent) => void
  deferred?: boolean
  priority?: number
  /** `"window"` covers the whole viewport and tracks resizes natively. Ignores
   *  position, side, align, anchor, offset and fit. */
  fill?: "window"
}

export type MutationHost = Pick<GpuixRenderer, "applyBatch">
export type WindowSizeHost = Pick<GpuixRenderer, "getWindowSize">
export type WindowInsetsHost = Pick<GpuixRenderer, "getWindowInsets">
export type SelectionHost = Pick<GpuixRenderer, "getSelectedText">
export type AppearanceHost = Pick<GpuixRenderer, "getAppearance">
/** The effective window appearance. `setAppearance` also takes `"system"`. */
export type Appearance = "light" | "dark"
/** The native motion value graph. Ids are chosen by the caller. */
export type MotionValueHost = Pick<
  GpuixRenderer,
  | "createMotionValue"
  | "createMotionTransform"
  | "createMotionFollower"
  | "createMotionSpring"
  | "disposeMotionValue"
  | "getMotionValue"
  | "getMotionValueVelocity"
  | "isMotionValueAnimating"
  | "setMotionValue"
  | "jumpMotionValue"
  | "animateMotionValue"
  | "stopMotionValue"
  | "setMotionValueListener"
>
/** `applyBatch` plus the live renderer methods a host may also expose. */
export type NativeRenderer = MutationHost &
  Partial<MotionValueHost> &
  Partial<
    Pick<
      GpuixRenderer,
      | "focusElement"
      | "blurElement"
      | "matchesFocus"
      | "focusNext"
      | "focusPrevious"
      | "getFocusedElementId"
      | "focusNextWithin"
      | "focusPreviousWithin"
      | "blur"
      | "setWindowKeyEvents"
      | "setSyncEventHandler"
      | "setWindowSelectionChange"
      | "setWindowAppearanceChange"
      | "setWindowMediaChange"
      | "getMediaFeatures"
      | "getElementBounds"
      | "getElementsAt"
      | "scrollTo"
      | "scrollToItem"
      | "scrollIntoView"
      | "setPointerCapture"
      | "releasePointerCapture"
      | "hasPointerCapture"
      | "clickElement"
      | "cancelDrag"
      | "requestPointerLock"
      | "exitPointerLock"
      | "pointerLockElement"
      | "getScrollMetrics"
      | "scrollByKey"
      | "setImage"
      | "setImagePixels"
      | "selectText"
      | "setSelectionRange"
      | "getTextSelection"
      | "requestSubmit"
      | "getScrollOffset"
      | "getListScrollTop"
      | "getSelectedText"
      | "clearSelection"
      | "getPaintedHighlights"
      | "getWindowSize"
      | "getWindowInsets"
      | "setWindowTitle"
      | "registerFont"
      | "setAppearance"
      | "getAppearance"
      | "promptForPaths"
      | "writeClipboardText"
      | "readClipboardText"
      | "supportsEyeDropper"
      | "openEyeDropper"
      | "activateWindow"
      | "minimizeWindow"
      | "zoomWindow"
      | "toggleFullscreen"
      | "setDebugFrameOverlay"
      | "getDebugFrameOverlay"
      | "cycleDebugFrameOverlay"
      | "resetDebugFrameOverlayStats"
      | "getDebugFrameOverlayStats"
    >
  >

/** Commit-phase mutation facade shared by framework adapters. */
export interface MutationRenderer {
  createElement(id: number, elementType: string): void
  destroyElement(id: number): Array<number>
  appendChild(parentId: number, childId: number): void
  insertBefore(parentId: number, childId: number, beforeId: number): void
  setStyle(id: number, style: object): void
  setText(id: number, content: string): void
  setEventListener(id: number, eventType: string, hasHandler: boolean): void
  setRoot(id: number): void
  setKeyboardFocusDim(enabled: boolean): void
  setCustomProp(id: number, key: string, value: object | string | number | boolean | null): void
  flushMutations(): void
}

export type DebugFrameOverlayMode = "hidden" | "minimal" | "full"

/** Method syntax makes the parameter bivariant, so an `onKeyDown` typed with
 *  `KeyEvent` can share the map. Dispatch builds the `KeyEvent` for key types. */
export type HostEventHandler = { handle(event: EventPayload): void }["handle"]

export type EventHandlerMap = Map<number, Map<string, HostEventHandler>>

/**
 * A `keyDown` / `keyUp` event, the same object every handler of one keystroke
 * sees, like one DOM event travelling from the focused element to `window`.
 *
 * `key`, `code`, `repeat`, `isComposing` and the `*Key` booleans follow DOM
 * `KeyboardEvent`: `key` is `"Enter"`, `"ArrowDown"`, `" "`, `"F1"`, or the
 * character produced (`"A"` for shift-a); `code` is the physical key (`"KeyA"`,
 * `"Digit1"`), assuming a US QWERTY layout for printable keys. The mapping from
 * GPUI key names lives in `events/key_names.rs`.
 */
export interface KeyEvent extends EventPayload {
  /** True after any handler of this keystroke called `preventDefault()`. */
  readonly defaultPrevented: boolean
  /**
   * Cancel the default action. On `keyDown` that is native: no editor action
   * (caret movement, deletion, Enter, Cmd+C), no text typed, and in JS, Tab
   * moving focus and Escape closing the top overlay. Handlers run before native
   * acts, except on Windows and Linux, where the key is delivered after the
   * fact and only the JS defaults can be cancelled.
   */
  preventDefault(): void
  /** Skip the handlers further along the path, up to and including `window`.
   *  The default action still runs, like the DOM. */
  stopPropagation(): void
}

export type WindowKeyEventHandler = (
  event: KeyEvent,
  renderer: NativeRenderer
) => void

export type WindowEventHandler = (
  event: EventPayload,
  renderer: NativeRenderer
) => void

export interface WindowKeyEventHandlers {
  /** A `window` bubble listener for `keydown`, for this root only. Runs after
   *  every element handler on the focused element's path. */
  onKeyDown?: WindowKeyEventHandler
  /** A `window` bubble listener for `keyup`, for this root only. */
  onKeyUp?: WindowKeyEventHandler
  /** Window-level text selection. Fires when the selected ranges change. */
  onSelectionChange?: WindowEventHandler
  /** Window-level appearance. Fires when the effective appearance flips
   *  between light and dark. `event.value` is `"light"` or `"dark"`. */
  onAppearanceChange?: WindowEventHandler
  /**
   * Window-level, like the DOM's document `pointerlockchange`. Fires when the
   * pointer lock element changes: after a granted `requestPointerLock()`, after
   * `exitPointerLock()`, when the platform drops the lock (the window lost
   * focus) and when the locked element leaves the tree. `event.target` is the
   * locked element's id, absent once the lock is gone. Read the same thing
   * from `renderer.pointerLockElement()`.
   */
  onPointerLockChange?: WindowEventHandler
  /**
   * Window-level, like the DOM's document `pointerlockerror`. Fires when a
   * `requestPointerLock()` fails: the element is not in the tree, or the
   * platform refused (the window is not active, or the platform cannot lock the
   * pointer). `event.target` is the element that was asked.
   */
  onPointerLockError?: WindowEventHandler
  /**
   * Tab and Shift+Tab move focus through `tabIndex` elements. Defaults to
   * true. Call `event.preventDefault()` in any `onKeyDown` to keep one Tab.
   */
  tabNavigation?: boolean
  /**
   * While a control has keyboard focus, every other focusable element dims.
   * Defaults to true. Set false to draw your own focus look with
   * `focusVisible`. Per element, use `style.keyboardFocusDim: false`.
   */
  keyboardFocusDim?: boolean
}

export interface RootEventHandlers extends WindowKeyEventHandlers {
  onEvent?: (event: EventPayload) => void
  onUncaughtError?: (
    error: Error,
    errorInfo: { componentStack?: string }
  ) => void
}

export interface ElementIdAllocator {
  nextElementId: number
}
