// GPUIX component definitions and native motion wrappers.

import { createElement, forwardRef, useContext, useEffect, useLayoutEffect, useMemo } from "react"
import type { ReactElement, ReactNode } from "react"
import {
  layoutDescription,
  reduceMotionValues,
  resolveExit,
  scopeLayoutId,
  withoutLayoutTransition,
  wireMotionTransition,
} from "@gpuix/native/host"
import type {
  ExitTarget,
  LayoutDrag,
  MotionLayoutProps,
  MotionProps,
  MotionTransition,
  MotionValueStyle,
  Props,
  PublicInstance,
  SvgCircleProps,
  SvgEllipseProps,
  SvgGroupProps,
  SvgLineProps,
  SvgMotionProps,
  SvgMotionTransition,
  SvgPathProps,
  SvgPolyProps,
  SvgRectProps,
  WithExitFunction,
} from "../types/host.js"
import { PresenceContext, usePresence } from "./animate-presence.js"
import { LayoutGroupContext } from "./layout-group.js"
import { MotionConfigContext } from "./motion-config.js"

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
  "virtual-list": "virtual-list",
} as const

export type GpuixComponentType = keyof typeof gpuixComponents

export interface MotionDivProps extends WithExitFunction<MotionProps>, MotionLayoutProps {
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

/** What every `motion.*` component takes, whatever it animates. */
interface MotionSource<Style, Transition> {
  initial?: Style | false
  animate?: Style
  exit?: ExitTarget<NonNullable<Style>>
  custom?: unknown
  transition?: Transition
  layout?: MotionLayoutProps["layout"]
  layoutId?: string
  /** The drag values of a `Reorder.Item`; not part of the API. */
  layoutDrag?: LayoutDrag
  onMotionComplete?: Props["onMotionComplete"]
}

export function createMotionComponent<Source extends MotionSource<object, MotionTransition | SvgMotionTransition>>(tag: string) {
  return forwardRef<PublicInstance, Source>(function Motion(
    { initial, animate, exit: exitTarget, custom, transition, layout, layoutId, layoutDrag, onMotionComplete, ...props },
    ref
  ): ReactElement {
    const presence = useContext(PresenceContext)
    const config = useContext(MotionConfigContext)
    const layoutGroupId = useContext(LayoutGroupContext)
    const reduced = config !== undefined && config.reduced
    const resolvedTransition = wireMotionTransition(transition ?? config?.transition)
    const motionTransition = withoutLayoutTransition(resolvedTransition)
    const [isPresent, safeToRemove] = usePresence()
    // An `exit` function has nothing to leave from outside an `AnimatePresence`.
    const exit =
      typeof exitTarget === "function" && presence === null
        ? undefined
        : resolveExit(exitTarget, presence?.custom ?? custom)
    const resolvedInitial = presence?.initial === false ? false : initial
    const resolvedAnimate = !isPresent && exit ? exit : animate

    useEffect(() => {
      if (!isPresent && !exit) safeToRemove?.()
    }, [exit, isPresent, safeToRemove])

    const motionKey = JSON.stringify([
      isPresent,
      resolvedInitial,
      resolvedAnimate,
      motionTransition,
      reduced,
    ])
    const generation = useMemo(() => ++nextMotionGeneration, [motionKey])

    const style: unknown = Reflect.get(props, "style")
    useLayoutEffect(() => (reduced ? reduceMotionValues(style) : undefined), [reduced, style])

    const hostProps: Record<string, unknown> = { ...props, ref }
    if (resolvedAnimate !== undefined) {
      hostProps.motion = {
        generation,
        isExit: !isPresent && exit !== undefined,
        initial: resolvedInitial,
        animate: resolvedAnimate,
        transition: motionTransition,
        ...(reduced ? { reducedMotion: true } : {}),
      }
    }
    const layoutProp = layoutDescription({
      layout,
      layoutId: scopeLayoutId(layoutGroupId, layoutId),
      transition: resolvedTransition as MotionTransition | undefined,
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
    return createElement(tag, hostProps)
  })
}

const MotionDiv = createMotionComponent<MotionDivProps>("div")

/** `motion.div` plus the drag values `Reorder.Item` hands it, for the element type it is `as`. */
export type MotionDragProps = MotionDivProps & { layoutDrag?: LayoutDrag }

/** Props of a `motion.path`, `motion.circle`, ...: the shape's own plus Framer's animation props. */
export type MotionShapeProps<Shape> = Omit<Shape, "motion" | "ref" | "key"> &
  WithExitFunction<SvgMotionProps> & {
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
  g: motionShape<SvgGroupProps>("g"),
  path: motionShape<SvgPathProps>("path"),
  circle: motionShape<SvgCircleProps>("circle"),
  ellipse: motionShape<SvgEllipseProps>("ellipse"),
  rect: motionShape<SvgRectProps>("rect"),
  line: motionShape<SvgLineProps>("line"),
  polyline: motionShape<SvgPolyProps>("polyline"),
  polygon: motionShape<SvgPolyProps>("polygon"),
} as const

// There is no `VirtualList` React wrapper. Windowing on the React side is the
// app's job: pass `itemCount`, `estimatedItemHeight` and `windowStart` to the
// host `<virtual-list>` and render only that slice. A generic wrapper cannot
// know when to widen its own window, so it silently dropped rows whenever
// `itemCount` grew without a scroll.
