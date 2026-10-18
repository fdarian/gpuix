import { createRenderEffect, mergeProps, onCleanup, splitProps, type JSX } from "solid-js"
import type { DomEvent, StyleDesc } from "@gpuix/native/host"
import {
  assertMotionSpanProps,
  layoutDescription,
  reduceMotionValues,
  resolveExit,
  resolveSpanMotion,
  scopeLayoutId,
  withoutLayoutTransition,
  wireMotionTransition,
} from "@gpuix/native/host"
import type {
  ExitTarget,
  LayoutDrag,
  MotionLayoutProps,
  MotionProps,
  MotionSpanAnimation,
  MotionSpanUnsupportedProps,
  MotionTransition,
  SvgMotionTransition,
  SvgCircleProps,
  SvgEllipseProps,
  SvgGroupProps,
  SvgLineProps,
  SvgMotionProps,
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
  WithExitFunction,
} from "@gpuix/native/host"
import type { SolidHostProps } from "../jsx-runtime.js"
import { createElement, spread } from "../universal.js"
import type { HostElement } from "../host.js"
import { useContext } from "solid-js"
import { PresenceGroupContext, useIsPresent } from "./animate-presence.js"
import { LayoutGroupContext } from "./layout-group.js"
import { MotionConfigContext } from "./motion-config.js"

export interface MotionDivProps extends SolidHostProps, WithExitFunction<MotionProps>, MotionLayoutProps {}

/**
 * Props of a `motion.span`: an inline span animates its colours (`animate`,
 * `initial`, `exit`, or the `variants` they name) and takes the props of a
 * `<span>`. It has no box to measure or drag, so `layout`, `layoutId`, `drag*`
 * and `while*` are errors.
 */
export type MotionSpanProps = Omit<
  SolidHostProps,
  "style" | keyof MotionSpanAnimation | keyof MotionSpanUnsupportedProps
> &
  MotionSpanAnimation &
  MotionSpanUnsupportedProps & { style?: StyleDesc }

/** A `motion.path`, `motion.circle`, ...: the shape's own props plus the animation. */
export type MotionShapeProps<Shape> = SolidHostProps<Shape> & WithExitFunction<SvgMotionProps>

interface MotionSource {
  initial?: object | MotionSpanAnimation["initial"]
  animate?: object | MotionSpanAnimation["animate"]
  exit?: ExitTarget<object> | MotionSpanAnimation["exit"]
  variants?: MotionSpanAnimation["variants"]
  custom?: unknown
  transition?: MotionTransition | SvgMotionTransition
  layout?: MotionLayoutProps["layout"]
  layoutId?: string
  /** The drag values of a `Reorder.Item`; not part of the API. */
  layoutDrag?: LayoutDrag
}

/** `motion.div` plus the drag values `Reorder.Item` hands it, for the element type it is `as`. */
export type MotionDragProps = MotionDivProps & { layoutDrag?: LayoutDrag }

export function createMotionComponent<Props extends MotionSource & SolidHostProps<object>>(tag: string) {
  const inline = tag === "span"
  return function Motion(props: Props): JSX.Element {
    if (inline) assertMotionSpanProps(props as Record<string, unknown>)
    else if ((props as MotionSource).variants !== undefined) throw new Error("variants is only supported by motion.span")
    const [motionProps, hostProps] = splitProps(props as MotionSource & Record<string, unknown>, [
      "initial",
      "animate",
      "exit",
      "variants",
      "custom",
      "transition",
      "layout",
      "layoutId",
      "layoutDrag",
    ])
    const isPresent = useIsPresent()
    const group = useContext(PresenceGroupContext)
    const config = useContext(MotionConfigContext)
    const layoutGroupId = useContext(LayoutGroupContext)
    const reduced = () => config !== undefined && config.reduced()
    // An `exit` function has nothing to leave from outside an `AnimatePresence`.
    const declaredExit = () =>
      typeof motionProps.exit === "function" && !group
        ? undefined
        : resolveExit(motionProps.exit as ExitTarget<object> | undefined, group?.custom() ?? motionProps.custom)
    const span = () =>
      resolveSpanMotion({
        initial: motionProps.initial as MotionSpanAnimation["initial"],
        animate: motionProps.animate as MotionSpanAnimation["animate"],
        exit: declaredExit() as Parameters<typeof resolveSpanMotion>[0]["exit"],
        variants: motionProps.variants,
        custom: group?.custom() ?? motionProps.custom,
      })
    const exit = () => (inline ? span().exit : declaredExit())
    createRenderEffect(() => {
      if (!reduced()) return
      const release = reduceMotionValues((hostProps as { style?: unknown }).style)
      onCleanup(release)
    })
    const element = createElement(tag)
    let generation = 0
    let targetKey: string | undefined
    let applied: object | undefined
    let leaving = false
    const resolvedTransition = (variantTransition?: MotionTransition) =>
      wireMotionTransition(
        variantTransition ?? (motionProps.transition !== undefined ? motionProps.transition : config?.transition())
      )
    spread(element, mergeProps(hostProps, {
      get layout() {
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
        const resolved = inline ? span() : undefined
        const initial = resolved ? resolved.initial : motionProps.initial
        const animate = resolved ? resolved.animate : motionProps.animate
        if (animate === undefined) return applied = undefined
        const transition = withoutLayoutTransition(resolvedTransition(resolved?.animateTransition))
        const nextKey = JSON.stringify([initial, animate, transition, reduced()])
        if (nextKey !== targetKey) {
          targetKey = nextKey
          generation += 1
        }
        return applied = {
          generation,
          isExit: false,
          initial,
          animate,
          exit: exit(),
          transition,
          ...(reduced() ? { reducedMotion: true } : {}),
        }
      },
      onMotionComplete(event: DomEvent) {
        if (event.motionGeneration !== generation) return
        ;(hostProps as { onMotionComplete?: (event: DomEvent) => void }).onMotionComplete?.(event)
      },
    }))
    return element as never
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
