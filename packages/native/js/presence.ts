/// What `AnimatePresence` means to the adapters, shared by React and Solid:
/// the `mode` prop, `exit` as a function of `custom`, and the box a
/// `popLayout` child keeps while it leaves.

import type { ElementBounds } from "../index.js"

/**
 * `"sync"`: entering and exiting children animate together. `"wait"`: a
 * child that enters while another is leaving renders only after every exit is
 * done. `"popLayout"`: a leaving child is taken out of flow, at the box it
 * last had inside its parent, so its siblings reflow at once.
 */
export type PresenceMode = "sync" | "wait" | "popLayout"

/**
 * An `exit` target, or a function that returns one from the `custom` value of
 * the `AnimatePresence` the child is leaving. Framer resolves `exit` the same
 * way from its variants.
 */
export type ExitTarget<Style extends object> =
  | Style
  // A method type so that `(direction: number) => Style` is assignable: `custom` is whatever the app passes.
  | { exit(custom: unknown): Style }["exit"]

export function resolveExit<Style extends object>(
  exit: ExitTarget<Style> | undefined,
  custom: unknown
): Style | undefined {
  return typeof exit === "function" ? exit(custom) : exit
}

/** Swaps a motion prop type's `exit` for an `ExitTarget` and adds `custom`. */
export type WithExitFunction<Props extends { exit?: object }> = Omit<Props, "exit"> & {
  exit?: ExitTarget<NonNullable<Props["exit"]>>
  /** Handed to an `exit` function when this child leaves; the `custom` of the `AnimatePresence` wins. */
  custom?: unknown
}

export type PopLayoutStyle = {
  position: "absolute"
  top: number
  left: number
  width: number
  height: number
  margin: 0
  marginTop: 0
  marginRight: 0
  marginBottom: 0
  marginLeft: 0
}

/** What `popLayoutStyle` needs to know of the parent: its box and how far its content is scrolled. */
export type PopLayoutParent = {
  bounds: ElementBounds
  scrollLeft: number
  scrollTop: number
}

/**
 * The style that paints `child` exactly where it painted before it left the
 * flow. An absolute box is placed from the parent's padding box (inside its
 * border), moves with the parent's scrolled content, and sits `margin` away
 * from its offsets, so the margin is zeroed rather than counted twice.
 * `parent` is `undefined` for a child of the window root, which sits at the
 * window origin.
 */
export function popLayoutStyle(child: ElementBounds, parent: PopLayoutParent | undefined): PopLayoutStyle {
  const originX = parent === undefined ? 0 : parent.bounds.x + parent.bounds.clientLeft - parent.scrollLeft
  const originY = parent === undefined ? 0 : parent.bounds.y + parent.bounds.clientTop - parent.scrollTop
  return {
    position: "absolute",
    top: child.y - originY,
    left: child.x - originX,
    width: child.width,
    height: child.height,
    margin: 0,
    marginTop: 0,
    marginRight: 0,
    marginBottom: 0,
    marginLeft: 0,
  }
}

type BoundsHost = {
  getElementBounds?: (elementId: number) => ElementBounds | null
  getScrollMetrics?: (elementId: number) => Array<number> | null
}

/**
 * The style that keeps an element where it last painted, or `undefined` when
 * it has not painted (nothing to hold in place).
 */
export function measurePopLayout(
  renderer: BoundsHost,
  childId: number,
  parentId: number | undefined
): PopLayoutStyle | undefined {
  if (!renderer.getElementBounds) {
    throw new Error('AnimatePresence mode="popLayout" needs a renderer with getElementBounds')
  }
  const child = renderer.getElementBounds(childId)
  if (child === null) return undefined
  if (parentId === undefined) return popLayoutStyle(child, undefined)
  if (!renderer.getScrollMetrics) {
    throw new Error('AnimatePresence mode="popLayout" needs a renderer with getScrollMetrics')
  }
  const bounds = renderer.getElementBounds(parentId)
  const metrics = renderer.getScrollMetrics(parentId)
  if (bounds === null || metrics === null) {
    throw new Error(`AnimatePresence mode="popLayout": parent element ${parentId} has not painted`)
  }
  return popLayoutStyle(child, { bounds, scrollLeft: metrics[0], scrollTop: metrics[1] })
}
