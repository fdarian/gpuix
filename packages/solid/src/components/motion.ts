import {
  createComponent,
  createEffect,
  createMemo,
  createRenderEffect,
  createSignal,
  mergeProps,
  onCleanup,
  splitProps,
  type JSX,
} from "solid-js"
import type { DomEvent, MotionValueStyle } from "@gpuix/native/host"
import {
  elementScope,
  gestureDescription,
  gestureHandlers,
  IGNORED_SPAN_PROPS,
  layoutDescription,
  motionComponentStyle,
  observeViewport,
  reduceMotionValues,
  resolveExit,
  resolveMotion,
  scopeLayoutId,
  SPAN_SCOPE,
  withoutIgnoredSpanStyle,
  withoutLayoutTransition,
  wireMotionTransition,
} from "@gpuix/native/host"
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
  SvgMotionStyle,
  SvgMotionTransition,
  SvgCircleProps,
  SvgEllipseProps,
  SvgGroupProps,
  SvgLineProps,
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
} from "@gpuix/native/host"
import type { SolidHostProps } from "../jsx-runtime.js"
import { createElement, spread } from "../universal.js"
import type { HostElement } from "../host.js"
import { useContext } from "solid-js"
import { PresenceGroupContext, useIsPresent } from "./animate-presence.js"
import { LayoutGroupContext } from "./layout-group.js"
import { MotionConfigContext } from "./motion-config.js"
import { VariantContext } from "./variant-context.js"

export interface MotionDivProps
  extends SolidHostProps,
    MotionAnimation<MotionStyle>,
    MotionGestureCallbacks,
    MotionLayoutProps {}

/**
 * Props of a `motion.span`: an inline span animates what an inline box shows (`animate`, `initial`,
 * `exit`, `whileHover`, `whileTap`, `whileFocus`, `whileInView`, or the `variants` they name) and takes
 * the props of a `<span>`. A non-atomic inline box takes no transform, so `layout`, `layoutId`, `drag*`
 * and the transform keys are accepted and have no effect, as in a browser.
 */
export type MotionSpanProps = Omit<
  SolidHostProps,
  "style" | keyof MotionSpanAnimation | keyof MotionGestureCallbacks | keyof MotionSpanIgnoredProps
> &
  MotionSpanAnimation &
  MotionGestureCallbacks &
  MotionSpanIgnoredProps & { style?: MotionValueStyle }

/** A `motion.path`, `motion.circle`, ...: the shape's own props plus the animation. */
export type MotionShapeProps<Shape> = SolidHostProps<Shape> &
  MotionAnimation<SvgMotionStyle, SvgMotionTransition> &
  Pick<MotionGestureCallbacks, "onViewportEnter" | "onViewportLeave">

interface MotionSource extends MotionAnimation<object, MotionTransition | SvgMotionTransition>, MotionGestureCallbacks {
  layout?: MotionLayoutProps["layout"]
  layoutId?: string
  /** The drag values of a `Reorder.Item`; not part of the API. */
  layoutDrag?: LayoutDrag
}

/** `motion.div` plus the drag values `Reorder.Item` hands it, for the element type it is `as`. */
export type MotionDragProps = MotionDivProps & { layoutDrag?: LayoutDrag }

