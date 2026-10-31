// GPUIX component definitions and native motion wrappers.

import { createElement, forwardRef, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react"
import type { ReactElement, ReactNode } from "react"
import {
  elementScope,
  gestureDescription,
  gestureHandlers,
  layoutDescription,
  motionComponentStyle,
  observeViewport,
  reduceMotionValues,
  resolveExit,
  resolveMotion,
  scopeLayoutId,
  SPAN_SCOPE,
  withoutIgnoredSpanProps,
  withoutIgnoredSpanStyle,
  withoutLayoutTransition,
  wireMotionTransition,
} from "@gpuix/native/host"
import type { StyleDesc } from "@gpuix/native/host"
import type {
  ExitTarget,
  LayoutDrag,
  MotionAnimation,
  MotionGestureCallbacks,
  MotionGestureHandlers,
  MotionInput,
  MotionLayoutProps,
  MotionSpanAnimation,
  MotionSpanIgnoredProps,
  MotionStyle,
  MotionTransition,
  MotionValueStyle,
  MotionVariantContext,
  Props,
  PublicInstance,
  SvgCircleProps,
  SvgEllipseProps,
  SvgGroupProps,
  SvgLineProps,
  SvgMotionStyle,
  SvgMotionTransition,
  SvgPathProps,
  SvgPolyProps,
  SvgRectProps,
  SvgFeDropShadowProps,
  SvgFeFloodProps,
  SvgFeGaussianBlurProps,
  SvgFeOffsetProps,
  SvgImageProps,
  SvgTextProps,
  SvgTspanProps,
} from "../types/host.js"
import { flushSync } from "../reconciler/reconciler.js"
import { PresenceContext, usePresence } from "./animate-presence.js"
import { LayoutGroupContext } from "./layout-group.js"
import { MotionConfigContext } from "./motion-config.js"
import { VariantContext } from "./variant-context.js"

let nextMotionGeneration = 0

export {
  AnimatePresence,
  PresenceContext,
  useIsPresent,
  usePresence,
} from "./animate-presence.js"
export type { AnimatePresenceProps } from "./animate-presence.js"
export { LayoutGroup } from "./layout-group.js"
export type { LayoutGroupProps } from "./layout-group.js"
export { MotionConfig, useReducedMotionConfig } from "./motion-config.js"
export type { MotionConfigProps } from "./motion-config.js"
export { UniversalStyle } from "./universal-style.js"
export type { UniversalStyleProps } from "./universal-style.js"

export const gpuixComponents = {
  div: "div",
  text: "text",
  span: "span",
  img: "img",
  svg: "svg",
  canvas: "canvas",
  input: "input",
  textarea: "textarea",
  label: "label",
  form: "form",
  anchored: "anchored",
  webview: "webview",
  nativeview: "nativeview",
  "virtual-list": "virtual-list",
} as const

export type GpuixComponentType = keyof typeof gpuixComponents

export interface MotionDivProps extends MotionAnimation<MotionStyle>, MotionGestureCallbacks, MotionLayoutProps {
  children?: ReactNode
  style?: MotionValueStyle
  onClick?: Props["onClick"]
  onMouseDown?: Props["onMouseDown"]
  onMouseUp?: Props["onMouseUp"]
  onMouseEnter?: Props["onMouseEnter"]
  onMouseLeave?: Props["onMouseLeave"]
  onMouseMove?: Props["onMouseMove"]
  onPointerDown?: Props["onPointerDown"]
  onPointerMove?: Props["onPointerMove"]
  onPointerUp?: Props["onPointerUp"]
  onPointerCancel?: Props["onPointerCancel"]
  onLostPointerCapture?: Props["onLostPointerCapture"]
  onMouseDownOutside?: Props["onMouseDownOutside"]
  onKeyDown?: Props["onKeyDown"]
  onKeyUp?: Props["onKeyUp"]
  onFocus?: Props["onFocus"]
  onBlur?: Props["onBlur"]
  onWheel?: Props["onWheel"]
  onScroll?: Props["onScroll"]
  onDrop?: Props["onDrop"]
  onMotionComplete?: Props["onMotionComplete"]
  onLayoutAnimationStart?: Props["onLayoutAnimationStart"]
  onLayoutAnimationComplete?: Props["onLayoutAnimationComplete"]
  autoFocus?: boolean
}

/**
 * Props of a `motion.span`: an inline span animates what an inline box shows (`animate`, `initial`,
 * `exit`, `whileHover`, `whileTap`, `whileFocus`, `whileInView`, or the `variants` they name) and takes
 * the props of a `<span>`. A non-atomic inline box takes no transform, so `layout`, `layoutId`, `drag*`
 * and the transform keys are accepted and have no effect, as in a browser.
 */
export interface MotionSpanProps
  extends Omit<Props, "style" | keyof MotionSpanAnimation | keyof MotionGestureCallbacks | keyof MotionSpanIgnoredProps>,
    MotionSpanAnimation,
    MotionGestureCallbacks,
    MotionSpanIgnoredProps {
  style?: MotionValueStyle
}

/** What every `motion.*` component takes, whatever it animates. */
interface MotionSource<Style, Transition> extends MotionAnimation<Style, Transition>, MotionGestureCallbacks {
  layout?: MotionLayoutProps["layout"]
  layoutId?: string
  /** The drag values of a `Reorder.Item`; not part of the API. */
  layoutDrag?: LayoutDrag
  onMotionComplete?: Props["onMotionComplete"]
}

export function createMotionComponent<Source extends MotionSource<object, MotionTransition | SvgMotionTransition>>(tag: string) {
  const inline = tag === "span"
  const scope = inline ? SPAN_SCOPE : elementScope(`motion.${tag}`)
  return forwardRef<PublicInstance, Source>(function Motion(
    {
      initial,
      animate,
      exit: exitTarget,
      custom,
      transition,
      layout,
      layoutId,
      layoutDrag,
      variants,
      inherit,
      whileHover,
      whileTap,
      whileFocus,
      whileInView,
      viewport,
      onHoverStart,
      onHoverEnd,
      onTapStart,
      onTap,
      onTapCancel,
      onViewportEnter,
      onViewportLeave,
      onMotionComplete,
      ...rest
    },
    ref
  ): ReactElement {
    // A span takes no transform: what Framer would move with one is accepted and left where it is.
    const props = inline
      ? withoutIgnoredSpanProps({ ...rest, style: withoutIgnoredSpanStyle(Reflect.get(rest, "style")) })
      : { ...rest, style: motionComponentStyle(Reflect.get(rest, "style")) }
    const [inView, setInView] = useState(false)
    const observed = useRef<PublicInstance | null>(null)
    const setRef = useCallback(
      (instance: PublicInstance | null) => {
        observed.current = instance
        if (typeof ref === "function") ref(instance)
        else if (ref) ref.current = instance
      },
      [ref]
    )
    const callbacks = useRef<MotionGestureCallbacks>({})
    callbacks.current = { onHoverStart, onHoverEnd, onTapStart, onTap, onTapCancel, onViewportEnter, onViewportLeave }
    const presence = useContext(PresenceContext)
    const config = useContext(MotionConfigContext)
    const layoutGroupId = useContext(LayoutGroupContext)
    const parentVariants = useContext(VariantContext)
    const inheritedVariants = inherit === false ? undefined : parentVariants
    const reduced = config !== undefined && config.reduced
    const [isPresent, safeToRemove] = usePresence()
    // An `exit` function has nothing to leave from outside an `AnimatePresence`.
    const exitResolved =
      typeof exitTarget === "function" && presence === null
        ? undefined
        : resolveExit(exitTarget as ExitTarget<object> | undefined, presence?.custom ?? custom)
    const resolved = resolveMotion(scope, {
      initial: presence?.initial === false ? false : (initial as MotionInput["initial"]),
      animate: animate as MotionInput["animate"],
      exit: exitResolved as MotionInput["exit"],
      variants: variants as MotionInput["variants"],
      custom: presence?.custom ?? custom,
      gestures: { whileHover, whileTap, whileFocus, whileInView },
      parent: inheritedVariants,
      inView,
    })
    const exit = resolved.exit
    const resolvedInitial = resolved.initial
    const resolvedAnimate = !isPresent && exit ? exit : resolved.animate
    const variantTransition = !isPresent && exit ? resolved.exitTransition : resolved.animateTransition
    const componentTransition = wireMotionTransition(transition ?? config?.transition)
    const motionTransition = withoutLayoutTransition(
      variantTransition === undefined ? componentTransition : wireMotionTransition(variantTransition)
    )

    useEffect(() => {
      if (!isPresent && !exit) safeToRemove?.()
    }, [exit, isPresent, safeToRemove])

    // Gestures are over once the element is on its way out.
    const gestures = isPresent
      ? gestureDescription(
          resolved,
          (own) => wireMotionTransition(own),
          resolved.inherited.includes("whileInView") ? inheritedVariants!.inView : inView
        )
      : undefined
    const watchesView =
      isPresent && (whileInView !== undefined || onViewportEnter !== undefined || onViewportLeave !== undefined)
    useEffect(() => {
      const target = observed.current
      if (!watchesView || !target) return
      // Native delivers it from a frame, not from an event React is handling.
      return observeViewport(target, viewport, (visible, entry) => {
        flushSync(() => setInView(visible))
        if (visible) callbacks.current.onViewportEnter?.(entry)
        else callbacks.current.onViewportLeave?.(entry)
      })
    }, [watchesView, viewport?.root, viewport?.margin, viewport?.amount, viewport?.once])

    const motionKey = JSON.stringify([
      isPresent,
      resolvedInitial,
      resolvedAnimate,
      motionTransition,
      reduced,
      gestures,
    ])
    const generation = useMemo(() => ++nextMotionGeneration, [motionKey])

    const style: unknown = Reflect.get(props, "style")
    useLayoutEffect(() => (reduced ? reduceMotionValues(style) : undefined), [reduced, style])

    const hostProps: Record<string, unknown> = {
      ...props,
      ...gestureHandlers(() => callbacks.current, props as MotionGestureHandlers),
      ref: watchesView ? setRef : ref,
    }
    if (resolvedAnimate !== undefined || gestures !== undefined) {
      hostProps.motion = {
        generation,
        isExit: !isPresent && exit !== undefined,
        initial: resolvedInitial,
        animate: resolvedAnimate ?? {},
        transition: motionTransition,
        ...(reduced ? { reducedMotion: true } : {}),
        ...gestures,
      }
    }
    // A layout animation moves a box with a transform, which a span has none of.
    const layoutProp = inline
      ? undefined
      : layoutDescription({
          layout,
          layoutId: scopeLayoutId(layoutGroupId, layoutId),
          transition: componentTransition as MotionTransition | undefined,
          reduced,
          drag: layoutDrag,
        })
    if (layoutProp !== undefined) hostProps.layout = layoutProp
    if (!isPresent || onMotionComplete) {
      hostProps.onMotionComplete = (event: Parameters<NonNullable<Props["onMotionComplete"]>>[0]) => {
        if (event.motionGeneration !== generation) return
        onMotionComplete?.(event)
        if (!isPresent) safeToRemove?.()
      }
    }
    // Always a provider, so that a component that starts or stops controlling variants keeps its element.
    const handedDown = useStableVariants(resolved.controls ?? inheritedVariants)
    return createElement(VariantContext.Provider, { value: handedDown }, createElement(tag, hostProps))
  })
}

/** The same object for the same names, so that a render of the controller does not render what is under it. */
function useStableVariants(value: MotionVariantContext | undefined): MotionVariantContext | undefined {
  const key = JSON.stringify(value)
  return useMemo(() => value, [key])
}

const MotionDiv = createMotionComponent<MotionDivProps>("div")
const MotionSpan = createMotionComponent<MotionSpanProps>("span")

/** `motion.div` plus the drag values `Reorder.Item` hands it, for the element type it is `as`. */
export type MotionDragProps = MotionDivProps & { layoutDrag?: LayoutDrag }

/** Props of a `motion.path`, `motion.circle`, ...: the shape's own plus Framer's animation props. */
export type MotionShapeProps<Shape> = Omit<Shape, "motion" | "ref" | "key"> &
  MotionAnimation<SvgMotionStyle, SvgMotionTransition> &
  Pick<MotionGestureCallbacks, "onViewportEnter" | "onViewportLeave"> & {
    onMotionComplete?: Props["onMotionComplete"]
  }

function motionShape<Shape>(tag: string) {
  return createMotionComponent<MotionShapeProps<Shape>>(tag)
}

/**
 * Native animations with a Motion-like declarative React API. `motion.path`,
 * `motion.circle` and the other SVG shapes animate attributes (`pathLength`,
 * `d`, `stroke`, ...) instead of style, and are valid only inside `<svg>`.
 */
export const motion = {
  div: MotionDiv,
  span: MotionSpan,
  g: motionShape<SvgGroupProps>("g"),
  path: motionShape<SvgPathProps>("path"),
  circle: motionShape<SvgCircleProps>("circle"),
  ellipse: motionShape<SvgEllipseProps>("ellipse"),
  rect: motionShape<SvgRectProps>("rect"),
  line: motionShape<SvgLineProps>("line"),
  polyline: motionShape<SvgPolyProps>("polyline"),
  polygon: motionShape<SvgPolyProps>("polygon"),
  text: motionShape<SvgTextProps>("text"),
  tspan: motionShape<SvgTspanProps>("tspan"),
  image: motionShape<SvgImageProps>("image"),
  feGaussianBlur: motionShape<SvgFeGaussianBlurProps>("feGaussianBlur"),
  feOffset: motionShape<SvgFeOffsetProps>("feOffset"),
  feFlood: motionShape<SvgFeFloodProps>("feFlood"),
  feDropShadow: motionShape<SvgFeDropShadowProps>("feDropShadow"),
} as const

// There is no `VirtualList` React wrapper. Windowing on the React side is the
// app's job: pass `itemCount`, `estimatedItemHeight` and `windowStart` to the
// host `<virtual-list>` and render only that slice. A generic wrapper cannot
// know when to widen its own window, so it silently dropped rows whenever
// `itemCount` grew without a scroll.
