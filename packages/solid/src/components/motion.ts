import { createRenderEffect, mergeProps, onCleanup, splitProps, type JSX } from "solid-js"
import type { EventPayload } from "@gpuix/native"
import { reduceMotionValues } from "@gpuix/native/host"
import type {
  MotionProps,
  SvgCircleProps,
  SvgEllipseProps,
  SvgGroupProps,
  SvgLineProps,
  SvgMotionProps,
  SvgPathProps,
  SvgPolyProps,
  SvgRectProps,
} from "@gpuix/native/host"
import type { SolidHostProps } from "../jsx-runtime.js"
import { createElement, spread } from "../universal.js"
import { useContext } from "solid-js"
import { PresenceContext } from "./animate-presence.js"
import { MotionConfigContext } from "./motion-config.js"

export interface MotionDivProps extends SolidHostProps, MotionProps {}

/** A `motion.path`, `motion.circle`, ...: the shape's own props plus the animation. */
export type MotionShapeProps<Shape> = SolidHostProps<Shape> & SvgMotionProps

interface MotionSource {
  initial?: object | false
  animate?: object
  exit?: object
  transition?: object
}

function createMotionComponent<Props extends MotionSource & SolidHostProps<object>>(tag: string) {
  return function Motion(props: Props): JSX.Element {
    const [motionProps, hostProps] = splitProps(props as MotionSource & Record<string, unknown>, [
      "initial",
      "animate",
      "exit",
      "transition",
    ])
    const presence = useContext(PresenceContext)
    const config = useContext(MotionConfigContext)
    const reduced = () => config !== undefined && config.reduced()
    createRenderEffect(() => {
      if (!reduced()) return
      const release = reduceMotionValues((hostProps as { style?: unknown }).style)
      onCleanup(release)
    })
    createRenderEffect(() => {
      if (presence && !presence.isPresent() && !motionProps.exit) {
        presence.safeToRemove()
      }
    })
    const element = createElement(tag)
    let generation = 0
    let targetKey: string | undefined
    spread(element, mergeProps(hostProps, {
      get motion() {
        const isExit = Boolean(presence && !presence.isPresent() && motionProps.exit)
        const initial = presence?.initial === false ? false : motionProps.initial
        const animate = isExit ? motionProps.exit! : motionProps.animate
        const transition = motionProps.transition !== undefined ? motionProps.transition : config?.transition()
        const nextKey = JSON.stringify([isExit, initial, animate, transition, reduced()])
        if (nextKey !== targetKey) {
          targetKey = nextKey
          generation += 1
        }
        return {
          generation,
          isExit,
          initial,
          animate,
          exit: motionProps.exit,
          transition,
          ...(reduced() ? { reducedMotion: true } : {}),
        }
      },
      onMotionComplete(event: EventPayload) {
        if (event.motionGeneration !== generation) return
        ;(hostProps as { onMotionComplete?: (event: EventPayload) => void }).onMotionComplete?.(event)
        if (presence && !presence.isPresent()) presence.safeToRemove()
      },
    }))
    return element as never
  }
}

const MotionDiv = createMotionComponent<MotionDivProps>("div")

/**
 * Native animations. `motion.path`, `motion.circle` and the other SVG shapes
 * animate attributes (`pathLength`, `d`, `stroke`, ...) instead of style, and
 * are valid only inside `<svg>`.
 */
export const motion = {
  div: MotionDiv,
  g: createMotionComponent<MotionShapeProps<SvgGroupProps>>("g"),
  path: createMotionComponent<MotionShapeProps<SvgPathProps>>("path"),
  circle: createMotionComponent<MotionShapeProps<SvgCircleProps>>("circle"),
  ellipse: createMotionComponent<MotionShapeProps<SvgEllipseProps>>("ellipse"),
  rect: createMotionComponent<MotionShapeProps<SvgRectProps>>("rect"),
  line: createMotionComponent<MotionShapeProps<SvgLineProps>>("line"),
  polyline: createMotionComponent<MotionShapeProps<SvgPolyProps>>("polyline"),
  polygon: createMotionComponent<MotionShapeProps<SvgPolyProps>>("polygon"),
} as const