export function createMotionComponent<Props extends MotionSource & SolidHostProps<object>>(tag: string) {
  const inline = tag === "span"
  const scope = inline ? SPAN_SCOPE : elementScope(`motion.${tag}`)
  return function Motion(props: Props): JSX.Element {
    const [motionProps, ownProps] = splitProps(props as MotionSource & Record<string, unknown>, [
      "initial",
      "animate",
      "exit",
      "variants",
      "inherit",
      "custom",
      "transition",
      "layout",
      "layoutId",
      "layoutDrag",
      "whileHover",
      "whileTap",
      "whileFocus",
      "whileInView",
      "viewport",
      "onHoverStart",
      "onHoverEnd",
      "onTapStart",
      "onTap",
      "onTapCancel",
      "onViewportEnter",
      "onViewportLeave",
    ])
    // A span takes no layout, drag or transform: what Framer would move with one is left where it is.
    const [, withoutIgnored] = splitProps(ownProps, inline ? (IGNORED_SPAN_PROPS as string[]) : [])
    const hostProps = mergeProps(withoutIgnored, {
      get style() {
        const style = (ownProps as { style?: unknown }).style
        return inline ? withoutIgnoredSpanStyle(style) : motionComponentStyle(style)
      },
    })
    const isPresent = useIsPresent()
    const group = useContext(PresenceGroupContext)
    const config = useContext(MotionConfigContext)
    const layoutGroupId = useContext(LayoutGroupContext)
    const parentVariants = useContext(VariantContext)
    const inheritedVariants = () => (motionProps.inherit === false ? undefined : parentVariants?.())
    const reduced = () => config !== undefined && config.reduced()
    // An `exit` function has nothing to leave from outside an `AnimatePresence`.
    const declaredExit = () =>
      typeof motionProps.exit === "function" && !group
        ? undefined
        : resolveExit(motionProps.exit as ExitTarget<object> | undefined, group?.custom() ?? motionProps.custom)
    const [inView, setInView] = createSignal(false)
    const resolved = createMemo(() =>
      resolveMotion(scope, {
        initial: motionProps.initial as MotionInput["initial"],
        animate: motionProps.animate as MotionInput["animate"],
        exit: declaredExit() as MotionInput["exit"],
        variants: motionProps.variants as MotionInput["variants"],
        custom: group?.custom() ?? motionProps.custom,
        gestures: {
          whileHover: motionProps.whileHover,
          whileTap: motionProps.whileTap,
          whileFocus: motionProps.whileFocus,
          whileInView: motionProps.whileInView,
        },
        parent: inheritedVariants(),
        inView: inView(),
      })
    )
    const handedDown = createMemo(() => resolved().controls ?? inheritedVariants(), undefined, {
      equals: (a, b) => JSON.stringify(a) === JSON.stringify(b),
    })
    const exit = () => resolved().exit
    createRenderEffect(() => {
      if (!reduced()) return
      const release = reduceMotionValues((hostProps as { style?: unknown }).style)
      onCleanup(release)
    })
    const element = createElement(tag)
    createEffect(() => {
      const watches =
        motionProps.whileInView !== undefined ||
        motionProps.onViewportEnter !== undefined ||
        motionProps.onViewportLeave !== undefined
      if (!watches || !isPresent()) return
      onCleanup(
        observeViewport(element as never, motionProps.viewport, (visible, entry) => {
          setInView(visible)
          if (visible) motionProps.onViewportEnter?.(entry)
          else motionProps.onViewportLeave?.(entry)
        })
      )
    })
    let generation = 0
    let targetKey: string | undefined
    let applied: object | undefined
    let leaving = false
    const resolvedTransition = (variantTransition?: MotionTransition) =>
      wireMotionTransition(
        variantTransition ?? (motionProps.transition !== undefined ? motionProps.transition : config?.transition())
      )
    const pointerHandlers = () =>
      gestureHandlers(
        () => ({
          onHoverStart: motionProps.onHoverStart,
          onHoverEnd: motionProps.onHoverEnd,
          onTapStart: motionProps.onTapStart,
          onTap: motionProps.onTap,
          onTapCancel: motionProps.onTapCancel,
        }),
        hostProps as MotionGestureHandlers
      )
    return createComponent(VariantContext.Provider, {
      value: handedDown,
      // The element's children are read here, under the provider, so they see what it hands down.
      get children() {
        spread(element, mergeProps(hostProps, {
          get onPointerEnter() {
            return pointerHandlers().onPointerEnter ?? (hostProps as MotionGestureHandlers).onPointerEnter
          },
          get onPointerLeave() {
            return pointerHandlers().onPointerLeave ?? (hostProps as MotionGestureHandlers).onPointerLeave
          },
          get onPointerDown() {
            return pointerHandlers().onPointerDown ?? (hostProps as MotionGestureHandlers).onPointerDown
          },
          get layout() {
            // A layout animation moves a box with a transform, which a span has none of.
            if (inline) return undefined
            return layoutDescription({
              layout: motionProps.layout as MotionLayoutProps["layout"],
              layoutId: scopeLayoutId(layoutGroupId?.(), motionProps.layoutId as string | undefined),
              transition: resolvedTransition() as MotionTransition | undefined,
              reduced: reduced(),
              drag: motionProps.layoutDrag,
            })
          },
          get motion() {
            // `AnimatePresence` sets the exit target of a leaving element, and this
            // component stays alive while it leaves: following its props again
            // would undo that.
            if (!isPresent()) {
              leaving = true
              return applied
            }
            if (leaving) {
              leaving = false
              targetKey = undefined
              generation = ((element as HostElement).props.get("motion") as { generation?: number } | undefined)?.generation ?? generation
            }
            const current = resolved()
            const gestures = gestureDescription(
              current,
              (own) => wireMotionTransition(own),
              current.inherited.includes("whileInView") ? inheritedVariants()!.inView : inView()
            )
            const animate = current.animate ?? (gestures !== undefined ? {} : undefined)
            if (animate === undefined) return applied = undefined
            const transition = withoutLayoutTransition(resolvedTransition(current.animateTransition))
            const nextKey = JSON.stringify([current.initial, animate, transition, reduced(), gestures])
            if (nextKey !== targetKey) {
              targetKey = nextKey
              generation += 1
            }
            return applied = {
              generation,
              isExit: false,
              initial: current.initial,
              animate,
              exit: exit(),
              transition,
              ...(reduced() ? { reducedMotion: true } : {}),
              ...gestures,
            }
          },
          onMotionComplete(event: DomEvent) {
            if (event.motionGeneration !== generation) return
            ;(hostProps as { onMotionComplete?: (event: DomEvent) => void }).onMotionComplete?.(event)
          },
        }))
        return element as never
      },
    }) as never
  }
}

