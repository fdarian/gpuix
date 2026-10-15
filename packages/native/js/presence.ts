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
}

/** `parent` is `undefined` for a child of the window root, which sits at the window origin. */
export function popLayoutStyle(child: ElementBounds, parent: ElementBounds | undefined): PopLayoutStyle {
  return {
    position: "absolute",
    top: child.y - (parent === undefined ? 0 : parent.y),
    left: child.x - (parent === undefined ? 0 : parent.x),
    width: child.width,
    height: child.height,
  }
}

type BoundsHost = {
  getElementBounds?: (elementId: number) => ElementBounds | null
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
  const parent = renderer.getElementBounds(parentId)
  if (parent === null) throw new Error(`AnimatePresence mode="popLayout": parent element ${parentId} has not painted`)
  return popLayoutStyle(child, parent)
}