const MotionDiv = createMotionComponent<MotionDivProps>("div")
const MotionSpan = createMotionComponent<MotionSpanProps>("span")

/**
 * Native animations. `motion.path`, `motion.circle` and the other SVG shapes
 * animate attributes (`pathLength`, `d`, `stroke`, ...) instead of style, and
 * are valid only inside `<svg>`.
 */
export const motion = {
  div: MotionDiv,
  span: MotionSpan,
  g: createMotionComponent<MotionShapeProps<SvgGroupProps>>("g"),
  path: createMotionComponent<MotionShapeProps<SvgPathProps>>("path"),
  circle: createMotionComponent<MotionShapeProps<SvgCircleProps>>("circle"),
  ellipse: createMotionComponent<MotionShapeProps<SvgEllipseProps>>("ellipse"),
  rect: createMotionComponent<MotionShapeProps<SvgRectProps>>("rect"),
  line: createMotionComponent<MotionShapeProps<SvgLineProps>>("line"),
  polyline: createMotionComponent<MotionShapeProps<SvgPolyProps>>("polyline"),
  polygon: createMotionComponent<MotionShapeProps<SvgPolyProps>>("polygon"),
  text: createMotionComponent<MotionShapeProps<SvgTextProps>>("text"),
  tspan: createMotionComponent<MotionShapeProps<SvgTspanProps>>("tspan"),
  image: createMotionComponent<MotionShapeProps<SvgImageProps>>("image"),
  feGaussianBlur: createMotionComponent<MotionShapeProps<SvgFeGaussianBlurProps>>("feGaussianBlur"),
  feOffset: createMotionComponent<MotionShapeProps<SvgFeOffsetProps>>("feOffset"),
  feFlood: createMotionComponent<MotionShapeProps<SvgFeFloodProps>>("feFlood"),
  feDropShadow: createMotionComponent<MotionShapeProps<SvgFeDropShadowProps>>("feDropShadow"),
} as const
